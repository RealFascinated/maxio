#![allow(
    clippy::needless_borrows_for_generic_args,
    clippy::useless_vec,
    clippy::needless_range_loop,
    clippy::manual_repeat_n
)]

use maxio::config::Config;
use maxio::iam::{IamStore, PgIamStore};
use maxio::server::{self, AppState};
use maxio::storage::blob::BlobStorage;
use maxio::storage::{MetadataStore, ObjectStorage, PgMetadataStore, Storage};
use std::sync::Arc;
use tempfile::TempDir;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

const ACCESS_KEY: &str = "maxioadmin";
const SECRET_KEY: &str = "maxioadmin";
const REGION: &str = "us-east-1";

struct ServerHandle {
    url: String,
    _keep_alive: TestKeepAlive,
}

struct TestKeepAlive {
    _dir: TempDir,
    _postgres: testcontainers::ContainerAsync<Postgres>,
}

impl std::ops::Deref for ServerHandle {
    type Target = str;

    fn deref(&self) -> &str {
        &self.url
    }
}

impl std::fmt::Display for ServerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.url)
    }
}

async fn start_postgres() -> (testcontainers::ContainerAsync<Postgres>, String) {
    let postgres = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let database_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    (postgres, database_url)
}

async fn create_storage(data_dir: &str, database_url: &str) -> Arc<dyn Storage> {
    maxio::db::run_migrations(database_url).await.unwrap();
    let pool = maxio::db::create_pool(database_url).await.unwrap();
    let meta: Arc<dyn MetadataStore> = Arc::new(PgMetadataStore::new(Arc::new(pool)));
    let blobs = BlobStorage::new(data_dir).await.unwrap();
    Arc::new(ObjectStorage::new(blobs, meta))
}

fn test_config(data_dir: String, database_url: String, default_buckets: &str) -> Config {
    Config {
        port: 0,
        address: "127.0.0.1".to_string(),
        data_dir,
        database_url,
        access_key: ACCESS_KEY.to_string(),
        secret_key: SECRET_KEY.to_string(),
        allow_insecure_dev: true,
        secure_cookies: false,
        default_buckets: default_buckets.to_string(),
        max_console_body_bytes: 1024 * 1024,
        metrics_token: String::new(),
        cache_dir: None,
        cache_max_size: 10 * 1024 * 1024 * 1024,
        cache_writeback: false,
        cache_flush_interval: 30,
        public_url: None,
        async_meta_write: false,
    }
}

async fn test_app_state(storage: Arc<dyn Storage>, config: Arc<Config>) -> AppState {
    maxio::db::run_migrations(&config.database_url)
        .await
        .unwrap();
    let pool = maxio::db::create_pool(&config.database_url).await.unwrap();
    let pool = Arc::new(pool);
    let user_store: Arc<dyn IamStore> = Arc::new(PgIamStore::new(pool.clone()));
    let metrics = Arc::new(maxio::metrics::MetricsRegistry::new().unwrap());
    let stats = maxio::stats::BucketStatsCache::new(Arc::clone(&pool), Arc::clone(&metrics));
    AppState {
        storage,
        config,
        login_rate_limiter: Arc::new(maxio::api::console::LoginRateLimiter::new()),
        user_store,
        db_pool: pool,
        metrics,
        stats,
        cache: None,
        signing_key_cache: Arc::new(maxio::auth::signing_key_cache::SigningKeyCache::new()),
    }
}

/// Spin up a test server on a random port.
async fn start_server() -> ServerHandle {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().to_str().unwrap().to_string();
    let (postgres, database_url) = start_postgres().await;
    let storage = create_storage(&data_dir, &database_url).await;
    let config = test_config(data_dir.clone(), database_url, "");

    let state = test_app_state(storage, Arc::new(config)).await;

    let app = server::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });

    ServerHandle {
        url: base_url,
        _keep_alive: TestKeepAlive {
            _dir: tmp,
            _postgres: postgres,
        },
    }
}

/// Sign a request with AWS Signature V4.
fn sign_request(method: &str, url: &str, headers: &mut Vec<(String, String)>, body: &[u8]) {
    let parsed = reqwest::Url::parse(url).unwrap();
    let host = parsed.host_str().unwrap();
    let port = parsed.port().unwrap();
    let host_header = format!("{}:{}", host, port);
    let path = parsed.path();
    let query = parsed.query().unwrap_or("");

    let now = chrono::Utc::now();
    let date_stamp = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

    let payload_hash = hex::encode(Sha256::digest(body));

    headers.push(("host".to_string(), host_header.clone()));
    headers.push(("x-amz-date".to_string(), amz_date.clone()));
    headers.push(("x-amz-content-sha256".to_string(), payload_hash.clone()));

    headers.sort_by(|a, b| a.0.cmp(&b.0));

    let signed_headers: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
    let signed_headers_str = signed_headers.join(";");

    let canonical_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{}:{}\n", k, v.trim()))
        .collect();

    let canonical_qs = if query.is_empty() {
        String::new()
    } else {
        let mut pairs: Vec<(String, String)> = query
            .split('&')
            .filter(|s| !s.is_empty())
            .map(|pair| {
                let mut parts = pair.splitn(2, '=');
                let key = parts.next().unwrap_or("").to_string();
                let val = parts.next().unwrap_or("").to_string();
                (key, val)
            })
            .collect();
        pairs.sort();
        pairs
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&")
    };

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method, path, canonical_qs, canonical_headers, signed_headers_str, payload_hash
    );

    let scope = format!("{}/{}/s3/aws4_request", date_stamp, REGION);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date,
        scope,
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );

    let key = format!("AWS4{}", SECRET_KEY);
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).unwrap();
    mac.update(date_stamp.as_bytes());
    let date_key = mac.finalize().into_bytes();

    let mut mac = HmacSha256::new_from_slice(&date_key).unwrap();
    mac.update(REGION.as_bytes());
    let date_region_key = mac.finalize().into_bytes();

    let mut mac = HmacSha256::new_from_slice(&date_region_key).unwrap();
    mac.update(b"s3");
    let date_region_service_key = mac.finalize().into_bytes();

    let mut mac = HmacSha256::new_from_slice(&date_region_service_key).unwrap();
    mac.update(b"aws4_request");
    let signing_key = mac.finalize().into_bytes();

    let mut mac = HmacSha256::new_from_slice(&signing_key).unwrap();
    mac.update(string_to_sign.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        ACCESS_KEY, scope, signed_headers_str, signature
    );
    headers.push(("authorization".to_string(), auth));
}

// ---- Default buckets tests ----

async fn start_server_with_default_buckets(default_buckets: &str) -> ServerHandle {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().to_str().unwrap().to_string();
    let (postgres, database_url) = start_postgres().await;
    let storage = create_storage(&data_dir, &database_url).await;

    maxio::storage::provision_default_buckets(storage.as_ref(), default_buckets).await;

    let config = test_config(data_dir.clone(), database_url, default_buckets);

    let state = test_app_state(storage, Arc::new(config)).await;

    let app = server::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });

    ServerHandle {
        url: base_url,
        _keep_alive: TestKeepAlive {
            _dir: tmp,
            _postgres: postgres,
        },
    }
}

