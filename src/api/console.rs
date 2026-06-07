use std::collections::{BTreeSet, HashMap};
use std::net::SocketAddr;
use std::time::Instant;

use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, Extension, Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use futures::TryStreamExt;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::auth::signature_v4;
use crate::server::AppState;
use crate::storage::Storage;

type HmacSha256 = Hmac<Sha256>;

const COOKIE_NAME: &str = "maxio_session";
const TOKEN_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60; // 7 days

const RATE_LIMIT_MAX: u32 = 10;
const RATE_LIMIT_WINDOW_SECS: u64 = 300; // 5 minutes

struct Bucket {
    count: u32,
    window_start: Instant,
}

pub struct LoginRateLimiter {
    buckets: std::sync::Mutex<HashMap<String, Bucket>>,
}

impl LoginRateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Returns `Some(retry_after_secs)` if the IP is rate-limited, `None` if allowed.
    /// Increments the counter on every call (success and failure both count).
    pub fn check_and_increment(&self, ip: &str) -> Option<u64> {
        let mut map = self.buckets.lock().unwrap();
        let now = Instant::now();

        // Prune expired entries to prevent unbounded memory growth
        map.retain(|_, b| {
            now.duration_since(b.window_start).as_secs() < RATE_LIMIT_WINDOW_SECS * 2
        });

        let bucket = map.entry(ip.to_string()).or_insert(Bucket {
            count: 0,
            window_start: now,
        });

        if now.duration_since(bucket.window_start).as_secs() >= RATE_LIMIT_WINDOW_SECS {
            bucket.count = 0;
            bucket.window_start = now;
        }

        bucket.count += 1;

        if bucket.count > RATE_LIMIT_MAX {
            let remaining = RATE_LIMIT_WINDOW_SECS
                .saturating_sub(now.duration_since(bucket.window_start).as_secs());
            Some(remaining.max(1))
        } else {
            None
        }
    }
}

fn extract_client_ip(headers: &HeaderMap, addr: &SocketAddr) -> String {
    let _ = headers;
    // Public console: do not trust spoofable X-Forwarded-For unless/until a
    // trusted-proxy allowlist is configured. Use the connected peer IP.
    addr.ip().to_string()
}

fn generate_token(username: &str, secret_key: &str, issued_at: i64) -> String {
    let issued_hex = format!("{:x}", issued_at);
    let mut mac =
        HmacSha256::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(format!("{}:{}", username, issued_hex).as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    format!("{}.{}", issued_hex, sig)
}

fn verify_token(token: &str, username: &str, secret_key: &str) -> bool {
    let Some((issued_hex, signature)) = token.split_once('.') else {
        return false;
    };

    let Ok(issued_at) = i64::from_str_radix(issued_hex, 16) else {
        return false;
    };

    let now = chrono::Utc::now().timestamp();
    if now - issued_at > TOKEN_MAX_AGE_SECS || issued_at > now + 60 {
        return false;
    }

    let mut mac =
        HmacSha256::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(format!("{}:{}", username, issued_hex).as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());

    constant_time_eq(signature.as_bytes(), expected.as_bytes())
}

async fn resolve_session_username(token: &str, state: &AppState) -> Option<String> {
    if verify_token(token, crate::iam::ROOT_USERNAME, &state.config.secret_key) {
        return Some(crate::iam::ROOT_USERNAME.to_string());
    }
    for user in state.user_store.list_users().await {
        if verify_token(token, &user.username, &state.config.secret_key) {
            return Some(user.username);
        }
    }
    None
}

#[derive(Clone, Debug)]
pub struct ConsoleSession {
    pub username: String,
    pub is_root: bool,
    pub user_id: String,
}

impl ConsoleSession {
    fn root() -> Self {
        Self {
            username: crate::iam::ROOT_USERNAME.to_string(),
            is_root: true,
            user_id: crate::iam::ROOT_CANONICAL_ID.to_string(),
        }
    }

    fn from_user(user: &crate::iam::types::IamUser) -> Self {
        Self {
            username: user.username.clone(),
            is_root: false,
            user_id: user.user_id.clone(),
        }
    }

    pub fn principal(&self) -> crate::iam::Principal {
        if self.is_root {
            crate::iam::Principal::root()
        } else {
            crate::iam::Principal {
                username: self.username.clone(),
                user_id: self.user_id.clone(),
                display_name: self.username.clone(),
                canonical_id: self.user_id.clone(),
                is_root: false,
                is_anonymous: false,
            }
        }
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn extract_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies
                .split(';')
                .map(|c| c.trim())
                .find(|c| c.starts_with(&format!("{}=", COOKIE_NAME)))
                .map(|c| c[COOKIE_NAME.len() + 1..].to_string())
        })
}

fn make_cookie(value: &str, max_age: i64, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };

    format!(
        "{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}",
        COOKIE_NAME, value, max_age, secure_flag
    )
}

