use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::server::AppState;

use super::access::session_capabilities;
use super::session::{
    ConsoleSession, TOKEN_MAX_AGE_SECS, constant_time_eq, cookies_require_https, extract_client_ip,
    extract_cookie, generate_token, make_cookie, resolve_session_username,
};

pub(crate) async fn console_auth_middleware(
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
    let cookie = make_cookie(&token, TOKEN_MAX_AGE_SECS, cookies_require_https(&state));

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

pub async fn auth_config(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "cookiesRequireHttps": cookies_require_https(&state),
    }))
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
    let cookie = make_cookie("", 0, cookies_require_https(&state));
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert("Set-Cookie", cookie.parse().unwrap());
    (
        StatusCode::OK,
        resp_headers,
        Json(serde_json::json!({"ok": true})),
    )
}

pub(crate) async fn console_csrf_middleware(
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
pub(crate) async fn require_root_middleware(request: Request, next: Next) -> Response {
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