#[tokio::test]
async fn test_default_buckets_created_on_boot() {
    let base_url = start_server_with_default_buckets("alpha,beta,gamma").await;

    // All buckets should exist
    let resp = s3_request("HEAD", &format!("{}/alpha", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let resp = s3_request("HEAD", &format!("{}/beta", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let resp = s3_request("HEAD", &format!("{}/gamma", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    // List should include default buckets
    let resp = s3_request("GET", &format!("{}/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Name>alpha</Name>"));
    assert!(body.contains("<Name>beta</Name>"));
    assert!(body.contains("<Name>gamma</Name>"));
}

#[tokio::test]
async fn test_default_buckets_skip_existing() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().to_str().unwrap().to_string();
    let (postgres, database_url) = start_postgres().await;
    let storage = create_storage(&data_dir, &database_url).await;

    // First provision: creates the bucket
    maxio::storage::provision_default_buckets(storage.as_ref(), "existing").await;
    // Second provision: must be idempotent — no error, no duplicate
    maxio::storage::provision_default_buckets(storage.as_ref(), "existing").await;

    let config = test_config(data_dir.clone(), database_url, "");
    let state = test_app_state(storage, Arc::new(config)).await;
    let _postgres = postgres;
    let app = server::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let resp = s3_request("HEAD", &format!("{}/existing", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    let resp = s3_request("GET", &format!("{}/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Name>existing</Name>"));
}

#[tokio::test]
async fn test_default_buckets_skips_invalid_names() {
    let base_url =
        start_server_with_default_buckets("INVALID,valid,b..a,a.-b,a-.b,192.168.0.1").await;

    for bucket in ["INVALID", "b..a", "a.-b", "a-.b", "192.168.0.1"] {
        let resp = s3_request("HEAD", &format!("{}/{}", base_url, bucket), vec![]).await;
        assert_eq!(resp.status(), 404, "{bucket} should be skipped");
    }

    let resp = s3_request("HEAD", &format!("{}/valid", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_empty_default_buckets() {
    let base_url = start_server_with_default_buckets("").await;

    // No default buckets should exist
    let resp = s3_request("GET", &format!("{}/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(!body.contains("<Name>"), "No buckets should be listed");
}

#[tokio::test]
async fn test_default_buckets_single() {
    let base_url = start_server_with_default_buckets("only-one").await;

    let resp = s3_request("HEAD", &format!("{}/only-one", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

/// Sign a request using comma-only separators (no spaces), like mc does.
fn sign_request_compact(method: &str, url: &str, headers: &mut Vec<(String, String)>, body: &[u8]) {
    // Reuse the same signing logic but produce compact auth header
    let parsed = reqwest::Url::parse(url).unwrap();
    let host = parsed.host_str().unwrap();
    let port = parsed.port().unwrap();
    let host_header = format!("{}:{}", host, port);
    let path = parsed.path();
    let query = parsed.query().unwrap_or("");

    let now = chrono::Utc::now();
    let date_stamp = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

    let payload_hash = hex::encode(Sha256::digest(body));

    headers.push(("host".to_string(), host_header.clone()));
    headers.push(("x-amz-date".to_string(), amz_date.clone()));
    headers.push(("x-amz-content-sha256".to_string(), payload_hash.clone()));

    headers.sort_by(|a, b| a.0.cmp(&b.0));

    let signed_headers: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
    let signed_headers_str = signed_headers.join(";");

    let canonical_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{}:{}\n", k, v.trim()))
        .collect();

    let canonical_qs = if query.is_empty() {
        String::new()
    } else {
        let mut pairs: Vec<(String, String)> = query
            .split('&')
            .filter(|s| !s.is_empty())
            .map(|pair| {
                let mut parts = pair.splitn(2, '=');
                let key = parts.next().unwrap_or("").to_string();
                let val = parts.next().unwrap_or("").to_string();
                (key, val)
            })
            .collect();
        pairs.sort();
        pairs
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&")
    };

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method, path, canonical_qs, canonical_headers, signed_headers_str, payload_hash
    );

    let scope = format!("{}/{}/s3/aws4_request", date_stamp, REGION);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date,
        scope,
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );

    let key = format!("AWS4{}", SECRET_KEY);
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).unwrap();
    mac.update(date_stamp.as_bytes());
    let date_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_key).unwrap();
    mac.update(REGION.as_bytes());
    let date_region_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_region_key).unwrap();
    mac.update(b"s3");
    let date_region_service_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_region_service_key).unwrap();
    mac.update(b"aws4_request");
    let signing_key = mac.finalize().into_bytes();

    let mut mac = HmacSha256::new_from_slice(&signing_key).unwrap();
    mac.update(string_to_sign.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    // Compact format: no spaces after commas (like mc sends)
    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={}/{},SignedHeaders={},Signature={}",
        ACCESS_KEY, scope, signed_headers_str, signature
    );
    headers.push(("authorization".to_string(), auth));
}

/// Build a signed request and send it.
async fn s3_request(method: &str, url: &str, body: Vec<u8>) -> reqwest::Response {
    let mut headers = Vec::new();
    sign_request(method, url, &mut headers, &body);

    let client = client();
    let mut builder = match method {
        "GET" => client.get(url),
        "PUT" => client.put(url),
        "HEAD" => client.head(url),
        "DELETE" => client.delete(url),
        "POST" => client.post(url),
        _ => panic!("unsupported method"),
    };

    for (k, v) in &headers {
        builder = builder.header(k.as_str(), v.as_str());
    }

    if !body.is_empty() {
        builder = builder.body(body);
    }

    builder.send().await.unwrap()
}

/// Sign and send a request with extra headers (e.g. x-amz-copy-source).
async fn s3_request_with_headers(
    method: &str,
    url: &str,
    body: Vec<u8>,
    extra_headers: Vec<(&str, &str)>,
) -> reqwest::Response {
    let mut headers: Vec<(String, String)> = extra_headers
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    sign_request(method, url, &mut headers, &body);

    let client = client();
    let mut builder = match method {
        "GET" => client.get(url),
        "PUT" => client.put(url),
        "HEAD" => client.head(url),
        "DELETE" => client.delete(url),
        "POST" => client.post(url),
        _ => panic!("unsupported method"),
    };

    for (k, v) in &headers {
        builder = builder.header(k.as_str(), v.as_str());
    }

    if !body.is_empty() {
        builder = builder.body(body);
    }

    builder.send().await.unwrap()
}

/// Build a signed request with compact auth header (no spaces after commas).
async fn s3_request_compact(method: &str, url: &str, body: Vec<u8>) -> reqwest::Response {
    let mut headers = Vec::new();
    sign_request_compact(method, url, &mut headers, &body);

    let client = client();
    let mut builder = match method {
        "GET" => client.get(url),
        "PUT" => client.put(url),
        "HEAD" => client.head(url),
        "DELETE" => client.delete(url),
        "POST" => client.post(url),
        _ => panic!("unsupported method"),
    };

    for (k, v) in &headers {
        builder = builder.header(k.as_str(), v.as_str());
    }

    if !body.is_empty() {
        builder = builder.body(body);
    }

    builder.send().await.unwrap()
}

/// Build a PUT request with STREAMING-AWS4-HMAC-SHA256-PAYLOAD (AWS chunked encoding).
async fn s3_put_chunked(url: &str, data: &[u8]) -> reqwest::Response {
    let parsed = reqwest::Url::parse(url).unwrap();
    let host = parsed.host_str().unwrap();
    let port = parsed.port().unwrap();
    let host_header = format!("{}:{}", host, port);
    let path = parsed.path();
    let query = parsed.query().unwrap_or("");

    let now = chrono::Utc::now();
    let date_stamp = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

    // For streaming, the payload hash is the literal string
    let payload_hash = "STREAMING-AWS4-HMAC-SHA256-PAYLOAD";

    let mut sign_headers = vec![
        ("host".to_string(), host_header.clone()),
        ("x-amz-content-sha256".to_string(), payload_hash.to_string()),
        ("x-amz-date".to_string(), amz_date.clone()),
        (
            "x-amz-decoded-content-length".to_string(),
            data.len().to_string(),
        ),
    ];
    sign_headers.sort_by(|a, b| a.0.cmp(&b.0));

    let signed_headers: Vec<&str> = sign_headers.iter().map(|(k, _)| k.as_str()).collect();
    let signed_headers_str = signed_headers.join(";");

    let canonical_headers: String = sign_headers
        .iter()
        .map(|(k, v)| format!("{}:{}\n", k, v.trim()))
        .collect();

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        "PUT", path, query, canonical_headers, signed_headers_str, payload_hash
    );

    let scope = format!("{}/{}/s3/aws4_request", date_stamp, REGION);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date,
        scope,
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );

    let key = format!("AWS4{}", SECRET_KEY);
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).unwrap();
    mac.update(date_stamp.as_bytes());
    let date_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_key).unwrap();
    mac.update(REGION.as_bytes());
    let date_region_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_region_key).unwrap();
    mac.update(b"s3");
    let date_region_service_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_region_service_key).unwrap();
    mac.update(b"aws4_request");
    let signing_key = mac.finalize().into_bytes();

    let mut mac = HmacSha256::new_from_slice(&signing_key).unwrap();
    mac.update(string_to_sign.as_bytes());
    let seed_signature = hex::encode(mac.finalize().into_bytes());

    // Compact auth header (no spaces)
    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={}/{},SignedHeaders={},Signature={}",
        ACCESS_KEY, scope, signed_headers_str, seed_signature
    );

    // Build AWS chunked body: "<hex_size>;chunk-signature=<sig>\r\n<data>\r\n0;chunk-signature=<sig>\r\n"
    // For simplicity, compute chunk signatures with a dummy (real mc would chain them)
    let chunk_sig = "0".repeat(64); // placeholder — server doesn't verify chunk sigs
    let mut chunked_body = Vec::new();
    chunked_body.extend_from_slice(
        format!("{:x};chunk-signature={}\r\n", data.len(), chunk_sig).as_bytes(),
    );
    chunked_body.extend_from_slice(data);
    chunked_body.extend_from_slice(b"\r\n");
    chunked_body.extend_from_slice(format!("0;chunk-signature={}\r\n", chunk_sig).as_bytes());

    client()
        .put(url)
        .header("host", &host_header)
        .header("x-amz-date", &amz_date)
        .header("x-amz-content-sha256", payload_hash)
        .header("x-amz-decoded-content-length", data.len().to_string())
        .header("authorization", &auth)
        .header("content-type", "application/octet-stream")
        .body(chunked_body)
        .send()
        .await
        .unwrap()
}

fn extract_xml_tag(body: &str, tag: &str) -> Option<String> {
    let start = format!("<{}>", tag);
    let end = format!("</{}>", tag);
    let from = body.find(&start)? + start.len();
    let to = body[from..].find(&end)? + from;
    Some(body[from..to].to_string())
}

// ---- Tests ----

#[tokio::test]
async fn test_healthz_is_public_and_returns_ok() {
    let base_url = start_server().await;
    let resp = client()
        .get(format!("{}/healthz", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_auth_config_is_public() {
    let base_url = start_server().await;
    let resp = client()
        .get(format!("{}/api/auth/config", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["cookiesRequireHttps"], false);
}

#[tokio::test]
async fn test_readyz_is_public_and_returns_ok() {
    let base_url = start_server().await;
    let resp = client()
        .get(format!("{}/readyz", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_security_headers_are_applied() {
    let base_url = start_server().await;
    let resp = client()
        .get(format!("{}/healthz", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert!(resp.headers().contains_key("content-security-policy"));
    assert_eq!(resp.headers().get("x-frame-options").unwrap(), "DENY");
}

#[tokio::test]
async fn test_ui_deep_link_uses_spa_fallback() {
    let base_url = start_server().await;
    let resp = client()
        .get(format!("{}/ui/buckets/example/settings", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("text/html")
    );
    assert_eq!(
        resp.headers().get("cache-control").unwrap(),
        "no-store, must-revalidate"
    );
    let body = resp.text().await.unwrap();
    assert!(body.contains("MaxIO"));
}

#[tokio::test]
async fn test_auth_rejects_bad_key() {
    let base_url = start_server().await;

    // Request with no auth header
    let resp = client().get(&*base_url).send().await.unwrap();
    assert_eq!(resp.status(), 403);

    // Request with garbage auth
    let resp = client()
        .get(&*base_url)
        .header("authorization", "garbage")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn test_auth_accepts_valid_signature() {
    let base_url = start_server().await;
    let resp = s3_request("GET", &format!("{}/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_create_bucket() {
    let base_url = start_server().await;

    // Create bucket
    let resp = s3_request("PUT", &format!("{}/test-bucket", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    // Head bucket should succeed
    let resp = s3_request("HEAD", &format!("{}/test-bucket", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_create_bucket_rejects_canonical_invalid_names() {
    let base_url = start_server().await;

    for bucket in ["a.-b", "a-.b", "192.168.0.1"] {
        let resp = s3_request("PUT", &format!("{}/{}", base_url, bucket), vec![]).await;
        assert_eq!(resp.status(), 400, "{bucket} should be rejected");
        let body = resp.text().await.unwrap();
        assert!(
            body.contains("<Code>InvalidBucketName</Code>"),
            "{bucket} should return InvalidBucketName, got {body}"
        );
    }
}

#[tokio::test]
async fn test_create_bucket_duplicate() {
    let base_url = start_server().await;

    let resp = s3_request("PUT", &format!("{}/test-bucket", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    // Creating same bucket again should fail
    let resp = s3_request("PUT", &format!("{}/test-bucket", base_url), vec![]).await;
    assert_eq!(resp.status(), 409);
}

#[tokio::test]
async fn test_head_bucket_not_found() {
    let base_url = start_server().await;

    let resp = s3_request("HEAD", &format!("{}/nonexistent", base_url), vec![]).await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_list_buckets() {
    let base_url = start_server().await;

    // Create two buckets
    s3_request("PUT", &format!("{}/alpha", base_url), vec![]).await;
    s3_request("PUT", &format!("{}/beta", base_url), vec![]).await;

    // List
    let resp = s3_request("GET", &format!("{}/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Name>alpha</Name>"));
    assert!(body.contains("<Name>beta</Name>"));
}

#[tokio::test]
async fn test_delete_bucket() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/to-delete", base_url), vec![]).await;

    let resp = s3_request("DELETE", &format!("{}/to-delete", base_url), vec![]).await;
    assert_eq!(resp.status(), 204);

    // Should be gone
    let resp = s3_request("HEAD", &format!("{}/to-delete", base_url), vec![]).await;
    assert_eq!(resp.status(), 404);
}

// Regression: delete_bucket must succeed after full object lifecycle
// (put + delete) even when metadata sidecars or empty dirs remain.
#[tokio::test]
async fn test_delete_bucket_after_object_lifecycle() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/bucket-one", base_url), vec![]).await;
    let r = s3_request(
        "PUT",
        &format!("{}/bucket-one/f.txt", base_url),
        b"x".to_vec(),
    )
    .await;
    assert_eq!(r.status(), 200);
    let r = s3_request("DELETE", &format!("{}/bucket-one/f.txt", base_url), vec![]).await;
    assert_eq!(r.status(), 204);

    let r = s3_request("DELETE", &format!("{}/bucket-one", base_url), vec![]).await;
    assert_eq!(
        r.status(),
        204,
        "bucket delete should succeed after object removed"
    );

    let r = s3_request("HEAD", &format!("{}/bucket-one", base_url), vec![]).await;
    assert_eq!(r.status(), 404);
}

// Regression: nested keys leave deep directory trees; delete_bucket must
// sweep empty parents.
#[tokio::test]
async fn test_delete_bucket_with_nested_path() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/bucket-two", base_url), vec![]).await;
    let r = s3_request(
        "PUT",
        &format!("{}/bucket-two/a/b/c/d.txt", base_url),
        b"y".to_vec(),
    )
    .await;
    assert_eq!(r.status(), 200);
    let r = s3_request(
        "DELETE",
        &format!("{}/bucket-two/a/b/c/d.txt", base_url),
        vec![],
    )
    .await;
    assert_eq!(r.status(), 204);

    let r = s3_request("DELETE", &format!("{}/bucket-two", base_url), vec![]).await;
    assert_eq!(
        r.status(),
        204,
        "bucket delete should sweep empty nested dirs"
    );
}

// Ensure we did not weaken the real emptiness check.
#[tokio::test]
async fn test_delete_bucket_rejects_real_object() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/bucket-three", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/bucket-three/stay.txt", base_url),
        b"z".to_vec(),
    )
    .await;

    let r = s3_request("DELETE", &format!("{}/bucket-three", base_url), vec![]).await;
    assert_eq!(r.status(), 409);

    // Bucket still exists.
    let r = s3_request("HEAD", &format!("{}/bucket-three", base_url), vec![]).await;
    assert_eq!(r.status(), 200);
}

// Regression: stale nested `.versions/` dir (from past versioning state)
// must not block bucket deletion. Exercised directly against the storage
// layer so the test does not depend on the S3 versioning API.
#[tokio::test]
async fn test_delete_bucket_sweeps_nested_versions() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().to_str().unwrap().to_string();
    let (postgres, database_url) = start_postgres().await;
    let storage = create_storage(&data_dir, &database_url).await;

    storage
        .create_bucket(&maxio::storage::BucketMeta {
            name: "leftover".to_string(),
            created_at: "2026-04-16T00:00:00.000Z".to_string(),
            versioning: false,
            cors_rules: None,
            owner_id: maxio::iam::ROOT_CANONICAL_ID.to_string(),
            owner_display_name: maxio::iam::ROOT_DISPLAY_NAME.to_string(),
            acl: Some(maxio::iam::Acl::private(
                maxio::iam::ROOT_CANONICAL_ID,
                maxio::iam::ROOT_DISPLAY_NAME,
            )),
            policy: None,
            public_read: false,
            public_list: false,
        })
        .await
        .unwrap();

    // Orphan on-disk artifacts must not block metadata-only bucket deletion.
    let bucket_root = tmp.path().join("buckets").join("leftover");
    let stale_versions = bucket_root.join("photos").join(".versions");
    tokio::fs::create_dir_all(&stale_versions).await.unwrap();
    tokio::fs::write(bucket_root.join("orphan.txt"), b"orphan bytes")
        .await
        .unwrap();

    let deleted = storage.delete_bucket("leftover").await.unwrap();
    assert!(
        deleted,
        "delete_bucket should succeed when metadata has no objects"
    );
    assert!(!storage.head_bucket("leftover").await.unwrap());
    let _postgres = postgres;
}

#[tokio::test]
async fn test_put_and_get_object() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;

    let data = b"hello maxio".to_vec();
    let resp = s3_request(
        "PUT",
        &format!("{}/mybucket/test.txt", base_url),
        data.clone(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    assert!(resp.headers().contains_key("etag"));

    // Get it back
    let resp = s3_request("GET", &format!("{}/mybucket/test.txt", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), b"hello maxio");
}

#[tokio::test]
async fn test_head_object() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/file.txt", base_url),
        b"data".to_vec(),
    )
    .await;

    let resp = s3_request("HEAD", &format!("{}/mybucket/file.txt", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers().get("content-length").unwrap(), "4");
}

#[tokio::test]
async fn test_delete_object() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/file.txt", base_url),
        b"data".to_vec(),
    )
    .await;

    let resp = s3_request("DELETE", &format!("{}/mybucket/file.txt", base_url), vec![]).await;
    assert_eq!(resp.status(), 204);

    // Should be gone
    let resp = s3_request("GET", &format!("{}/mybucket/file.txt", base_url), vec![]).await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_delete_object_missing_bucket_returns_404() {
    let base_url = start_server().await;

    let resp = s3_request(
        "DELETE",
        &format!("{}/missing-bucket/file.txt", base_url),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 404);
    let body = resp.text().await.unwrap();
    assert!(body.contains("NoSuchBucket"), "body: {}", body);
}

#[tokio::test]
async fn test_list_objects() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/a.txt", base_url),
        b"aaa".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/b.txt", base_url),
        b"bbb".to_vec(),
    )
    .await;

    let resp = s3_request("GET", &format!("{}/mybucket?list-type=2", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Key>a.txt</Key>"));
    assert!(body.contains("<Key>b.txt</Key>"));
    assert!(body.contains("<KeyCount>2</KeyCount>"));
}

#[tokio::test]
async fn test_list_objects_v2_empty_delimiter() {
    // mcli rm -r sends delimiter= (empty) for flat recursive listing.
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/nested/a.txt", base_url),
        b"aaa".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/b.txt", base_url),
        b"bbb".to_vec(),
    )
    .await;

    let resp = s3_request(
        "GET",
        &format!(
            "{}/mybucket?list-type=2&delimiter=&encoding-type=url&fetch-owner=true&prefix=",
            base_url
        ),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Key>nested/a.txt</Key>"), "body: {}", body);
    assert!(body.contains("<Key>b.txt</Key>"), "body: {}", body);
    assert!(body.contains("<KeyCount>2</KeyCount>"), "body: {}", body);
    assert!(
        !body.contains("<CommonPrefixes>"),
        "empty delimiter must not produce common prefixes: {}",
        body
    );
}

// ---- New tests for findings ----

#[tokio::test]
async fn test_auth_compact_header_no_spaces() {
    // mc sends Authorization header with commas but no spaces:
    // Credential=...,SignedHeaders=...,Signature=...
    let base_url = start_server().await;

    let resp = s3_request_compact("GET", &format!("{}/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    // Also test PUT bucket with compact header
    let resp = s3_request_compact("PUT", &format!("{}/compact-bucket", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_last_modified_http_date_format() {
    // Last-Modified header must be RFC 7231 format: "Tue, 17 Feb 2026 22:17:45 GMT"
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/file.txt", base_url),
        b"data".to_vec(),
    )
    .await;

    // HEAD should return RFC 7231 Last-Modified
    let resp = s3_request("HEAD", &format!("{}/mybucket/file.txt", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let last_modified = resp
        .headers()
        .get("last-modified")
        .unwrap()
        .to_str()
        .unwrap();
    // Should match pattern like "Mon, 17 Feb 2026 22:17:45 GMT"
    assert!(
        last_modified.ends_with(" GMT"),
        "Last-Modified should end with GMT: {}",
        last_modified
    );
    assert!(
        last_modified.contains(", "),
        "Last-Modified should contain comma-space: {}",
        last_modified
    );
    // Must NOT be ISO 8601 (no "T" between date and time digits)
    assert!(
        !last_modified.contains("T0"),
        "Last-Modified must not be ISO 8601: {}",
        last_modified
    );
    assert!(
        !last_modified.contains("T1"),
        "Last-Modified must not be ISO 8601: {}",
        last_modified
    );
    assert!(
        !last_modified.contains("T2"),
        "Last-Modified must not be ISO 8601: {}",
        last_modified
    );

    // GET should also return RFC 7231 Last-Modified
    let resp = s3_request("GET", &format!("{}/mybucket/file.txt", base_url), vec![]).await;
    let last_modified = resp
        .headers()
        .get("last-modified")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(last_modified.ends_with(" GMT"));
    // Verify it parses as HTTP date (day-of-week, DD Mon YYYY HH:MM:SS GMT)
    assert!(
        last_modified.len() > 25,
        "Last-Modified should be full HTTP date: {}",
        last_modified
    );
}

#[tokio::test]
async fn test_put_object_aws_chunked_encoding() {
    // mc sends uploads with x-amz-content-sha256: STREAMING-AWS4-HMAC-SHA256-PAYLOAD
    // and the body is in AWS chunked format
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;

    let data = b"hello chunked world";
    let resp = s3_put_chunked(&format!("{}/mybucket/chunked.txt", base_url), data).await;
    assert_eq!(resp.status(), 200);
    assert!(resp.headers().contains_key("etag"));

    // Verify the stored content is decoded (no chunk framing)
    let resp = s3_request("GET", &format!("{}/mybucket/chunked.txt", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.bytes().await.unwrap();
    assert_eq!(
        body.as_ref(),
        data,
        "Chunked upload content should be decoded"
    );
}

#[tokio::test]
async fn test_put_object_response_headers() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;

    // PUT should return ETag
    let resp = s3_request(
        "PUT",
        &format!("{}/mybucket/file.txt", base_url),
        b"test data".to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let etag = resp.headers().get("etag").unwrap().to_str().unwrap();
    assert!(
        etag.starts_with('"') && etag.ends_with('"'),
        "ETag should be quoted: {}",
        etag
    );

    // HEAD should return Content-Type, Content-Length, ETag, Last-Modified
    let resp = s3_request("HEAD", &format!("{}/mybucket/file.txt", base_url), vec![]).await;
    assert!(resp.headers().contains_key("content-type"));
    assert!(resp.headers().contains_key("content-length"));
    assert!(resp.headers().contains_key("etag"));
    assert!(resp.headers().contains_key("last-modified"));
    assert_eq!(resp.headers().get("content-length").unwrap(), "9");

    // GET should also have these headers
    let resp = s3_request("GET", &format!("{}/mybucket/file.txt", base_url), vec![]).await;
    assert!(resp.headers().contains_key("content-type"));
    assert!(resp.headers().contains_key("content-length"));
    assert!(resp.headers().contains_key("etag"));
    assert!(resp.headers().contains_key("last-modified"));
}

#[tokio::test]
async fn test_delete_objects_batch() {
    // mc uses POST /{bucket}?delete to delete objects (DeleteObjects API)
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/a.txt", base_url),
        b"aaa".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/b.txt", base_url),
        b"bbb".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/c.txt", base_url),
        b"ccc".to_vec(),
    )
    .await;

    // Batch delete a.txt and b.txt
    let delete_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Delete>
  <Object><Key>a.txt</Key></Object>
  <Object><Key>b.txt</Key></Object>
</Delete>"#;

    let resp = s3_request(
        "POST",
        &format!("{}/mybucket?delete", base_url),
        delete_xml.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("<Deleted>"),
        "Response should contain Deleted elements"
    );
    assert!(body.contains("<Key>a.txt</Key>"));
    assert!(body.contains("<Key>b.txt</Key>"));

    // Verify a.txt and b.txt are gone
    let resp = s3_request("GET", &format!("{}/mybucket/a.txt", base_url), vec![]).await;
    assert_eq!(resp.status(), 404);
    let resp = s3_request("GET", &format!("{}/mybucket/b.txt", base_url), vec![]).await;
    assert_eq!(resp.status(), 404);

    // c.txt should still exist
    let resp = s3_request("GET", &format!("{}/mybucket/c.txt", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_delete_objects_batch_missing_bucket_returns_404() {
    let base_url = start_server().await;

    let delete_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Delete>
  <Object><Key>a.txt</Key></Object>
</Delete>"#;

    let resp = s3_request(
        "POST",
        &format!("{}/missing-bucket?delete", base_url),
        delete_xml.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 404);
    let body = resp.text().await.unwrap();
    assert!(body.contains("NoSuchBucket"), "body: {}", body);
}

#[tokio::test]
async fn test_delete_objects_batch_with_version_id() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/ver-bucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/ver-bucket?versioning", base_url),
        br#"<?xml version="1.0" encoding="UTF-8"?>
<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Status>Enabled</Status>
</VersioningConfiguration>"#
            .to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/ver-bucket/obj.txt", base_url),
        b"v1".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/ver-bucket/obj.txt", base_url),
        b"v2".to_vec(),
    )
    .await;

    let list_resp = s3_request("GET", &format!("{}/ver-bucket?versions", base_url), vec![]).await;
    let list_body = list_resp.text().await.unwrap();
    let old_vid = list_body
        .split("<VersionId>")
        .nth(2)
        .and_then(|s| s.split("</VersionId>").next())
        .expect("second version id");
    assert_ne!(old_vid, "null");

    let delete_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Delete>
  <Object><Key>obj.txt</Key><VersionId>{old_vid}</VersionId></Object>
</Delete>"#
    );
    let resp = s3_request(
        "POST",
        &format!("{}/ver-bucket?delete", base_url),
        delete_xml.into_bytes(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Deleted>"), "body: {}", body);
    assert!(
        !body.contains(old_vid),
        "deleted version should not appear as error: {}",
        body
    );

    let list_resp = s3_request("GET", &format!("{}/ver-bucket?versions", base_url), vec![]).await;
    let list_body = list_resp.text().await.unwrap();
    assert!(
        !list_body.contains(&format!("<VersionId>{old_vid}</VersionId>")),
        "old version should be gone: {}",
        list_body
    );
}

#[tokio::test]
async fn test_list_object_versions_pagination() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/page-bucket", base_url), vec![]).await;
    for key in ["a.txt", "b.txt", "c.txt"] {
        s3_request(
            "PUT",
            &format!("{}/page-bucket/{key}", base_url),
            b"x".to_vec(),
        )
        .await;
    }

    let resp = s3_request(
        "GET",
        &format!("{}/page-bucket?versions&max-keys=2", base_url),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<IsTruncated>true</IsTruncated>"), "{}", body);
    assert!(body.contains("<NextKeyMarker>"), "{}", body);

    let marker = body
        .split("<NextKeyMarker>")
        .nth(1)
        .and_then(|s| s.split("</NextKeyMarker>").next())
        .expect("next key marker");
    let resp = s3_request(
        "GET",
        &format!(
            "{}/page-bucket?versions&max-keys=2&key-marker={marker}",
            base_url
        ),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("<IsTruncated>false</IsTruncated>"),
        "{}",
        body
    );
}

#[tokio::test]
async fn test_trailing_slash_bucket_routes() {
    // mc sends PUT /bucket/ (with trailing slash)
    let base_url = start_server().await;

    // Create with trailing slash
    let resp = s3_request("PUT", &format!("{}/mybucket/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    // HEAD with trailing slash
    let resp = s3_request("HEAD", &format!("{}/mybucket/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    // GET (list) with trailing slash
    let resp = s3_request(
        "GET",
        &format!("{}/mybucket/?list-type=2", base_url),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);

    // DELETE with trailing slash
    let resp = s3_request("DELETE", &format!("{}/mybucket/", base_url), vec![]).await;
    assert_eq!(resp.status(), 204);
}

#[tokio::test]
async fn test_chunked_upload_interrupted_then_retry() {
    // Simulate: send a truncated/incomplete chunked upload, then retry with a valid one.
    // The server should not leave corrupt data from the partial upload, and the retry
    // should succeed with correct content.
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;

    let url = format!("{}/mybucket/interrupted.txt", base_url);

    // Build a truncated chunked body: valid first chunk header but missing data/terminator.
    // This simulates a client that starts uploading and then drops the connection.
    let parsed = reqwest::Url::parse(&url).unwrap();
    let host = parsed.host_str().unwrap();
    let port = parsed.port().unwrap();
    let host_header = format!("{}:{}", host, port);
    let path = parsed.path();

    let now = chrono::Utc::now();
    let date_stamp = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let payload_hash = "STREAMING-AWS4-HMAC-SHA256-PAYLOAD";

    let mut sign_headers = vec![
        ("host".to_string(), host_header.clone()),
        ("x-amz-content-sha256".to_string(), payload_hash.to_string()),
        ("x-amz-date".to_string(), amz_date.clone()),
        (
            "x-amz-decoded-content-length".to_string(),
            "1000".to_string(),
        ),
    ];
    sign_headers.sort_by(|a, b| a.0.cmp(&b.0));

    let signed_headers: Vec<&str> = sign_headers.iter().map(|(k, _)| k.as_str()).collect();
    let signed_headers_str = signed_headers.join(";");
    let canonical_headers: String = sign_headers
        .iter()
        .map(|(k, v)| format!("{}:{}\n", k, v.trim()))
        .collect();

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        "PUT", path, "", canonical_headers, signed_headers_str, payload_hash
    );
    let scope = format!("{}/{}/s3/aws4_request", date_stamp, REGION);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date,
        scope,
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );
    let key = format!("AWS4{}", SECRET_KEY);
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).unwrap();
    mac.update(date_stamp.as_bytes());
    let date_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_key).unwrap();
    mac.update(REGION.as_bytes());
    let date_region_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_region_key).unwrap();
    mac.update(b"s3");
    let date_region_service_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_region_service_key).unwrap();
    mac.update(b"aws4_request");
    let signing_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&signing_key).unwrap();
    mac.update(string_to_sign.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={}/{},SignedHeaders={},Signature={}",
        ACCESS_KEY, scope, signed_headers_str, signature
    );

    // Send a truncated chunked body: claims 1000 bytes but only sends a partial chunk
    let chunk_sig = "0".repeat(64);
    let truncated_body = format!("3e8;chunk-signature={}\r\npartial data only", chunk_sig);

    // This request should fail (connection reset / error) since we promised 1000 bytes
    // but sent far fewer. We don't care about the exact error, just that it doesn't
    // leave the server in a broken state.
    let _ = client()
        .put(&url)
        .header("host", &host_header)
        .header("x-amz-date", &amz_date)
        .header("x-amz-content-sha256", payload_hash)
        .header("x-amz-decoded-content-length", "1000")
        .header("authorization", &auth)
        .header("content-type", "application/octet-stream")
        .body(truncated_body.into_bytes())
        .send()
        .await;

    // Small delay to let server finish processing
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Now do a proper chunked upload to the same key — this MUST succeed
    let good_data = b"hello after interrupted upload";
    let resp = s3_put_chunked(&url, good_data).await;
    assert_eq!(
        resp.status(),
        200,
        "Retry upload after interrupted should succeed"
    );

    // Verify content is from the successful retry, not the partial upload
    let resp = s3_request("GET", &url, vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.bytes().await.unwrap();
    assert_eq!(
        body.as_ref(),
        good_data,
        "Content should be from the retry, not the interrupted upload"
    );
}

#[tokio::test]
async fn test_chunked_upload_multi_chunk() {
    // Test chunked upload with multiple chunks (not just one chunk + terminator)
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;

    let url = format!("{}/mybucket/multichunk.txt", base_url);
    let parsed = reqwest::Url::parse(&url).unwrap();
    let host = parsed.host_str().unwrap();
    let port = parsed.port().unwrap();
    let host_header = format!("{}:{}", host, port);
    let path = parsed.path();

    let chunk1 = b"first chunk data ";
    let chunk2 = b"second chunk data ";
    let chunk3 = b"third chunk data";
    let total_len = chunk1.len() + chunk2.len() + chunk3.len();

    let now = chrono::Utc::now();
    let date_stamp = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let payload_hash = "STREAMING-AWS4-HMAC-SHA256-PAYLOAD";

    let mut sign_headers = vec![
        ("host".to_string(), host_header.clone()),
        ("x-amz-content-sha256".to_string(), payload_hash.to_string()),
        ("x-amz-date".to_string(), amz_date.clone()),
        (
            "x-amz-decoded-content-length".to_string(),
            total_len.to_string(),
        ),
    ];
    sign_headers.sort_by(|a, b| a.0.cmp(&b.0));

    let signed_headers: Vec<&str> = sign_headers.iter().map(|(k, _)| k.as_str()).collect();
    let signed_headers_str = signed_headers.join(";");
    let canonical_headers: String = sign_headers
        .iter()
        .map(|(k, v)| format!("{}:{}\n", k, v.trim()))
        .collect();

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        "PUT", path, "", canonical_headers, signed_headers_str, payload_hash
    );
    let scope = format!("{}/{}/s3/aws4_request", date_stamp, REGION);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date,
        scope,
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );
    let key = format!("AWS4{}", SECRET_KEY);
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).unwrap();
    mac.update(date_stamp.as_bytes());
    let date_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_key).unwrap();
    mac.update(REGION.as_bytes());
    let date_region_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_region_key).unwrap();
    mac.update(b"s3");
    let date_region_service_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_region_service_key).unwrap();
    mac.update(b"aws4_request");
    let signing_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&signing_key).unwrap();
    mac.update(string_to_sign.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={}/{},SignedHeaders={},Signature={}",
        ACCESS_KEY, scope, signed_headers_str, signature
    );

    // Build multi-chunk body
    let chunk_sig = "0".repeat(64);
    let mut chunked_body = Vec::new();
    for chunk_data in [&chunk1[..], &chunk2[..], &chunk3[..]] {
        chunked_body.extend_from_slice(
            format!("{:x};chunk-signature={}\r\n", chunk_data.len(), chunk_sig).as_bytes(),
        );
        chunked_body.extend_from_slice(chunk_data);
        chunked_body.extend_from_slice(b"\r\n");
    }
    // Terminating chunk
    chunked_body.extend_from_slice(format!("0;chunk-signature={}\r\n", chunk_sig).as_bytes());

    let resp = client()
        .put(&url)
        .header("host", &host_header)
        .header("x-amz-date", &amz_date)
        .header("x-amz-content-sha256", payload_hash)
        .header("x-amz-decoded-content-length", total_len.to_string())
        .header("authorization", &auth)
        .header("content-type", "application/octet-stream")
        .body(chunked_body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    // Verify all chunks were concatenated correctly
    let resp = s3_request("GET", &url, vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.bytes().await.unwrap();
    let expected = b"first chunk data second chunk data third chunk data";
    assert_eq!(
        body.as_ref(),
        expected,
        "Multi-chunk content should be concatenated"
    );

    // Verify content-length matches
    let resp = s3_request("HEAD", &url, vec![]).await;
    assert_eq!(
        resp.headers().get("content-length").unwrap(),
        &total_len.to_string()
    );
}

#[tokio::test]
async fn test_multipart_create_upload() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;

    let resp = s3_request(
        "POST",
        &format!("{}/mybucket/large.bin?uploads=", base_url),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    let upload_id = extract_xml_tag(&body, "UploadId").unwrap();
    assert!(!upload_id.is_empty());
}

#[tokio::test]
async fn test_multipart_upload_part() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    let create = s3_request(
        "POST",
        &format!("{}/mybucket/large.bin?uploads=", base_url),
        vec![],
    )
    .await;
    let upload_id = extract_xml_tag(&create.text().await.unwrap(), "UploadId").unwrap();

    let resp = s3_request(
        "PUT",
        &format!(
            "{}/mybucket/large.bin?partNumber=1&uploadId={}",
            base_url, upload_id
        ),
        b"part-one".to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let etag = resp.headers().get("etag").unwrap().to_str().unwrap();
    assert!(etag.starts_with('"') && etag.ends_with('"'));
}

#[tokio::test]
async fn test_multipart_complete() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    let create = s3_request(
        "POST",
        &format!("{}/mybucket/large.bin?uploads=", base_url),
        vec![],
    )
    .await;
    let upload_id = extract_xml_tag(&create.text().await.unwrap(), "UploadId").unwrap();

    let p1 = vec![b'a'; 5 * 1024 * 1024];
    let p2 = b"tail".to_vec();
    let r1 = s3_request(
        "PUT",
        &format!(
            "{}/mybucket/large.bin?partNumber=1&uploadId={}",
            base_url, upload_id
        ),
        p1.clone(),
    )
    .await;
    let e1 = r1
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let r2 = s3_request(
        "PUT",
        &format!(
            "{}/mybucket/large.bin?partNumber=2&uploadId={}",
            base_url, upload_id
        ),
        p2.clone(),
    )
    .await;
    let e2 = r2
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let complete_xml = format!(
        "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{}</ETag></Part><Part><PartNumber>2</PartNumber><ETag>{}</ETag></Part></CompleteMultipartUpload>",
        e1, e2
    );
    let complete = s3_request(
        "POST",
        &format!("{}/mybucket/large.bin?uploadId={}", base_url, upload_id),
        complete_xml.into_bytes(),
    )
    .await;
    assert_eq!(complete.status(), 200);

    let get = s3_request("GET", &format!("{}/mybucket/large.bin", base_url), vec![]).await;
    assert_eq!(get.status(), 200);
    let body = get.bytes().await.unwrap();
    let mut expected = p1;
    expected.extend_from_slice(&p2);
    assert_eq!(body.as_ref(), expected.as_slice());
}

#[tokio::test]
async fn test_multipart_get_part_number() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    let create = s3_request(
        "POST",
        &format!("{}/mybucket/parts.bin?uploads=", base_url),
        vec![],
    )
    .await;
    let upload_id = extract_xml_tag(&create.text().await.unwrap(), "UploadId").unwrap();

    let p1 = vec![b'A'; 5 * 1024 * 1024];
    let p2 = vec![b'B'; 3 * 1024 * 1024];
    let r1 = s3_request(
        "PUT",
        &format!(
            "{}/mybucket/parts.bin?partNumber=1&uploadId={}",
            base_url, upload_id
        ),
        p1.clone(),
    )
    .await;
    let e1 = r1
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let r2 = s3_request(
        "PUT",
        &format!(
            "{}/mybucket/parts.bin?partNumber=2&uploadId={}",
            base_url, upload_id
        ),
        p2.clone(),
    )
    .await;
    let e2 = r2
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let complete_xml = format!(
        "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{}</ETag></Part><Part><PartNumber>2</PartNumber><ETag>{}</ETag></Part></CompleteMultipartUpload>",
        e1, e2
    );
    let complete = s3_request(
        "POST",
        &format!("{}/mybucket/parts.bin?uploadId={}", base_url, upload_id),
        complete_xml.into_bytes(),
    )
    .await;
    assert_eq!(complete.status(), 200);

    // GET partNumber=1 should return only part 1 data
    let get_p1 = s3_request(
        "GET",
        &format!("{}/mybucket/parts.bin?partNumber=1", base_url),
        vec![],
    )
    .await;
    assert_eq!(get_p1.status(), 206);
    assert_eq!(
        get_p1.headers().get("content-length").unwrap(),
        &(5 * 1024 * 1024).to_string()
    );
    assert_eq!(get_p1.headers().get("x-amz-mp-parts-count").unwrap(), "2");
    let body1 = get_p1.bytes().await.unwrap();
    assert_eq!(body1.len(), 5 * 1024 * 1024);
    assert!(body1.iter().all(|&b| b == b'A'));

    // GET partNumber=2 should return only part 2 data
    let get_p2 = s3_request(
        "GET",
        &format!("{}/mybucket/parts.bin?partNumber=2", base_url),
        vec![],
    )
    .await;
    assert_eq!(get_p2.status(), 206);
    assert_eq!(
        get_p2.headers().get("content-length").unwrap(),
        &(3 * 1024 * 1024).to_string()
    );
    let body2 = get_p2.bytes().await.unwrap();
    assert_eq!(body2.len(), 3 * 1024 * 1024);
    assert!(body2.iter().all(|&b| b == b'B'));

    // HEAD partNumber=1 should return part-specific headers
    let head_p1 = s3_request(
        "HEAD",
        &format!("{}/mybucket/parts.bin?partNumber=1", base_url),
        vec![],
    )
    .await;
    assert_eq!(head_p1.status(), 206);
    assert_eq!(
        head_p1.headers().get("content-length").unwrap(),
        &(5 * 1024 * 1024).to_string()
    );
    assert_eq!(head_p1.headers().get("x-amz-mp-parts-count").unwrap(), "2");
}

#[tokio::test]
async fn test_multipart_complete_part_too_small() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    let create = s3_request(
        "POST",
        &format!("{}/mybucket/large.bin?uploads=", base_url),
        vec![],
    )
    .await;
    let upload_id = extract_xml_tag(&create.text().await.unwrap(), "UploadId").unwrap();

    let r1 = s3_request(
        "PUT",
        &format!(
            "{}/mybucket/large.bin?partNumber=1&uploadId={}",
            base_url, upload_id
        ),
        b"tiny".to_vec(),
    )
    .await;
    let e1 = r1
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let r2 = s3_request(
        "PUT",
        &format!(
            "{}/mybucket/large.bin?partNumber=2&uploadId={}",
            base_url, upload_id
        ),
        b"tail".to_vec(),
    )
    .await;
    let e2 = r2
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let complete_xml = format!(
        "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{}</ETag></Part><Part><PartNumber>2</PartNumber><ETag>{}</ETag></Part></CompleteMultipartUpload>",
        e1, e2
    );
    let complete = s3_request(
        "POST",
        &format!("{}/mybucket/large.bin?uploadId={}", base_url, upload_id),
        complete_xml.into_bytes(),
    )
    .await;
    assert_eq!(complete.status(), 400);
    let body = complete.text().await.unwrap();
    assert!(body.contains("<Code>EntityTooSmall</Code>"));
}

#[tokio::test]
async fn test_multipart_abort() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    let create = s3_request(
        "POST",
        &format!("{}/mybucket/large.bin?uploads=", base_url),
        vec![],
    )
    .await;
    let upload_id = extract_xml_tag(&create.text().await.unwrap(), "UploadId").unwrap();

    let abort = s3_request(
        "DELETE",
        &format!("{}/mybucket/large.bin?uploadId={}", base_url, upload_id),
        vec![],
    )
    .await;
    assert_eq!(abort.status(), 204);
}

#[tokio::test]
async fn test_multipart_list_parts() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    let create = s3_request(
        "POST",
        &format!("{}/mybucket/large.bin?uploads=", base_url),
        vec![],
    )
    .await;
    let upload_id = extract_xml_tag(&create.text().await.unwrap(), "UploadId").unwrap();

    s3_request(
        "PUT",
        &format!(
            "{}/mybucket/large.bin?partNumber=1&uploadId={}",
            base_url, upload_id
        ),
        b"part-one".to_vec(),
    )
    .await;

    let list = s3_request(
        "GET",
        &format!("{}/mybucket/large.bin?uploadId={}", base_url, upload_id),
        vec![],
    )
    .await;
    assert_eq!(list.status(), 200);
    let body = list.text().await.unwrap();
    assert!(body.contains("<PartNumber>1</PartNumber>"));
}

#[tokio::test]
async fn test_multipart_list_uploads() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    let create = s3_request(
        "POST",
        &format!("{}/mybucket/large.bin?uploads=", base_url),
        vec![],
    )
    .await;
    let upload_id = extract_xml_tag(&create.text().await.unwrap(), "UploadId").unwrap();

    let list = s3_request("GET", &format!("{}/mybucket?uploads=", base_url), vec![]).await;
    assert_eq!(list.status(), 200);
    let body = list.text().await.unwrap();
    assert!(body.contains(&upload_id));
    assert!(body.contains("<Key>large.bin</Key>"));
}

#[tokio::test]
async fn test_multipart_no_such_upload() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;

    let resp = s3_request(
        "GET",
        &format!("{}/mybucket/missing.bin?uploadId=does-not-exist", base_url),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 404);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Code>NoSuchUpload</Code>"));
}

#[tokio::test]
async fn test_multipart_excluded_from_list_objects() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    let create = s3_request(
        "POST",
        &format!("{}/mybucket/in-progress.bin?uploads=", base_url),
        vec![],
    )
    .await;
    let upload_id = extract_xml_tag(&create.text().await.unwrap(), "UploadId").unwrap();
    s3_request(
        "PUT",
        &format!(
            "{}/mybucket/in-progress.bin?partNumber=1&uploadId={}",
            base_url, upload_id
        ),
        b"partial".to_vec(),
    )
    .await;

    let list = s3_request("GET", &format!("{}/mybucket?list-type=2", base_url), vec![]).await;
    assert_eq!(list.status(), 200);
    let body = list.text().await.unwrap();
    assert!(!body.contains("in-progress.bin"));
}

#[tokio::test]
async fn test_multipart_etag_format() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    let create = s3_request(
        "POST",
        &format!("{}/mybucket/etag.bin?uploads=", base_url),
        vec![],
    )
    .await;
    let upload_id = extract_xml_tag(&create.text().await.unwrap(), "UploadId").unwrap();

    let p1 = vec![b'a'; 5 * 1024 * 1024];
    let p2 = b"tail".to_vec();
    let r1 = s3_request(
        "PUT",
        &format!(
            "{}/mybucket/etag.bin?partNumber=1&uploadId={}",
            base_url, upload_id
        ),
        p1,
    )
    .await;
    let e1 = r1
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let r2 = s3_request(
        "PUT",
        &format!(
            "{}/mybucket/etag.bin?partNumber=2&uploadId={}",
            base_url, upload_id
        ),
        p2,
    )
    .await;
    let e2 = r2
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let complete_xml = format!(
        "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{}</ETag></Part><Part><PartNumber>2</PartNumber><ETag>{}</ETag></Part></CompleteMultipartUpload>",
        e1, e2
    );
    let complete = s3_request(
        "POST",
        &format!("{}/mybucket/etag.bin?uploadId={}", base_url, upload_id),
        complete_xml.into_bytes(),
    )
    .await;
    let body = complete.text().await.unwrap();
    let etag = extract_xml_tag(&body, "ETag").unwrap();
    assert!(etag.starts_with('"') && etag.ends_with('"'));
    assert!(etag.contains("-2"));
}

#[tokio::test]
async fn test_copy_object_basic() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;

    // Upload source object
    s3_request(
        "PUT",
        &format!("{}/mybucket/src.txt", base_url),
        b"copy me".to_vec(),
    )
    .await;

    // Copy to new key in same bucket
    let resp = s3_request_with_headers(
        "PUT",
        &format!("{}/mybucket/dst.txt", base_url),
        vec![],
        vec![("x-amz-copy-source", "/mybucket/src.txt")],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<CopyObjectResult>"));
    assert!(body.contains("<ETag>"));
    assert!(body.contains("<LastModified>"));

    // Verify destination content matches source
    let resp = s3_request("GET", &format!("{}/mybucket/dst.txt", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    let content = resp.bytes().await.unwrap();
    assert_eq!(content.as_ref(), b"copy me");
}

#[tokio::test]
async fn test_copy_object_cross_bucket() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/src-bucket", base_url), vec![]).await;
    s3_request("PUT", &format!("{}/dst-bucket", base_url), vec![]).await;

    s3_request(
        "PUT",
        &format!("{}/src-bucket/file.txt", base_url),
        b"cross bucket".to_vec(),
    )
    .await;

    let resp = s3_request_with_headers(
        "PUT",
        &format!("{}/dst-bucket/file.txt", base_url),
        vec![],
        vec![("x-amz-copy-source", "/src-bucket/file.txt")],
    )
    .await;
    assert_eq!(resp.status(), 200);

    let resp = s3_request("GET", &format!("{}/dst-bucket/file.txt", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), b"cross bucket");
}

#[tokio::test]
async fn test_copy_object_metadata_copy() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;

    // Upload with specific content-type
    s3_request_with_headers(
        "PUT",
        &format!("{}/mybucket/src.txt", base_url),
        b"hello".to_vec(),
        vec![("content-type", "text/plain")],
    )
    .await;

    // Copy with default COPY directive
    s3_request_with_headers(
        "PUT",
        &format!("{}/mybucket/dst.txt", base_url),
        vec![],
        vec![("x-amz-copy-source", "/mybucket/src.txt")],
    )
    .await;

    // HEAD destination — content-type should be preserved
    let resp = s3_request("HEAD", &format!("{}/mybucket/dst.txt", base_url), vec![]).await;
    assert_eq!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "text/plain"
    );
}

#[tokio::test]
async fn test_copy_object_metadata_replace() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;

    s3_request_with_headers(
        "PUT",
        &format!("{}/mybucket/src.txt", base_url),
        b"hello".to_vec(),
        vec![("content-type", "text/plain")],
    )
    .await;

    // Copy with REPLACE directive and new content-type
    s3_request_with_headers(
        "PUT",
        &format!("{}/mybucket/dst.txt", base_url),
        vec![],
        vec![
            ("x-amz-copy-source", "/mybucket/src.txt"),
            ("x-amz-metadata-directive", "REPLACE"),
            ("content-type", "application/json"),
        ],
    )
    .await;

    let resp = s3_request("HEAD", &format!("{}/mybucket/dst.txt", base_url), vec![]).await;
    assert_eq!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "application/json"
    );
}

#[tokio::test]
async fn test_copy_object_source_not_found() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;

    let resp = s3_request_with_headers(
        "PUT",
        &format!("{}/mybucket/dst.txt", base_url),
        vec![],
        vec![("x-amz-copy-source", "/mybucket/nonexistent.txt")],
    )
    .await;
    assert_eq!(resp.status(), 404);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Code>NoSuchKey</Code>"));
}

#[tokio::test]
async fn test_copy_object_no_leading_slash() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/mybucket/src.txt", base_url),
        b"no slash".to_vec(),
    )
    .await;

    // Copy source without leading slash
    let resp = s3_request_with_headers(
        "PUT",
        &format!("{}/mybucket/dst.txt", base_url),
        vec![],
        vec![("x-amz-copy-source", "mybucket/src.txt")],
    )
    .await;
    assert_eq!(resp.status(), 200);

    let resp = s3_request("GET", &format!("{}/mybucket/dst.txt", base_url), vec![]).await;
    assert_eq!(resp.bytes().await.unwrap().as_ref(), b"no slash");
}

/// Generate a presigned URL for the given method/path.
fn presign_url(base_url: &str, method: &str, path: &str, expires_secs: u64) -> String {
    let parsed = reqwest::Url::parse(&format!("{}{}", base_url, path)).unwrap();
    let host = parsed.host_str().unwrap();
    let port = parsed.port().unwrap();
    let host_header = format!("{}:{}", host, port);

    let now = chrono::Utc::now();
    let date_stamp = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let credential = format!("{}/{}/{}/s3/aws4_request", ACCESS_KEY, date_stamp, REGION);

    let mut qs_params = vec![
        (
            "X-Amz-Algorithm".to_string(),
            "AWS4-HMAC-SHA256".to_string(),
        ),
        ("X-Amz-Credential".to_string(), credential.clone()),
        ("X-Amz-Date".to_string(), amz_date.clone()),
        ("X-Amz-Expires".to_string(), expires_secs.to_string()),
        ("X-Amz-SignedHeaders".to_string(), "host".to_string()),
    ];
    qs_params.sort();

    let canonical_qs: String = qs_params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode_s3(k), percent_encode_s3(v)))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_headers = format!("host:{}\n", host_header);
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\nhost\nUNSIGNED-PAYLOAD",
        method, path, canonical_qs, canonical_headers
    );

    let scope = format!("{}/{}/s3/aws4_request", date_stamp, REGION);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date,
        scope,
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );

    let key = format!("AWS4{}", SECRET_KEY);
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).unwrap();
    mac.update(date_stamp.as_bytes());
    let date_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_key).unwrap();
    mac.update(REGION.as_bytes());
    let date_region_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_region_key).unwrap();
    mac.update(b"s3");
    let date_region_service_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_region_service_key).unwrap();
    mac.update(b"aws4_request");
    let signing_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&signing_key).unwrap();
    mac.update(string_to_sign.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    format!(
        "{}{}?{}&X-Amz-Signature={}",
        base_url, path, canonical_qs, signature
    )
}