async fn console_auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let session = match extract_cookie(request.headers()) {
        Some(token) => match resolve_session_username(&token, &state).await {
            Some(username) if username == crate::iam::ROOT_USERNAME => Some(ConsoleSession::root()),
            Some(username) => state
                .user_store
                .get_user(&username)
                .await
                .map(|u| ConsoleSession::from_user(&u))
                .or(Some(ConsoleSession::root())),
            None => None,
        },
        None => None,
    };

    let Some(session) = session else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Not authenticated"})),
        )
            .into_response();
    };

    request.extensions_mut().insert(session);
    next.run(request).await
}

type ConsoleDeny = (StatusCode, Json<serde_json::Value>);

fn console_forbidden() -> ConsoleDeny {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"error": "Access Denied"})),
    )
}

async fn console_check(
    state: &AppState,
    session: &ConsoleSession,
    action: &str,
    resource: &str,
    bucket_policy: Option<&str>,
    bucket_acl: Option<&crate::iam::Acl>,
) -> Result<(), ConsoleDeny> {
    crate::iam::authz::authorize(
        state,
        &session.principal(),
        action,
        resource,
        bucket_policy,
        bucket_acl,
        None,
    )
    .await
    .map_err(|_| console_forbidden())
}

async fn console_can(
    state: &AppState,
    session: &ConsoleSession,
    action: &str,
    resource: &str,
) -> bool {
    console_check(state, session, action, resource, None, None)
        .await
        .is_ok()
}

async fn console_bucket_check(
    state: &AppState,
    session: &ConsoleSession,
    bucket: &str,
    action: &str,
) -> Result<(), Response> {
    match state.storage.get_bucket_auth_info(bucket).await {
        Ok((policy, acl)) => console_check(
            state,
            session,
            action,
            &crate::iam::authz::bucket_arn(bucket),
            policy.as_deref(),
            Some(&acl),
        )
        .await
        .map_err(|deny| deny.into_response()),
        Err(_) if session.is_root => Ok(()),
        Err(_) => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Bucket not found"})),
        )
            .into_response()),
    }
}

async fn console_object_check(
    state: &AppState,
    session: &ConsoleSession,
    bucket: &str,
    key: &str,
    action: &str,
) -> Result<(), Response> {
    match state.storage.get_bucket_auth_info(bucket).await {
        Ok((policy, acl)) => console_check(
            state,
            session,
            action,
            &crate::iam::authz::object_arn(bucket, key),
            policy.as_deref(),
            Some(&acl),
        )
        .await
        .map_err(|deny| deny.into_response()),
        Err(_) if session.is_root => Ok(()),
        Err(_) => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Bucket not found"})),
        )
            .into_response()),
    }
}

async fn session_capabilities(state: &AppState, session: &ConsoleSession) -> serde_json::Value {
    serde_json::json!({
        "canCreateBucket": session.is_root
            || console_can(state, session, "s3:CreateBucket", "arn:aws:s3:::*").await,
        "canListAllBuckets": session.is_root
            || console_can(state, session, "s3:ListAllMyBuckets", "arn:aws:s3:::*").await,
        "canManageUsers": session.is_root,
    })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    access_key: String,
    secret_key: String,
}

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> impl IntoResponse {
    let ip = extract_client_ip(&headers, &addr);

    if let Some(retry_after) = state.login_rate_limiter.check_and_increment(&ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(axum::http::header::RETRY_AFTER, retry_after.to_string())],
            Json(serde_json::json!({"error": "Too many login attempts. Try again later."})),
        )
            .into_response();
    }

    // Use constant-time comparison to prevent timing side-channel attacks
    let key_match = constant_time_eq(
        body.access_key.as_bytes(),
        state.config.access_key.as_bytes(),
    );
    let secret_match = constant_time_eq(
        body.secret_key.as_bytes(),
        state.config.secret_key.as_bytes(),
    );
    let session_username = if key_match && secret_match {
        crate::iam::ROOT_USERNAME.to_string()
    } else if let Some(user) = state
        .user_store
        .lookup_by_credentials(&body.access_key, &body.secret_key)
        .await
    {
        user.username
    } else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid credentials"})),
        )
            .into_response();
    };

    let now = chrono::Utc::now().timestamp();
    let token = generate_token(&session_username, &state.config.secret_key, now);
    let cookie = make_cookie(
        &token,
        TOKEN_MAX_AGE_SECS,
        state.config.secure_cookies && !state.config.allow_insecure_dev,
    );

    let mut resp_headers = HeaderMap::new();
    resp_headers.insert("Set-Cookie", cookie.parse().unwrap());

    let session = if session_username == crate::iam::ROOT_USERNAME {
        ConsoleSession::root()
    } else {
        state
            .user_store
            .get_user(&session_username)
            .await
            .map(|u| ConsoleSession::from_user(&u))
            .unwrap_or_else(ConsoleSession::root)
    };

    (
        StatusCode::OK,
        resp_headers,
        Json(serde_json::json!({
            "ok": true,
            "username": session_username,
            "isRoot": session.is_root,
            "capabilities": session_capabilities(&state, &session).await,
        })),
    )
        .into_response()
}