fn percent_encode_s3(input: &str) -> String {
    const S3_URI_ENCODE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    percent_encoding::utf8_percent_encode(input, S3_URI_ENCODE).to_string()
}

#[tokio::test]
async fn test_presigned_get_object() {
    let base_url = start_server().await;

    let url = format!("{}/presign-bucket", base_url);
    s3_request("PUT", &url, vec![]).await;

    let body = b"presigned test content";
    let url = format!("{}/presign-bucket/test.txt", base_url);
    s3_request("PUT", &url, body.to_vec()).await;

    let presigned = presign_url(&base_url, "GET", "/presign-bucket/test.txt", 300);
    let resp = client().get(&presigned).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), body);
}

#[tokio::test]
async fn test_presigned_put_object() {
    let base_url = start_server().await;

    let url = format!("{}/presign-put-bucket", base_url);
    s3_request("PUT", &url, vec![]).await;

    let presigned = presign_url(&base_url, "PUT", "/presign-put-bucket/uploaded.txt", 300);
    let body = b"uploaded via presigned PUT";
    let resp = client()
        .put(&presigned)
        .body(body.to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let url = format!("{}/presign-put-bucket/uploaded.txt", base_url);
    let resp = s3_request("GET", &url, vec![]).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), body);
}

#[tokio::test]
async fn test_presigned_head_object() {
    let base_url = start_server().await;

    let url = format!("{}/presign-head-bucket", base_url);
    s3_request("PUT", &url, vec![]).await;

    let url = format!("{}/presign-head-bucket/test.txt", base_url);
    s3_request("PUT", &url, b"head test".to_vec()).await;

    let presigned = presign_url(&base_url, "HEAD", "/presign-head-bucket/test.txt", 300);
    let resp = client().head(&presigned).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()
            .get("content-length")
            .unwrap()
            .to_str()
            .unwrap(),
        "9"
    );
}

#[tokio::test]
async fn test_presigned_expired_url() {
    let base_url = start_server().await;

    let url = format!("{}/presign-expire-bucket", base_url);
    s3_request("PUT", &url, vec![]).await;
    let url = format!("{}/presign-expire-bucket/test.txt", base_url);
    s3_request("PUT", &url, b"data".to_vec()).await;

    // Manually craft a presigned URL with a timestamp from 2 hours ago
    let parsed =
        reqwest::Url::parse(&format!("{}/presign-expire-bucket/test.txt", base_url)).unwrap();
    let host = parsed.host_str().unwrap();
    let port = parsed.port().unwrap();
    let host_header = format!("{}:{}", host, port);

    let past = chrono::Utc::now() - chrono::Duration::hours(2);
    let date_stamp = past.format("%Y%m%d").to_string();
    let amz_date = past.format("%Y%m%dT%H%M%SZ").to_string();
    let credential = format!("{}/{}/{}/s3/aws4_request", ACCESS_KEY, date_stamp, REGION);

    let mut qs_params = vec![
        (
            "X-Amz-Algorithm".to_string(),
            "AWS4-HMAC-SHA256".to_string(),
        ),
        ("X-Amz-Credential".to_string(), credential.clone()),
        ("X-Amz-Date".to_string(), amz_date.clone()),
        ("X-Amz-Expires".to_string(), "60".to_string()),
        ("X-Amz-SignedHeaders".to_string(), "host".to_string()),
    ];
    qs_params.sort();
    let canonical_qs: String = qs_params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode_s3(k), percent_encode_s3(v)))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_request = format!(
        "GET\n/presign-expire-bucket/test.txt\n{}\nhost:{}\n\nhost\nUNSIGNED-PAYLOAD",
        canonical_qs, host_header
    );
    let scope = format!("{}/{}/s3/aws4_request", date_stamp, REGION);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date,
        scope,
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );

    let key = format!("AWS4{}", SECRET_KEY);
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).unwrap();
    mac.update(date_stamp.as_bytes());
    let date_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_key).unwrap();
    mac.update(REGION.as_bytes());
    let date_region_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_region_key).unwrap();
    mac.update(b"s3");
    let date_region_service_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&date_region_service_key).unwrap();
    mac.update(b"aws4_request");
    let signing_key = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&signing_key).unwrap();
    mac.update(string_to_sign.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    let presigned = format!(
        "{}/presign-expire-bucket/test.txt?{}&X-Amz-Signature={}",
        base_url, canonical_qs, signature
    );

    let resp = client().get(&presigned).send().await.unwrap();
    assert_eq!(resp.status(), 403);
    let body = resp.text().await.unwrap();
    assert!(body.contains("Request has expired"));
}