pub async fn check(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let authenticated = match extract_cookie(&headers) {
        Some(token) => resolve_session_username(&token, &state).await,
        None => None,
    };

    if let Some(username) = authenticated {
        let session = if username == crate::iam::ROOT_USERNAME {
            ConsoleSession::root()
        } else {
            state
                .user_store
                .get_user(&username)
                .await
                .map(|u| ConsoleSession::from_user(&u))
                .unwrap_or_else(ConsoleSession::root)
        };
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "username": username,
                "isRoot": session.is_root,
                "capabilities": session_capabilities(&state, &session).await,
            })),
        )
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Not authenticated"})),
        )
    }
}

pub async fn logout(State(state): State<AppState>) -> impl IntoResponse {
    let cookie = make_cookie(
        "",
        0,
        state.config.secure_cookies && !state.config.allow_insecure_dev,
    );
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert("Set-Cookie", cookie.parse().unwrap());
    (
        StatusCode::OK,
        resp_headers,
        Json(serde_json::json!({"ok": true})),
    )
}

async fn console_csrf_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let mutating = matches!(
        method,
        axum::http::Method::POST
            | axum::http::Method::PUT
            | axum::http::Method::PATCH
            | axum::http::Method::DELETE
    );
    if mutating {
        let headers = request.headers();
        let host = headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let origin = headers
            .get("origin")
            .and_then(|v| v.to_str().ok())
            .or_else(|| headers.get("referer").and_then(|v| v.to_str().ok()));
        if let Some(origin) = origin {
            if !same_origin_host(origin, host) && !dev_loopback_origin_allowed(&state, origin, host)
            {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"error": "CSRF origin check failed"})),
                )
                    .into_response();
            }
        }
    }
    let mut response = next.run(request).await;
    apply_security_headers(response.headers_mut());
    response
}

fn same_origin_host(origin_or_referer: &str, host: &str) -> bool {
    origin_host(origin_or_referer)
        .map(|h| h.eq_ignore_ascii_case(host))
        .unwrap_or(false)
}

fn dev_loopback_origin_allowed(state: &AppState, origin_or_referer: &str, host: &str) -> bool {
    state.config.allow_insecure_dev
        && origin_host(origin_or_referer)
            .map(|origin_host| is_loopback_host(origin_host) && is_loopback_host(host))
            .unwrap_or(false)
}

fn origin_host(origin_or_referer: &str) -> Option<&str> {
    origin_or_referer
        .strip_prefix("https://")
        .or_else(|| origin_or_referer.strip_prefix("http://"))
        .and_then(|rest| rest.split('/').next())
}