#[tokio::test]
async fn test_presigned_bad_signature() {
    let base_url = start_server().await;

    let url = format!("{}/presign-bad-sig-bucket", base_url);
    s3_request("PUT", &url, vec![]).await;

    let mut presigned = presign_url(&base_url, "GET", "/presign-bad-sig-bucket/test.txt", 300);
    let last = presigned.pop().unwrap();
    presigned.push(if last == 'a' { 'b' } else { 'a' });

    let resp = client().get(&presigned).send().await.unwrap();
    assert_eq!(resp.status(), 403);
}

// ── Console presign endpoint tests ───────────────────────────────────

/// Helper: login via console API and return the session cookie value.
async fn console_login(base_url: &str) -> String {
    let resp = client()
        .post(&format!("{}/api/auth/login", base_url))
        .json(&serde_json::json!({"accessKey": ACCESS_KEY, "secretKey": SECRET_KEY}))
        .send()
        .await
        .unwrap();
    if resp.status() != 200 {
        let status = resp.status();
        let body = resp.text().await.unwrap();
        panic!("login failed with status {}: {}", status, body);
    }
    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .expect("login should set cookie")
        .to_str()
        .unwrap()
        .to_string();
    // Extract value from "maxio_session=VALUE; ..."
    let value = set_cookie
        .strip_prefix("maxio_session=")
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    value.to_string()
}

#[tokio::test]
async fn test_console_mutation_allows_dev_loopback_origin_via_vite_proxy() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;

    let resp = client()
        .post(format!("{}/api/buckets", base_url))
        .header("cookie", format!("maxio_session={}", session))
        .header("origin", "http://127.0.0.1:5173")
        .json(&serde_json::json!({ "name": "dev-origin-bucket" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "body: {}", resp.text().await.unwrap());
}

#[tokio::test]
async fn test_console_mutation_rejects_cross_site_origin() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;

    let resp = client()
        .post(format!("{}/api/buckets", base_url))
        .header("cookie", format!("maxio_session={}", session))
        .header("origin", "http://evil.example")
        .json(&serde_json::json!({ "name": "evil-origin-bucket" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn test_console_list_objects_search() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/search-bucket", base_url), vec![]).await;

    for key in [
        "alpha.txt",
        "folder/beta.txt",
        "folder/gamma-alpha.txt",
        "other.txt",
    ] {
        s3_request(
            "PUT",
            &format!("{}/search-bucket/{}", base_url, key),
            b"x".to_vec(),
        )
        .await;
    }

    let session = console_login(&base_url).await;

    let resp = client()
        .get(format!(
            "{}/api/buckets/search-bucket/objects?q=alpha",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let keys: Vec<String> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(keys, vec!["alpha.txt".to_string()]);
    let prefixes: Vec<String> = body["prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    assert_eq!(prefixes, vec!["folder/".to_string()]);

    let resp = client()
        .get(format!(
            "{}/api/buckets/search-bucket/objects?q=folder",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["files"].as_array().unwrap().is_empty());
    let prefixes: Vec<String> = body["prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    assert_eq!(prefixes, vec!["folder/".to_string()]);

    s3_request(
        "PUT",
        &format!("{}/search-bucket/empty-archive/", base_url),
        vec![],
    )
    .await;

    let resp = client()
        .get(format!(
            "{}/api/buckets/search-bucket/objects?q=archive",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let prefixes: Vec<String> = body["prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    assert!(prefixes.contains(&"empty-archive/".to_string()));

    let resp = client()
        .get(format!(
            "{}/api/buckets/search-bucket/objects?prefix=folder/&q=alpha",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let keys: Vec<String> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(keys, vec!["folder/gamma-alpha.txt".to_string()]);
}

#[tokio::test]
async fn test_console_list_objects_omits_current_folder_marker() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/marker-bucket", base_url), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/marker-bucket/nested/", base_url),
        vec![],
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/marker-bucket/nested/file.txt", base_url),
        b"x".to_vec(),
    )
    .await;

    let session = console_login(&base_url).await;

    let resp = client()
        .get(format!(
            "{}/api/buckets/marker-bucket/objects?prefix=nested/",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let prefixes: Vec<String> = body["prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    assert!(!prefixes.contains(&"nested/".to_string()));
    let keys: Vec<String> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(keys, vec!["nested/file.txt".to_string()]);
}

#[tokio::test]
async fn test_console_delete_object_missing_bucket_returns_404() {
    let base_url = start_server().await;
    let session = console_login(&base_url).await;

    let resp = client()
        .delete(&format!(
            "{}/api/buckets/missing-bucket/objects/file.txt",
            base_url
        ))
        .header("cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "Bucket not found");
}

#[tokio::test]
async fn test_console_presign_simple_key() {
    let base_url = start_server().await;

    // Create bucket and upload object via S3 API
    s3_request("PUT", &format!("{}/cpresign-bucket", base_url), vec![]).await;
    let body = b"console presign test";
    s3_request(
        "PUT",
        &format!("{}/cpresign-bucket/test.txt", base_url),
        body.to_vec(),
    )
    .await;

    // Login to console API
    let session = console_login(&base_url).await;

    // Generate presigned URL via console endpoint
    let resp = client()
        .get(&format!(
            "{}/api/buckets/cpresign-bucket/presign/test.txt?expires=300",
            base_url
        ))
        .header("Cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    let presigned_url = json["url"]
        .as_str()
        .expect("response should have url field");

    // Fetch the presigned URL without any auth — should succeed
    let resp = client().get(presigned_url).send().await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "presigned URL should return 200, got {}",
        resp.status()
    );
    assert_eq!(resp.bytes().await.unwrap().as_ref(), body);
}

#[tokio::test]
async fn test_console_presign_key_with_spaces() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/cpresign-space", base_url), vec![]).await;
    let body = b"file with spaces";
    // Upload with a key containing spaces (URL-encoded in the request)
    s3_request(
        "PUT",
        &format!("{}/cpresign-space/my%20file.txt", base_url),
        body.to_vec(),
    )
    .await;

    let session = console_login(&base_url).await;

    // Request presigned URL for the key with spaces (URL-encoded in the API path)
    let resp = client()
        .get(&format!(
            "{}/api/buckets/cpresign-space/presign/my%20file.txt?expires=300",
            base_url
        ))
        .header("Cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    let presigned_url = json["url"]
        .as_str()
        .expect("response should have url field");

    let resp = client().get(presigned_url).send().await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "presigned URL for key with spaces should return 200, got {}",
        resp.status()
    );
    assert_eq!(resp.bytes().await.unwrap().as_ref(), body);
}

#[tokio::test]
async fn test_console_presign_nested_key() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/cpresign-nested", base_url), vec![]).await;
    let body = b"nested key content";
    s3_request(
        "PUT",
        &format!("{}/cpresign-nested/folder/sub/file.txt", base_url),
        body.to_vec(),
    )
    .await;

    let session = console_login(&base_url).await;

    let resp = client()
        .get(&format!(
            "{}/api/buckets/cpresign-nested/presign/folder/sub/file.txt?expires=300",
            base_url
        ))
        .header("Cookie", format!("maxio_session={}", session))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    let presigned_url = json["url"]
        .as_str()
        .expect("response should have url field");

    let resp = client().get(presigned_url).send().await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "presigned URL for nested key should return 200, got {}",
        resp.status()
    );
    assert_eq!(resp.bytes().await.unwrap().as_ref(), body);
}

#[tokio::test]
async fn test_console_presign_uses_forwarded_host() {
    let base_url = start_server().await;

    s3_request("PUT", &format!("{}/cpresign-proxy", base_url), vec![]).await;
    let body = b"proxy presign test";
    s3_request(
        "PUT",
        &format!("{}/cpresign-proxy/test.txt", base_url),
        body.to_vec(),
    )
    .await;

    let session = console_login(&base_url).await;

    let resp = client()
        .get(&format!(
            "{}/api/buckets/cpresign-proxy/presign/test.txt?expires=300",
            base_url
        ))
        .header("Cookie", format!("maxio_session={}", session))
        .header("X-Forwarded-Host", "cdn.example.com")
        .header("X-Forwarded-Proto", "https")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    let presigned_url = json["url"]
        .as_str()
        .expect("response should have url field");
    assert!(
        presigned_url.starts_with("https://cdn.example.com/cpresign-proxy/test.txt?"),
        "unexpected presigned URL: {presigned_url}"
    );

    let parsed = reqwest::Url::parse(presigned_url).unwrap();
    let path = parsed.path();
    let query = parsed.query().expect("presigned URL should have query");
    let resp = client()
        .get(format!("{base_url}{path}?{query}"))
        .header("Host", "cdn.example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "presigned URL behind proxy host should return 200, got {}",
        resp.status()
    );
    assert_eq!(resp.bytes().await.unwrap().as_ref(), body);
}

// ── Range request tests ──────────────────────────────────────────────

#[tokio::test]
async fn test_get_object_range_first_bytes() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/range-bucket", base_url), vec![]).await;

    let content: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
    s3_request_with_headers(
        "PUT",
        &format!("{}/range-bucket/file.bin", base_url),
        content.clone(),
        vec![],
    )
    .await;

    let resp = s3_request_with_headers(
        "GET",
        &format!("{}/range-bucket/file.bin", base_url),
        vec![],
        vec![("range", "bytes=0-499")],
    )
    .await;

    assert_eq!(resp.status(), 206);
    assert_eq!(resp.headers()["content-length"], "500");
    assert_eq!(resp.headers()["content-range"], "bytes 0-499/1000");
    assert_eq!(resp.headers()["accept-ranges"], "bytes");
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), &content[0..500]);
}

#[tokio::test]
async fn test_get_object_range_middle_bytes() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/range-mid-bucket", base_url), vec![]).await;

    let content: Vec<u8> = (0..100).map(|i| (i % 256) as u8).collect();
    s3_request_with_headers(
        "PUT",
        &format!("{}/range-mid-bucket/file.bin", base_url),
        content.clone(),
        vec![],
    )
    .await;

    let resp = s3_request_with_headers(
        "GET",
        &format!("{}/range-mid-bucket/file.bin", base_url),
        vec![],
        vec![("range", "bytes=10-19")],
    )
    .await;

    assert_eq!(resp.status(), 206);
    assert_eq!(resp.headers()["content-length"], "10");
    assert_eq!(resp.headers()["content-range"], "bytes 10-19/100");
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), &content[10..20]);
}

#[tokio::test]
async fn test_get_object_range_suffix() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/range-sfx-bucket", base_url), vec![]).await;

    let content: Vec<u8> = (0u16..1000).map(|i| (i % 256) as u8).collect();
    s3_request_with_headers(
        "PUT",
        &format!("{}/range-sfx-bucket/file.bin", base_url),
        content.clone(),
        vec![],
    )
    .await;

    let resp = s3_request_with_headers(
        "GET",
        &format!("{}/range-sfx-bucket/file.bin", base_url),
        vec![],
        vec![("range", "bytes=-100")],
    )
    .await;

    assert_eq!(resp.status(), 206);
    assert_eq!(resp.headers()["content-length"], "100");
    assert_eq!(resp.headers()["content-range"], "bytes 900-999/1000");
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), &content[900..1000]);
}

#[tokio::test]
async fn test_get_object_range_open_end() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/range-open-bucket", base_url), vec![]).await;

    let content: Vec<u8> = (0u16..1000).map(|i| (i % 256) as u8).collect();
    s3_request_with_headers(
        "PUT",
        &format!("{}/range-open-bucket/file.bin", base_url),
        content.clone(),
        vec![],
    )
    .await;

    let resp = s3_request_with_headers(
        "GET",
        &format!("{}/range-open-bucket/file.bin", base_url),
        vec![],
        vec![("range", "bytes=500-")],
    )
    .await;

    assert_eq!(resp.status(), 206);
    assert_eq!(resp.headers()["content-length"], "500");
    assert_eq!(resp.headers()["content-range"], "bytes 500-999/1000");
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), &content[500..1000]);
}

#[tokio::test]
async fn test_get_object_range_clamp_beyond_end() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/range-clamp-bucket", base_url), vec![]).await;

    let content: Vec<u8> = (0..100).map(|i| (i % 256) as u8).collect();
    s3_request_with_headers(
        "PUT",
        &format!("{}/range-clamp-bucket/file.bin", base_url),
        content.clone(),
        vec![],
    )
    .await;

    let resp = s3_request_with_headers(
        "GET",
        &format!("{}/range-clamp-bucket/file.bin", base_url),
        vec![],
        vec![("range", "bytes=0-9999")],
    )
    .await;

    assert_eq!(resp.status(), 206);
    assert_eq!(resp.headers()["content-length"], "100");
    assert_eq!(resp.headers()["content-range"], "bytes 0-99/100");
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), &content[..]);
}

#[tokio::test]
async fn test_get_object_range_invalid_416() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/range-416-bucket", base_url), vec![]).await;

    let content: Vec<u8> = (0..100).map(|i| (i % 256) as u8).collect();
    s3_request_with_headers(
        "PUT",
        &format!("{}/range-416-bucket/file.bin", base_url),
        content,
        vec![],
    )
    .await;

    let resp = s3_request_with_headers(
        "GET",
        &format!("{}/range-416-bucket/file.bin", base_url),
        vec![],
        vec![("range", "bytes=5000-6000")],
    )
    .await;

    assert_eq!(resp.status(), 416);
}