fn is_loopback_host(host_with_optional_port: &str) -> bool {
    let host = host_with_optional_port
        .strip_prefix('[')
        .and_then(|rest| rest.split(']').next())
        .unwrap_or_else(|| {
            host_with_optional_port
                .split(':')
                .next()
                .unwrap_or(host_with_optional_port)
        });

    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn apply_security_headers(headers: &mut HeaderMap) {
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("referrer-policy", "same-origin".parse().unwrap());
    headers.insert("x-frame-options", "DENY".parse().unwrap());
}

pub async fn list_buckets(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
) -> impl IntoResponse {
    match state.storage.list_buckets().await {
        Ok(buckets) => {
            let buckets = crate::iam::authz::filter_buckets_by_access(
                &state,
                &session.principal(),
                buckets,
            )
            .await;
            let list: Vec<serde_json::Value> = buckets
                .into_iter()
                .map(|b| {
                    let stat = state.stats.get(&b.name);
                    serde_json::json!({
                        "name": b.name,
                        "createdAt": b.created_at,
                        "versioning": b.versioning,
                        "objectCount": stat.as_ref().map(|s| s.object_count),
                        "sizeBytes": stat.as_ref().map(|s| s.size_bytes),
                    })
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({ "buckets": list }))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct CreateBucketRequest {
    name: String,
}

pub async fn create_bucket(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Json(body): Json<CreateBucketRequest>,
) -> impl IntoResponse {
    if let Err(deny) = console_check(
        &state,
        &session,
        "s3:CreateBucket",
        &crate::iam::authz::bucket_arn(&body.name),
        None,
        None,
    )
    .await
    {
        return deny.into_response();
    }

    if crate::storage::validate_bucket_name(&body.name).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid bucket name"})),
        )
            .into_response();
    }
    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let (owner_id, owner_display_name) = if session.is_root {
        (
            crate::iam::ROOT_CANONICAL_ID.to_string(),
            crate::iam::ROOT_DISPLAY_NAME.to_string(),
        )
    } else {
        (session.user_id.clone(), session.username.clone())
    };
    let meta = crate::storage::BucketMeta {
        name: body.name.clone(),
        created_at: now,
        region: state.config.region.clone(),
        versioning: false,
        cors_rules: None,
        owner_id: owner_id.clone(),
        owner_display_name: owner_display_name.clone(),
        acl: Some(crate::iam::Acl::private(
            &owner_id,
            &owner_display_name,
        )),
        policy: None,
        public_read: false,
        public_list: false,
    };

    match state.storage.create_bucket(&meta).await {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Ok(false) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Bucket already exists"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn delete_bucket_api(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_bucket_check(&state, &session, &bucket, "s3:DeleteBucket").await
    {
        return resp;
    }

    match state.storage.delete_bucket(&bucket).await {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Bucket not found"})),
        )
            .into_response(),
        Err(crate::storage::StorageError::BucketNotEmpty) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Bucket is not empty"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct ListObjectsParams {
    prefix: Option<String>,
    delimiter: Option<String>,
}

pub async fn list_objects(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Query(params): Query<ListObjectsParams>,
) -> impl IntoResponse {
    if let Err(resp) = console_bucket_check(&state, &session, &bucket, "s3:ListBucket").await {
        return resp;
    }

    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Bucket not found"})),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    }

    let prefix = params.prefix.unwrap_or_default();
    let delimiter = params.delimiter.unwrap_or_else(|| "/".to_string());

    let all_objects = match crate::storage::list_objects_all(state.storage.as_ref(), &bucket, &prefix).await {
        Ok(objects) => objects,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    let mut files = Vec::new();
    let mut prefix_set = BTreeSet::new();

    for obj in &all_objects {
        let suffix = &obj.key[prefix.len()..];
        if let Some(pos) = suffix.find(delimiter.as_str()) {
            let common = format!("{}{}", prefix, &suffix[..pos + delimiter.len()]);
            prefix_set.insert(common);
        } else if !obj.key.ends_with('/') {
            files.push(serde_json::json!({
                "key": obj.key,
                "size": obj.size,
                "lastModified": obj.last_modified,
                "etag": obj.etag,
            }));
        }
    }

    // Determine which prefixes are empty (only contain a folder marker, no real objects)
    let mut empty_prefixes: Vec<&String> = Vec::new();
    for p in &prefix_set {
        let has_children = all_objects
            .iter()
            .any(|obj| obj.key.starts_with(p.as_str()) && obj.key != *p);
        if !has_children {
            empty_prefixes.push(p);
        }
    }

    let prefixes: Vec<&String> = prefix_set.iter().collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "files": files,
            "prefixes": prefixes,
            "emptyPrefixes": empty_prefixes,
        })),
    )
        .into_response()
}

pub async fn upload_object(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path((bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> impl IntoResponse {
    if let Err(resp) =
        console_object_check(&state, &session, &bucket, &key, "s3:PutObject").await
    {
        return resp;
    }

    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Bucket not found"})),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    }

    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");

    let stream = body.into_data_stream();
    let reader = tokio_util::io::StreamReader::new(
        stream.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)),
    );

    match state
        .storage
        .put_object(
            &bucket,
            &key,
            content_type,
            Box::pin(reader),
            None,
        )
        .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "etag": result.etag,
                "size": result.size,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn delete_object_api(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path((bucket, key)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_object_check(&state, &session, &bucket, &key, "s3:DeleteObject").await
    {
        return resp;
    }

    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Bucket not found"})),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    }

    match state.storage.delete_object(&bucket, &key).await {
        Ok(_) => {
            if let Err(e) =
                preserve_empty_parent_folder_after_object_delete(state.storage.as_ref(), &bucket, &key)
                    .await
            {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e})),
                )
                    .into_response();
            }
            (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

fn parent_folder_prefix_for_deleted_object(key: &str) -> Option<String> {
    if key.ends_with('/') {
        return None;
    }
    key.rfind('/')
        .map(|idx| key[..=idx].to_string())
        .filter(|prefix| !prefix.is_empty())
}

async fn preserve_empty_parent_folder_after_object_delete(
    storage: &dyn Storage,
    bucket: &str,
    key: &str,
) -> Result<(), String> {
    let Some(parent_prefix) = parent_folder_prefix_for_deleted_object(key) else {
        return Ok(());
    };

    let remaining = crate::storage::list_objects_all(storage, bucket, &parent_prefix)
        .await
        .map_err(|e| e.to_string())?;

    let parent_still_exists = remaining
        .iter()
        .any(|obj| obj.key == parent_prefix || obj.key.starts_with(&parent_prefix));
    if parent_still_exists {
        return Ok(());
    }

    storage
        .put_object(
            bucket,
            &parent_prefix,
            "application/x-directory",
            Box::pin(tokio::io::empty()),
            None,
        )
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub async fn download_object(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path((bucket, key)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_object_check(&state, &session, &bucket, &key, "s3:GetObject").await
    {
        return resp;
    }

    let (reader, meta) = match state.storage.get_object(&bucket, &key).await {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Object not found"})),
            )
                .into_response();
        }
    };

    let filename = key.rsplit('/').next().unwrap_or(&key);
    let safe_filename = sanitize_filename(filename);
    let stream = tokio_util::io::ReaderStream::with_capacity(reader, 256 * 1024);
    let body = axum::body::Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", &meta.content_type)
        .header("Content-Length", meta.size.to_string())
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", safe_filename),
        )
        .body(body)
        .unwrap()
        .into_response()
}

/// Sanitize a filename for use in Content-Disposition headers.
/// Removes characters that could enable header injection.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '"' && *c != '\\' && *c != '\r' && *c != '\n')
        .collect()
}

#[derive(serde::Deserialize)]
pub struct PresignParams {
    expires: Option<u64>,
}

pub async fn presign_object(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<PresignParams>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(resp) =
        console_object_check(&state, &session, &bucket, &key, "s3:GetObject").await
    {
        return resp;
    }

    // Verify object exists
    match state.storage.head_object(&bucket, &key).await {
        Ok(_) => {}
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Object not found"})),
            )
                .into_response();
        }
    }

    let expires_secs = params.expires.unwrap_or(3600).min(604800);

    // Determine the host from the request
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost:9000");

    let now = chrono::Utc::now();
    let date_stamp = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let region = &state.config.region;
    let access_key = &state.config.access_key;

    let credential = format!("{}/{}/{}/s3/aws4_request", access_key, date_stamp, region);

    const S3_ENCODE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    let encode =
        |s: &str| -> String { percent_encoding::utf8_percent_encode(s, S3_ENCODE).to_string() };

    // URI-encode each path segment per AWS SigV4 spec. The bucket/key values
    // arrive decoded from Axum's Path extractor, so we must encode them for
    // both the canonical request and the presigned URL.
    let encoded_key: String = key
        .split('/')
        .map(|s| encode(s))
        .collect::<Vec<_>>()
        .join("/");
    let path = format!("/{}/{}", encode(&bucket), encoded_key);

    // Build query string params (sorted alphabetically, excluding Signature)
    let qs_params = [
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
        ("X-Amz-Credential", credential.clone()),
        ("X-Amz-Date", amz_date.clone()),
        ("X-Amz-Expires", expires_secs.to_string()),
        ("X-Amz-SignedHeaders", "host".to_string()),
    ];

    let canonical_qs: String = qs_params
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_headers = format!("host:{}\n", host);
    let canonical_request = format!(
        "GET\n{}\n{}\n{}\nhost\nUNSIGNED-PAYLOAD",
        path, canonical_qs, canonical_headers
    );

    let scope = format!("{}/{}/s3/aws4_request", date_stamp, region);
    let canonical_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date, scope, canonical_hash
    );

    let signing_key =
        signature_v4::derive_signing_key(&state.config.secret_key, &date_stamp, region);

    let mut mac = HmacSha256::new_from_slice(&signing_key).unwrap();
    mac.update(string_to_sign.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    // Determine scheme
    let scheme = if headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "https")
        .unwrap_or(false)
    {
        "https"
    } else {
        "http"
    };

    let presigned_url = format!(
        "{}://{}{}?{}&X-Amz-Signature={}",
        scheme, host, path, canonical_qs, signature
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "url": presigned_url,
            "expiresIn": expires_secs,
        })),
    )
        .into_response()
}