#[tokio::test]
async fn test_get_object_no_range_has_accept_ranges() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/range-ar-bucket", base_url), vec![]).await;

    s3_request_with_headers(
        "PUT",
        &format!("{}/range-ar-bucket/file.txt", base_url),
        b"hello".to_vec(),
        vec![],
    )
    .await;

    let resp = s3_request_with_headers(
        "GET",
        &format!("{}/range-ar-bucket/file.txt", base_url),
        vec![],
        vec![],
    )
    .await;

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["accept-ranges"], "bytes");
}

#[tokio::test]
async fn test_get_object_range_preserves_headers() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/range-hdr-bucket", base_url), vec![]).await;

    s3_request_with_headers(
        "PUT",
        &format!("{}/range-hdr-bucket/file.txt", base_url),
        b"hello world".to_vec(),
        vec![("content-type", "text/plain")],
    )
    .await;

    let resp = s3_request_with_headers(
        "GET",
        &format!("{}/range-hdr-bucket/file.txt", base_url),
        vec![],
        vec![("range", "bytes=0-4")],
    )
    .await;

    assert_eq!(resp.status(), 206);
    assert!(resp.headers().contains_key("etag"));
    assert!(resp.headers().contains_key("last-modified"));
    assert!(resp.headers().contains_key("content-type"));
}

#[tokio::test]
async fn test_head_object_accept_ranges() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/range-head-bucket", base_url), vec![]).await;

    s3_request_with_headers(
        "PUT",
        &format!("{}/range-head-bucket/file.txt", base_url),
        b"hello".to_vec(),
        vec![],
    )
    .await;

    let resp = s3_request_with_headers(
        "HEAD",
        &format!("{}/range-head-bucket/file.txt", base_url),
        vec![],
        vec![],
    )
    .await;

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["accept-ranges"], "bytes");
}

#[tokio::test]
async fn test_put_folder_marker() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;

    // Create folder marker via PutObject with trailing slash
    let resp = s3_request("PUT", &format!("{}/mybucket/photos/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);

    // Folder should appear in ListObjectsV2 as a CommonPrefix
    let resp = s3_request(
        "GET",
        &format!("{}/mybucket?list-type=2&delimiter=%2F", base_url),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Prefix>photos/</Prefix>"), "body: {}", body);

    // HeadObject on the folder marker should return 200
    let resp = s3_request("HEAD", &format!("{}/mybucket/photos/", base_url), vec![]).await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_folder_marker_with_children() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;

    // Create folder marker
    s3_request("PUT", &format!("{}/mybucket/docs/", base_url), vec![]).await;

    // Upload object inside it
    s3_request_with_headers(
        "PUT",
        &format!("{}/mybucket/docs/readme.txt", base_url),
        b"hello".to_vec(),
        vec![],
    )
    .await;

    // List at root — should see "docs/" as CommonPrefix
    let resp = s3_request(
        "GET",
        &format!("{}/mybucket?list-type=2&delimiter=%2F", base_url),
        vec![],
    )
    .await;
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Prefix>docs/</Prefix>"), "body: {}", body);
    assert!(
        !body.contains("readme.txt"),
        "readme.txt should not appear at root"
    );

    // List inside docs/ — should see readme.txt
    let resp = s3_request(
        "GET",
        &format!(
            "{}/mybucket?list-type=2&prefix=docs%2F&delimiter=%2F",
            base_url
        ),
        vec![],
    )
    .await;
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("<Key>docs/readme.txt</Key>"),
        "body: {}",
        body
    );

    // Delete folder marker — the child object should still exist
    s3_request("DELETE", &format!("{}/mybucket/docs/", base_url), vec![]).await;
    let resp = s3_request(
        "GET",
        &format!("{}/mybucket/docs/readme.txt", base_url),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_delete_folder_marker() {
    let base_url = start_server().await;
    s3_request("PUT", &format!("{}/mybucket", base_url), vec![]).await;

    // Create and then delete folder marker
    s3_request("PUT", &format!("{}/mybucket/empty-dir/", base_url), vec![]).await;
    s3_request(
        "DELETE",
        &format!("{}/mybucket/empty-dir/", base_url),
        vec![],
    )
    .await;

    // HeadObject should now return 404
    let resp = s3_request("HEAD", &format!("{}/mybucket/empty-dir/", base_url), vec![]).await;
    assert_eq!(resp.status(), 404);
}

// ── Object Tagging ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_put_and_get_object_tagging() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/tag-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/tag-bucket/obj.txt", base),
        b"hello".to_vec(),
    )
    .await;

    let tagging_xml = r#"<Tagging><TagSet><Tag><Key>env</Key><Value>prod</Value></Tag><Tag><Key>team</Key><Value>platform</Value></Tag></TagSet></Tagging>"#;
    let resp = s3_request(
        "PUT",
        &format!("{}/tag-bucket/obj.txt?tagging", base),
        tagging_xml.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let resp = s3_request(
        "GET",
        &format!("{}/tag-bucket/obj.txt?tagging", base),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Key>env</Key>"));
    assert!(body.contains("<Value>prod</Value>"));
    assert!(body.contains("<Key>team</Key>"));
    assert!(body.contains("<Value>platform</Value>"));
}

#[tokio::test]
async fn test_get_object_tagging_no_tags() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/notag-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/notag-bucket/obj.txt", base),
        b"hello".to_vec(),
    )
    .await;

    let resp = s3_request(
        "GET",
        &format!("{}/notag-bucket/obj.txt?tagging", base),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<Tagging>") || body.contains("<TagSet"));
    assert!(!body.contains("<Tag>"));
}

#[tokio::test]
async fn test_delete_object_tagging() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/deltag-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/deltag-bucket/obj.txt", base),
        b"hello".to_vec(),
    )
    .await;

    let tagging_xml =
        r#"<Tagging><TagSet><Tag><Key>env</Key><Value>prod</Value></Tag></TagSet></Tagging>"#;
    s3_request(
        "PUT",
        &format!("{}/deltag-bucket/obj.txt?tagging", base),
        tagging_xml.as_bytes().to_vec(),
    )
    .await;

    let resp = s3_request(
        "DELETE",
        &format!("{}/deltag-bucket/obj.txt?tagging", base),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 204);

    let resp = s3_request(
        "GET",
        &format!("{}/deltag-bucket/obj.txt?tagging", base),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(!body.contains("<Tag>"));
}

#[tokio::test]
async fn test_get_object_tagging_no_such_key() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/nsk-bucket", base), vec![]).await;

    let resp = s3_request(
        "GET",
        &format!("{}/nsk-bucket/nonexistent.txt?tagging", base),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), 404);
    let body = resp.text().await.unwrap();
    assert!(body.contains("NoSuchKey"));
}

#[tokio::test]
async fn test_put_object_tagging_too_many_tags() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/manytagbucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/manytagbucket/obj.txt", base),
        b"data".to_vec(),
    )
    .await;

    let tags: String = (1..=11)
        .map(|i| format!("<Tag><Key>key{}</Key><Value>val{}</Value></Tag>", i, i))
        .collect();
    let tagging_xml = format!("<Tagging><TagSet>{}</TagSet></Tagging>", tags);
    let resp = s3_request(
        "PUT",
        &format!("{}/manytagbucket/obj.txt?tagging", base),
        tagging_xml.into_bytes(),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body = resp.text().await.unwrap();
    assert!(body.contains("InvalidArgument"));
}

#[tokio::test]
async fn test_put_object_tagging_key_too_long() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/longtag-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/longtag-bucket/obj.txt", base),
        b"data".to_vec(),
    )
    .await;

    let long_key = "k".repeat(129);
    let tagging_xml = format!(
        "<Tagging><TagSet><Tag><Key>{}</Key><Value>v</Value></Tag></TagSet></Tagging>",
        long_key
    );
    let resp = s3_request(
        "PUT",
        &format!("{}/longtag-bucket/obj.txt?tagging", base),
        tagging_xml.into_bytes(),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body = resp.text().await.unwrap();
    assert!(body.contains("InvalidArgument"));
}

#[tokio::test]
async fn test_put_object_tagging_value_too_long() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/longval-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/longval-bucket/obj.txt", base),
        b"data".to_vec(),
    )
    .await;

    let long_val = "v".repeat(257);
    let tagging_xml = format!(
        "<Tagging><TagSet><Tag><Key>k</Key><Value>{}</Value></Tag></TagSet></Tagging>",
        long_val
    );
    let resp = s3_request(
        "PUT",
        &format!("{}/longval-bucket/obj.txt?tagging", base),
        tagging_xml.into_bytes(),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body = resp.text().await.unwrap();
    assert!(body.contains("InvalidArgument"));
}

// UploadPartCopy: copy entire source object as a multipart part
#[tokio::test]
async fn test_upload_part_copy_full() {
    let base = start_server().await;

    // Create source bucket and object
    s3_request("PUT", &format!("{}/src-upc", base), vec![]).await;
    let src_data: Vec<u8> = (0u8..255).cycle().take(5 * 1024 * 1024).collect(); // 5 MiB
    s3_request(
        "PUT",
        &format!("{}/src-upc/source.bin", base),
        src_data.clone(),
    )
    .await;

    // Create destination bucket and start multipart upload
    s3_request("PUT", &format!("{}/dst-upc", base), vec![]).await;
    let create = s3_request(
        "POST",
        &format!("{}/dst-upc/dest.bin?uploads=", base),
        vec![],
    )
    .await;
    let upload_id = extract_xml_tag(&create.text().await.unwrap(), "UploadId").unwrap();

    // UploadPartCopy: copy full source as part 1
    let resp = s3_request_with_headers(
        "PUT",
        &format!(
            "{}/dst-upc/dest.bin?partNumber=1&uploadId={}",
            base, upload_id
        ),
        vec![],
        vec![("x-amz-copy-source", "/src-upc/source.bin")],
    )
    .await;
    assert_eq!(resp.status(), 200, "upload_part_copy should return 200");
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("<CopyPartResult>"),
        "response should be CopyPartResult XML, got: {}",
        body
    );
    let etag = extract_xml_tag(&body, "ETag").unwrap();
    assert!(
        etag.starts_with('"') && etag.ends_with('"'),
        "ETag should be quoted"
    );

    // Complete the multipart upload
    let complete_xml = format!(
        "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{}</ETag></Part></CompleteMultipartUpload>",
        etag
    );
    let complete = s3_request(
        "POST",
        &format!("{}/dst-upc/dest.bin?uploadId={}", base, upload_id),
        complete_xml.into_bytes(),
    )
    .await;
    assert_eq!(complete.status(), 200);

    // Verify content matches source
    let get = s3_request("GET", &format!("{}/dst-upc/dest.bin", base), vec![]).await;
    assert_eq!(get.status(), 200);
    assert_eq!(get.bytes().await.unwrap().as_ref(), src_data.as_slice());
}

// UploadPartCopy: copy a byte range from source object as a multipart part
#[tokio::test]
async fn test_upload_part_copy_range() {
    let base = start_server().await;

    // Create source with known content
    s3_request("PUT", &format!("{}/src-upcr", base), vec![]).await;
    // part1: 5 MiB of 'A', part2: 1 KiB of 'B'
    let part1: Vec<u8> = vec![b'A'; 5 * 1024 * 1024];
    let part2: Vec<u8> = vec![b'B'; 1024];
    let mut src_data = part1.clone();
    src_data.extend_from_slice(&part2);
    s3_request(
        "PUT",
        &format!("{}/src-upcr/source.bin", base),
        src_data.clone(),
    )
    .await;

    // Create destination and start multipart upload
    s3_request("PUT", &format!("{}/dst-upcr", base), vec![]).await;
    let create = s3_request(
        "POST",
        &format!("{}/dst-upcr/dest.bin?uploads=", base),
        vec![],
    )
    .await;
    let upload_id = extract_xml_tag(&create.text().await.unwrap(), "UploadId").unwrap();

    // Part 1: bytes 0 to (5MiB - 1)
    let r1 = s3_request_with_headers(
        "PUT",
        &format!(
            "{}/dst-upcr/dest.bin?partNumber=1&uploadId={}",
            base, upload_id
        ),
        vec![],
        vec![
            ("x-amz-copy-source", "/src-upcr/source.bin"),
            (
                "x-amz-copy-source-range",
                &format!("bytes=0-{}", 5 * 1024 * 1024 - 1),
            ),
        ],
    )
    .await;
    assert_eq!(r1.status(), 200);
    let body1 = r1.text().await.unwrap();
    assert!(body1.contains("<CopyPartResult>"));
    let e1 = extract_xml_tag(&body1, "ETag").unwrap();

    // Part 2: remaining bytes
    let r2 = s3_request_with_headers(
        "PUT",
        &format!(
            "{}/dst-upcr/dest.bin?partNumber=2&uploadId={}",
            base, upload_id
        ),
        vec![],
        vec![
            ("x-amz-copy-source", "/src-upcr/source.bin"),
            (
                "x-amz-copy-source-range",
                &format!("bytes={}-{}", 5 * 1024 * 1024, src_data.len() - 1),
            ),
        ],
    )
    .await;
    assert_eq!(r2.status(), 200);
    let body2 = r2.text().await.unwrap();
    assert!(body2.contains("<CopyPartResult>"));
    let e2 = extract_xml_tag(&body2, "ETag").unwrap();

    // Complete
    let complete_xml = format!(
        "<CompleteMultipartUpload>\
            <Part><PartNumber>1</PartNumber><ETag>{}</ETag></Part>\
            <Part><PartNumber>2</PartNumber><ETag>{}</ETag></Part>\
        </CompleteMultipartUpload>",
        e1, e2
    );
    let complete = s3_request(
        "POST",
        &format!("{}/dst-upcr/dest.bin?uploadId={}", base, upload_id),
        complete_xml.into_bytes(),
    )
    .await;
    assert_eq!(complete.status(), 200);

    // Verify reconstructed content matches original source
    let get = s3_request("GET", &format!("{}/dst-upcr/dest.bin", base), vec![]).await;
    assert_eq!(get.status(), 200);
    assert_eq!(get.bytes().await.unwrap().as_ref(), src_data.as_slice());
}

// ---- CORS API tests ----

const CORS_XML_WILDCARD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CORSConfiguration>
  <CORSRule>
    <AllowedOrigin>*</AllowedOrigin>
    <AllowedMethod>GET</AllowedMethod>
    <AllowedMethod>PUT</AllowedMethod>
    <AllowedHeader>*</AllowedHeader>
    <MaxAgeSeconds>3600</MaxAgeSeconds>
  </CORSRule>
</CORSConfiguration>"#;

const CORS_XML_EXACT_ORIGIN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CORSConfiguration>
  <CORSRule>
    <AllowedOrigin>http://example.com</AllowedOrigin>
    <AllowedMethod>GET</AllowedMethod>
  </CORSRule>
</CORSConfiguration>"#;

#[tokio::test]
async fn test_put_get_delete_bucket_cors() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/cors-bucket", base), vec![]).await;

    // GetBucketCors on bucket with no CORS → 404 NoSuchCORSConfiguration
    let resp = s3_request("GET", &format!("{}/cors-bucket?cors", base), vec![]).await;
    assert_eq!(resp.status(), 404);
    let body = resp.text().await.unwrap();
    assert!(body.contains("NoSuchCORSConfiguration"));

    // PutBucketCors
    let resp = s3_request(
        "PUT",
        &format!("{}/cors-bucket?cors", base),
        CORS_XML_WILDCARD.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 200);

    // GetBucketCors → should return config
    let resp = s3_request("GET", &format!("{}/cors-bucket?cors", base), vec![]).await;
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("CORSConfiguration"));
    assert!(body.contains("AllowedMethod"));
    assert!(body.contains("GET"));

    // DeleteBucketCors
    let resp = s3_request("DELETE", &format!("{}/cors-bucket?cors", base), vec![]).await;
    assert_eq!(resp.status(), 204);

    // GetBucketCors after delete → 404 again
    let resp = s3_request("GET", &format!("{}/cors-bucket?cors", base), vec![]).await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_put_cors_invalid_method() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/cors-invalid", base), vec![]).await;

    let bad_cors = r#"<?xml version="1.0" encoding="UTF-8"?>
<CORSConfiguration>
  <CORSRule>
    <AllowedOrigin>*</AllowedOrigin>
    <AllowedMethod>PATCH</AllowedMethod>
  </CORSRule>
</CORSConfiguration>"#;

    let resp = s3_request(
        "PUT",
        &format!("{}/cors-invalid?cors", base),
        bad_cors.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body = resp.text().await.unwrap();
    assert!(body.contains("InvalidArgument"));
}

#[tokio::test]
async fn test_cors_preflight_allowed() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/preflight-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/preflight-bucket?cors", base),
        CORS_XML_WILDCARD.as_bytes().to_vec(),
    )
    .await;

    // OPTIONS preflight — should return 200 with CORS headers
    let resp = client()
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/preflight-bucket/file.txt", base),
        )
        .header("Origin", "http://example.com")
        .header("Access-Control-Request-Method", "GET")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let headers = resp.headers();
    assert!(headers.contains_key("access-control-allow-origin"));
    assert!(headers.contains_key("access-control-allow-methods"));
}

#[tokio::test]
async fn test_cors_preflight_no_config_returns_403() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/no-cors-bucket", base), vec![]).await;

    let resp = client()
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/no-cors-bucket/file.txt", base),
        )
        .header("Origin", "http://example.com")
        .header("Access-Control-Request-Method", "GET")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn test_cors_preflight_unmatched_origin_returns_403() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/exact-origin-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/exact-origin-bucket?cors", base),
        CORS_XML_EXACT_ORIGIN.as_bytes().to_vec(),
    )
    .await;

    let resp = client()
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/exact-origin-bucket/file.txt", base),
        )
        .header("Origin", "http://other.com")
        .header("Access-Control-Request-Method", "GET")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn test_cors_normal_request_gets_headers() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/cors-normal-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/cors-normal-bucket/obj.txt", base),
        b"hello".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/cors-normal-bucket?cors", base),
        CORS_XML_WILDCARD.as_bytes().to_vec(),
    )
    .await;

    let resp = s3_request_with_headers(
        "GET",
        &format!("{}/cors-normal-bucket/obj.txt", base),
        vec![],
        vec![("origin", "http://example.com")],
    )
    .await;

    assert_eq!(resp.status(), 200);
    let headers = resp.headers();
    assert!(headers.contains_key("access-control-allow-origin"));
    assert_eq!(
        headers
            .get("access-control-allow-origin")
            .unwrap()
            .to_str()
            .unwrap(),
        "http://example.com"
    );
}

#[tokio::test]
async fn test_cors_no_origin_no_cors_headers() {
    let base = start_server().await;
    s3_request("PUT", &format!("{}/cors-noorigin-bucket", base), vec![]).await;
    s3_request(
        "PUT",
        &format!("{}/cors-noorigin-bucket/obj.txt", base),
        b"hello".to_vec(),
    )
    .await;
    s3_request(
        "PUT",
        &format!("{}/cors-noorigin-bucket?cors", base),
        CORS_XML_WILDCARD.as_bytes().to_vec(),
    )
    .await;

    let resp = s3_request(
        "GET",
        &format!("{}/cors-noorigin-bucket/obj.txt", base),
        vec![],
    )
    .await;

    assert_eq!(resp.status(), 200);
    assert!(!resp.headers().contains_key("access-control-allow-origin"));
}

#[tokio::test]
async fn test_list_objects_page_db_pagination() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().to_str().unwrap();
    let (postgres, database_url) = start_postgres().await;
    let storage = create_storage(data_dir, &database_url).await;

    storage
        .create_bucket(&maxio::storage::BucketMeta {
            name: "page-bucket".to_string(),
            created_at: "2026-06-07T00:00:00.000Z".to_string(),
            versioning: false,
            cors_rules: None,
            owner_id: maxio::iam::ROOT_CANONICAL_ID.to_string(),
            owner_display_name: maxio::iam::ROOT_DISPLAY_NAME.to_string(),
            acl: Some(maxio::iam::Acl::private(
                maxio::iam::ROOT_CANONICAL_ID,
                maxio::iam::ROOT_DISPLAY_NAME,
            )),
            policy: None,
            public_read: false,
            public_list: false,
        })
        .await
        .unwrap();

    for key in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"] {
        storage
            .put_object(
                "page-bucket",
                key,
                "text/plain",
                Box::pin(std::io::Cursor::new(b"x")),
                None,
            )
            .await
            .unwrap();
    }

    let page1 = storage
        .list_objects_page("page-bucket", "", None, 2, None)
        .await
        .unwrap();
    assert_eq!(page1.objects.len(), 2);
    assert!(page1.is_truncated);
    assert_eq!(page1.next_continuation.as_deref(), Some("b.txt"));

    let page2 = storage
        .list_objects_page(
            "page-bucket",
            "",
            page1.next_continuation.as_deref(),
            2,
            None,
        )
        .await
        .unwrap();
    assert_eq!(page2.objects.len(), 2);
    assert!(page2.is_truncated);
    assert_eq!(page2.next_continuation.as_deref(), Some("d.txt"));

    let page3 = storage
        .list_objects_page(
            "page-bucket",
            "",
            page2.next_continuation.as_deref(),
            2,
            None,
        )
        .await
        .unwrap();
    assert_eq!(page3.objects.len(), 1);
    assert!(!page3.is_truncated);
    let _postgres = postgres;
}

#[tokio::test]
async fn test_put_rollback_when_publish_fails() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().to_str().unwrap();
    let (postgres, database_url) = start_postgres().await;
    let storage = create_storage(data_dir, &database_url).await;

    storage
        .create_bucket(&maxio::storage::BucketMeta {
            name: "rollback-bucket".to_string(),
            created_at: "2026-06-07T00:00:00.000Z".to_string(),
            versioning: false,
            cors_rules: None,
            owner_id: maxio::iam::ROOT_CANONICAL_ID.to_string(),
            owner_display_name: maxio::iam::ROOT_DISPLAY_NAME.to_string(),
            acl: Some(maxio::iam::Acl::private(
                maxio::iam::ROOT_CANONICAL_ID,
                maxio::iam::ROOT_DISPLAY_NAME,
            )),
            policy: None,
            public_read: false,
            public_list: false,
        })
        .await
        .unwrap();

    // Block publish by occupying the final object path with a directory.
    let blocked = tmp
        .path()
        .join("buckets")
        .join("rollback-bucket")
        .join("blocked.txt");
    tokio::fs::create_dir_all(&blocked).await.unwrap();

    let put_err = storage
        .put_object(
            "rollback-bucket",
            "blocked.txt",
            "text/plain",
            Box::pin(std::io::Cursor::new(b"data")),
            None,
        )
        .await;
    assert!(matches!(put_err, Err(maxio::storage::StorageError::Io(_))));

    assert!(
        storage
            .head_object("rollback-bucket", "blocked.txt")
            .await
            .is_err()
    );
    let _postgres = postgres;
}

#[tokio::test]
async fn test_housekeeping_removes_stale_multipart_uploads() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().to_str().unwrap();
    let (postgres, database_url) = start_postgres().await;
    let storage = create_storage(data_dir, &database_url).await;

    storage
        .create_bucket(&maxio::storage::BucketMeta {
            name: "mp-bucket".to_string(),
            created_at: "2026-06-07T00:00:00.000Z".to_string(),
            versioning: false,
            cors_rules: None,
            owner_id: maxio::iam::ROOT_CANONICAL_ID.to_string(),
            owner_display_name: maxio::iam::ROOT_DISPLAY_NAME.to_string(),
            acl: Some(maxio::iam::Acl::private(
                maxio::iam::ROOT_CANONICAL_ID,
                maxio::iam::ROOT_DISPLAY_NAME,
            )),
            policy: None,
            public_read: false,
            public_list: false,
        })
        .await
        .unwrap();

    let upload = storage
        .create_multipart_upload("mp-bucket", "stale.txt", "text/plain", None)
        .await
        .unwrap();
    let upload_id = upload.upload_id.clone();

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let (removed, _) = storage
        .housekeeping_sweep(chrono::Duration::milliseconds(10))
        .await;
    assert!(removed >= 1);

    assert!(
        storage
            .list_multipart_uploads("mp-bucket", None)
            .await
            .unwrap()
            .iter()
            .all(|u| u.upload_id != upload_id)
    );
    let _postgres = postgres;
}

#[tokio::test]
async fn test_orphan_meta_scan_and_delete() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().to_str().unwrap().to_string();
    let (postgres, database_url) = start_postgres().await;
    let storage = create_storage(&data_dir, &database_url).await;

    storage
        .create_bucket(&maxio::storage::BucketMeta {
            name: "orphan-bucket".to_string(),
            created_at: "2026-06-08T00:00:00.000Z".to_string(),
            versioning: false,
            cors_rules: None,
            owner_id: maxio::iam::ROOT_CANONICAL_ID.to_string(),
            owner_display_name: maxio::iam::ROOT_DISPLAY_NAME.to_string(),
            acl: Some(maxio::iam::Acl::private(
                maxio::iam::ROOT_CANONICAL_ID,
                maxio::iam::ROOT_DISPLAY_NAME,
            )),
            policy: None,
            public_read: false,
            public_list: false,
        })
        .await
        .unwrap();

    let body: maxio::storage::ByteStream = Box::pin(std::io::Cursor::new(b"orphan test".to_vec()));
    storage
        .put_object("orphan-bucket", "missing.txt", "text/plain", body, None)
        .await
        .unwrap();

    tokio::fs::remove_file(
        tmp.path()
            .join("buckets")
            .join("orphan-bucket")
            .join("missing.txt"),
    )
    .await
    .unwrap();

    let pool = maxio::db::create_pool(&database_url).await.unwrap();
    let blobs = BlobStorage::new(&data_dir).await.unwrap();
    let orphans = maxio::storage::orphans::scan_orphaned_meta(Arc::new(pool.clone()), &blobs, None)
        .await
        .unwrap();
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].bucket, "orphan-bucket");
    assert_eq!(orphans[0].key, "missing.txt");

    let meta: Arc<dyn MetadataStore> = Arc::new(PgMetadataStore::new(Arc::new(pool)));
    let removed = maxio::storage::orphans::delete_orphaned_meta(meta.as_ref(), &orphans)
        .await
        .unwrap();
    assert_eq!(removed, 1);

    let blobs = BlobStorage::new(&data_dir).await.unwrap();
    let pool = maxio::db::create_pool(&database_url).await.unwrap();
    let orphans = maxio::storage::orphans::scan_orphaned_meta(Arc::new(pool), &blobs, None)
        .await
        .unwrap();
    assert!(orphans.is_empty());

    let _postgres = postgres;
}