#[derive(serde::Deserialize)]
pub struct CreateFolderRequest {
    name: String,
}

pub async fn create_folder(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Json(body): Json<CreateFolderRequest>,
) -> impl IntoResponse {
    let name = body.name.trim().trim_matches('/');
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Folder name is required"})),
        )
            .into_response();
    }

    let key = format!("{}/", name);
    if let Err(resp) =
        console_object_check(&state, &session, &bucket, &key, "s3:PutObject").await
    {
        return resp;
    }

    match state
        .storage
        .put_object(
            &bucket,
            &key,
            "application/x-directory",
            Box::pin(tokio::io::empty()),
            None,
        )
        .await
    {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_versioning(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_bucket_check(&state, &session, &bucket, "s3:GetBucketVersioning").await
    {
        return resp;
    }

    match state.storage.is_versioned(&bucket).await {
        Ok(enabled) => (
            StatusCode::OK,
            Json(serde_json::json!({"enabled": enabled})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct SetVersioningRequest {
    enabled: bool,
}

pub async fn set_versioning(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Json(body): Json<SetVersioningRequest>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_bucket_check(&state, &session, &bucket, "s3:PutBucketVersioning").await
    {
        return resp;
    }

    match state.storage.set_versioning(&bucket, body.enabled).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}


pub async fn get_public(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_bucket_check(&state, &session, &bucket, "s3:GetBucketPolicy").await
    {
        return resp;
    }

    match state.storage.get_bucket_policy(&bucket).await {
        Ok(policy) => {
            let read = crate::iam::policy::policy_has_public_read(policy.as_deref());
            let list = crate::iam::policy::policy_has_public_list(policy.as_deref());
            (
                StatusCode::OK,
                Json(serde_json::json!({"read": read, "list": list})),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct SetPublicRequest {
    read: bool,
    list: bool,
}

pub async fn set_public(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Json(body): Json<SetPublicRequest>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_bucket_check(&state, &session, &bucket, "s3:PutBucketPolicy").await
    {
        return resp;
    }

    let existing = match state.storage.get_bucket_policy(&bucket).await {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };
    let policy = match crate::iam::policy::merge_public_access_policy(
        &bucket,
        existing.as_deref(),
        body.read,
        body.list,
    ) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e})),
            )
                .into_response();
        }
    };
    match state.storage.put_bucket_policy(&bucket, &policy).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct ListVersionsParams {
    key: String,
}

pub async fn list_versions(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path(bucket): Path<String>,
    Query(params): Query<ListVersionsParams>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_object_check(&state, &session, &bucket, &params.key, "s3:GetObjectVersion").await
    {
        return resp;
    }

    let all = match state
        .storage
        .list_object_versions(&bucket, &params.key)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    // Filter to only versions matching this exact key
    let versions: Vec<serde_json::Value> = all
        .into_iter()
        .filter(|v| v.key == params.key)
        .map(|v| {
            serde_json::json!({
                "versionId": v.version_id,
                "lastModified": v.last_modified,
                "size": v.size,
                "etag": v.etag,
                "isDeleteMarker": v.is_delete_marker,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({"versions": versions})),
    )
        .into_response()
}

pub async fn delete_version(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path((bucket, version_id, key)): Path<(String, String, String)>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_object_check(&state, &session, &bucket, &key, "s3:DeleteObjectVersion").await
    {
        return resp;
    }

    match state
        .storage
        .delete_object_version(&bucket, &key, &version_id)
        .await
    {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn download_version(
    State(state): State<AppState>,
    Extension(session): Extension<ConsoleSession>,
    Path((bucket, version_id, key)): Path<(String, String, String)>,
) -> impl IntoResponse {
    if let Err(resp) =
        console_object_check(&state, &session, &bucket, &key, "s3:GetObjectVersion").await
    {
        return resp;
    }

    let (reader, meta) = match state
        .storage
        .get_object_version(&bucket, &key, &version_id)
        .await
    {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Version not found"})),
            )
                .into_response();
        }
    };

    let filename = key.rsplit('/').next().unwrap_or(&key);
    let safe_filename = sanitize_filename(filename);
    let stream = tokio_util::io::ReaderStream::with_capacity(reader, 256 * 1024);
    let body = axum::body::Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", &meta.content_type)
        .header("Content-Length", meta.size.to_string())
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", safe_filename),
        )
        .body(body)
        .unwrap()
        .into_response()
}

async fn require_root_middleware(
    request: Request,
    next: Next,
) -> Response {
    let authorized = request
        .extensions()
        .get::<ConsoleSession>()
        .map(|s| s.is_root)
        .unwrap_or(false);
    if !authorized {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Admin access required"})),
        )
            .into_response();
    }
    next.run(request).await
}

pub async fn list_users_api(State(state): State<AppState>) -> impl IntoResponse {
    let users: Vec<_> = state
        .user_store
        .list_users()
        .await
        .into_iter()
        .map(|u| {
            serde_json::json!({
                "username": u.username,
                "userId": u.user_id,
                "createdAt": u.created_at,
                "accessKeys": u.access_keys.iter().map(|k| serde_json::json!({
                    "accessKeyId": k.access_key_id,
                    "status": format!("{:?}", k.status),
                    "createdAt": k.created_at,
                })).collect::<Vec<_>>(),
                "attachedPolicies": u.attached_policies,
                "inlinePolicies": u.inline_policies.iter().map(|p| p.policy_name.clone()).collect::<Vec<_>>(),
            })
        })
        .collect();
    (StatusCode::OK, Json(serde_json::json!({ "users": users }))).into_response()
}

pub async fn create_user_key_api(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> impl IntoResponse {
    match state.user_store.create_access_key(&username).await {
        Ok(key) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "accessKeyId": key.access_key_id,
                "secretAccessKey": key.secret_access_key,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

pub async fn delete_user_key_api(
    State(state): State<AppState>,
    Path((username, access_key_id)): Path<(String, String)>,
) -> impl IntoResponse {
    match state
        .user_store
        .delete_access_key(&username, &access_key_id)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct PutUserPolicyRequest {
    document: String,
}

pub async fn put_user_policy_api(
    State(state): State<AppState>,
    Path((username, policy_name)): Path<(String, String)>,
    Json(body): Json<PutUserPolicyRequest>,
) -> impl IntoResponse {
    let doc: crate::iam::types::PolicyDocumentRaw = match serde_json::from_str(&body.document) {
        Ok(d) => d,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid policy document JSON"})),
            )
                .into_response();
        }
    };
    match state
        .user_store
        .put_user_policy(&username, &policy_name, doc)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

pub async fn get_user_policy_api(
    State(state): State<AppState>,
    Path((username, policy_name)): Path<(String, String)>,
) -> impl IntoResponse {
    let user = match state.user_store.get_user(&username).await {
        Some(u) => u,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "User not found"})),
            )
                .into_response();
        }
    };
    let policy = user
        .inline_policies
        .iter()
        .find(|p| p.policy_name == policy_name);
    match policy {
        Some(p) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "policyName": p.policy_name,
                "document": serde_json::to_string(&p.document).unwrap_or_default(),
            })),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Policy not found"})),
        )
            .into_response(),
    }
}

pub async fn delete_user_policy_api(
    State(state): State<AppState>,
    Path((username, policy_name)): Path<(String, String)>,
) -> impl IntoResponse {
    match state
        .user_store
        .delete_user_policy(&username, &policy_name)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachPolicyRequest {
    policy_arn: String,
}

pub async fn attach_user_policy_api(
    State(state): State<AppState>,
    Path(username): Path<String>,
    Json(body): Json<AttachPolicyRequest>,
) -> impl IntoResponse {
    match state
        .user_store
        .attach_user_policy(&username, &body.policy_arn)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

pub async fn detach_user_policy_api(
    State(state): State<AppState>,
    Path(username): Path<String>,
    Json(body): Json<AttachPolicyRequest>,
) -> impl IntoResponse {
    match state
        .user_store
        .detach_user_policy(&username, &body.policy_arn)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

pub async fn list_policies_api(State(state): State<AppState>) -> impl IntoResponse {
    let policies: Vec<_> = state
        .user_store
        .list_managed_policies()
        .await
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "name": p.policy_name,
                "policyId": p.policy_id,
                "arn": p.arn,
            })
        })
        .collect();
    (StatusCode::OK, Json(serde_json::json!({ "policies": policies }))).into_response()
}

#[derive(serde::Deserialize)]
pub struct CreatePolicyApiRequest {
    name: String,
    document: String,
}

pub async fn create_policy_api(
    State(state): State<AppState>,
    Json(body): Json<CreatePolicyApiRequest>,
) -> impl IntoResponse {
    let doc: crate::iam::types::PolicyDocumentRaw = match serde_json::from_str(&body.document) {
        Ok(d) => d,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid policy document JSON"})),
            )
                .into_response();
        }
    };
    match state
        .user_store
        .create_managed_policy(&body.name, doc)
        .await
    {
        Ok(policy) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "name": policy.policy_name,
                "arn": policy.arn,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

pub async fn get_policy_api(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.user_store.get_managed_policy(&name).await {
        Some(p) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "name": p.policy_name,
                "arn": p.arn,
                "document": serde_json::to_string(&p.document).unwrap_or_default(),
            })),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Policy not found"})),
        )
            .into_response(),
    }
}

pub async fn delete_policy_api(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.user_store.delete_managed_policy(&name).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct CreateUserApiRequest {
    username: String,
}

pub async fn create_user_api(
    State(state): State<AppState>,
    Json(body): Json<CreateUserApiRequest>,
) -> impl IntoResponse {
    match state.user_store.create_user(&body.username).await {
        Ok(user) => {
            let key = state
                .user_store
                .create_access_key(&user.username)
                .await
                .ok();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "username": user.username,
                    "userId": user.user_id,
                    "accessKey": key.map(|k| serde_json::json!({
                        "accessKeyId": k.access_key_id,
                        "secretAccessKey": k.secret_access_key,
                    })),
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

pub async fn delete_user_api(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> impl IntoResponse {
    match state.user_store.delete_user(&username).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

pub fn console_router(state: AppState) -> Router<AppState> {
    let json_body_limit = DefaultBodyLimit::max(state.config.max_console_body_bytes);

    let public = Router::new()
        .route("/auth/login", post(login))
        .route("/auth/check", get(check))
        .layer(json_body_limit);

    let admin_routes: Router<AppState> = Router::new()
        .route("/users", get(list_users_api))
        .route("/users", post(create_user_api))
        .route("/users/{username}", delete(delete_user_api))
        .route("/users/{username}/keys", post(create_user_key_api))
        .route(
            "/users/{username}/keys/{access_key_id}",
            delete(delete_user_key_api),
        )
        .route(
            "/users/{username}/policies/{policy_name}",
            get(get_user_policy_api)
                .put(put_user_policy_api)
                .delete(delete_user_policy_api),
        )
        .route("/users/{username}/attach-policy", post(attach_user_policy_api))
        .route("/users/{username}/detach-policy", post(detach_user_policy_api))
        .route("/policies", get(list_policies_api).post(create_policy_api))
        .route(
            "/policies/{name}",
            get(get_policy_api).delete(delete_policy_api),
        )
        .layer(axum::middleware::from_fn(require_root_middleware));

    let protected_limited = Router::new()
        .route("/auth/logout", post(logout))
        .route("/buckets", get(list_buckets))
        .route("/buckets", post(create_bucket))
        .route("/buckets/{bucket}", delete(delete_bucket_api))
        .route("/buckets/{bucket}/folders", post(create_folder))
        .route("/buckets/{bucket}/objects", get(list_objects))
        .route(
            "/buckets/{bucket}/objects/{*key}",
            delete(delete_object_api),
        )
        .route("/buckets/{bucket}/download/{*key}", get(download_object))
        .route("/buckets/{bucket}/presign/{*key}", get(presign_object))
        .route("/buckets/{bucket}/versioning", get(get_versioning))
        .route("/buckets/{bucket}/versioning", put(set_versioning))
        .route("/buckets/{bucket}/public", get(get_public))
        .route("/buckets/{bucket}/public", put(set_public))
        .route("/buckets/{bucket}/versions", get(list_versions))
        .route(
            "/buckets/{bucket}/versions/{version_id}/objects/{*key}",
            delete(delete_version),
        )
        .route(
            "/buckets/{bucket}/versions/{version_id}/download/{*key}",
            get(download_version),
        )
        .merge(admin_routes)
        .layer(json_body_limit);

    let protected_streaming =
        Router::new().route("/buckets/{bucket}/upload/{*key}", put(upload_object));

    let protected = protected_limited
        .merge(protected_streaming)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            console_csrf_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state,
            console_auth_middleware,
        ));

    public.merge(protected)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::storage::blob::BlobStorage;
    use crate::storage::{BucketMeta, ByteStream, MetadataStore, ObjectStorage, PgMetadataStore};
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    use super::*;

    async fn test_storage(
        data_dir: &str,
    ) -> Result<(Arc<dyn Storage>, testcontainers::ContainerAsync<Postgres>), Box<dyn std::error::Error>>
    {
        let postgres = Postgres::default().start().await?;
        let port = postgres.get_host_port_ipv4(5432).await?;
        let database_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
        crate::db::run_migrations(&database_url).await?;
        let pool = crate::db::create_pool(&database_url).await?;
        let meta: Arc<dyn MetadataStore> = Arc::new(PgMetadataStore::new(Arc::new(pool)));
        let blobs = BlobStorage::new(data_dir, false, 10 * 1024 * 1024, 0).await?;
        Ok((Arc::new(ObjectStorage::new(blobs, meta)), postgres))
    }

    async fn create_test_bucket(storage: &dyn Storage, bucket: &str) {
        storage
            .create_bucket(&BucketMeta {
                name: bucket.to_string(),
                created_at: "2026-05-18T00:00:00.000Z".to_string(),
                region: "us-east-1".to_string(),
                versioning: false,
                cors_rules: None,
                owner_id: crate::iam::ROOT_CANONICAL_ID.to_string(),
                owner_display_name: crate::iam::ROOT_DISPLAY_NAME.to_string(),
                acl: Some(crate::iam::Acl::private(
                    crate::iam::ROOT_CANONICAL_ID,
                    crate::iam::ROOT_DISPLAY_NAME,
                )),
                policy: None,
                public_read: false,
                public_list: false,
            })
            .await
            .unwrap();
    }

    fn bytes(data: &'static [u8]) -> ByteStream {
        Box::pin(data)
    }

    #[test]
    fn parent_folder_prefix_ignores_root_files_and_folder_markers() {
        assert_eq!(parent_folder_prefix_for_deleted_object("file.txt"), None);
        assert_eq!(parent_folder_prefix_for_deleted_object("folder/"), None);
        assert_eq!(
            parent_folder_prefix_for_deleted_object("folder/file.txt"),
            Some("folder/".to_string())
        );
        assert_eq!(
            parent_folder_prefix_for_deleted_object("a/b/file.txt"),
            Some("a/b/".to_string())
        );
    }

    #[tokio::test]
    async fn deleting_last_console_file_preserves_parent_folder_marker() {
        let temp = tempfile::tempdir().unwrap();
        let (storage, _pg) = test_storage(temp.path().to_str().unwrap()).await.unwrap();
        create_test_bucket(storage.as_ref(), "bucket").await;

        storage
            .put_object(
                "bucket",
                "folder/file.txt",
                "text/plain",
                bytes(b"hello"),
                None,
            )
            .await
            .unwrap();

        storage
            .delete_object("bucket", "folder/file.txt")
            .await
            .unwrap();
        preserve_empty_parent_folder_after_object_delete(storage.as_ref(), "bucket", "folder/file.txt")
            .await
            .unwrap();

        let objects = crate::storage::list_objects_all(storage.as_ref(), "bucket", "folder/")
            .await
            .unwrap();
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].key, "folder/");
        assert_eq!(objects[0].content_type, "application/x-directory");
    }

    #[tokio::test]
    async fn deleting_folder_marker_does_not_recreate_it() {
        let temp = tempfile::tempdir().unwrap();
        let (storage, _pg) = test_storage(temp.path().to_str().unwrap()).await.unwrap();
        create_test_bucket(storage.as_ref(), "bucket").await;

        storage
            .put_object(
                "bucket",
                "folder/",
                "application/x-directory",
                Box::pin(tokio::io::empty()),
                None,
            )
            .await
            .unwrap();

        storage.delete_object("bucket", "folder/").await.unwrap();
        preserve_empty_parent_folder_after_object_delete(storage.as_ref(), "bucket", "folder/")
            .await
            .unwrap();

        let objects = crate::storage::list_objects_all(storage.as_ref(), "bucket", "folder/")
            .await
            .unwrap();
        assert!(objects.is_empty());
    }
}
