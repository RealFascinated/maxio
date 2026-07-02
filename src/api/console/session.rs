use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Instant;

use axum::http::HeaderMap;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::auth::signature_v4;
use crate::iam::types::KeyStatus;
use crate::server::AppState;

type HmacSha256 = Hmac<Sha256>;

pub(crate) const COOKIE_NAME: &str = "maxio_session";
pub(crate) const TOKEN_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60; // 7 days

const RATE_LIMIT_MAX: u32 = 10;
const RATE_LIMIT_WINDOW_SECS: u64 = 300; // 5 minutes

struct Bucket {
    count: u32,
    window_start: Instant,
}

pub struct LoginRateLimiter {
    buckets: std::sync::Mutex<HashMap<String, Bucket>>,
}

pub struct RevokedSessions {
    tokens: Mutex<HashSet<String>>,
}

impl RevokedSessions {
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(HashSet::new()),
        }
    }

    pub fn revoke(&self, token: &str) {
        if !token.is_empty() {
            self.tokens.lock().unwrap().insert(token.to_string());
        }
    }

    pub fn is_revoked(&self, token: &str) -> bool {
        self.tokens.lock().unwrap().contains(token)
    }
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

pub(crate) fn extract_client_ip(headers: &HeaderMap, addr: &SocketAddr) -> String {
    let _ = headers;
    // Public console: do not trust spoofable X-Forwarded-For unless/until a
    // trusted-proxy allowlist is configured. Use the connected peer IP.
    addr.ip().to_string()
}

pub(crate) fn generate_token(access_key_id: &str, secret_key: &str, issued_at: i64) -> String {
    let issued_hex = format!("{:x}", issued_at);
    let mut mac =
        HmacSha256::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(format!("{}:{}", access_key_id, issued_hex).as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    format!("{}.{}.{}", issued_hex, access_key_id, sig)
}

fn verify_token(token: &str, secret_key: &str) -> Option<String> {
    let mut parts = token.split('.');
    let issued_hex = parts.next()?;
    let access_key_id = parts.next()?;
    let signature = parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    let Ok(issued_at) = i64::from_str_radix(issued_hex, 16) else {
        return None;
    };

    let now = chrono::Utc::now().timestamp();
    if now - issued_at > TOKEN_MAX_AGE_SECS || issued_at > now + 60 {
        return None;
    }

    let mut mac =
        HmacSha256::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(format!("{}:{}", access_key_id, issued_hex).as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());

    if signature_v4::constant_time_eq(signature.as_bytes(), expected.as_bytes()) {
        Some(access_key_id.to_string())
    } else {
        None
    }
}

pub(crate) async fn resolve_session_access_key(token: &str, state: &AppState) -> Option<String> {
    if token.is_empty() || state.revoked_sessions.is_revoked(token) {
        return None;
    }
    let access_key_id = verify_token(token, &state.config.secret_key)?;
    session_from_access_key(state, &access_key_id).await?;
    Some(access_key_id)
}

pub(crate) async fn session_from_access_key(
    state: &AppState,
    access_key_id: &str,
) -> Option<ConsoleSession> {
    if signature_v4::constant_time_eq(access_key_id.as_bytes(), state.config.access_key.as_bytes())
    {
        return Some(ConsoleSession::root(state.config.access_key.clone()));
    }

    let (user, key) = state.user_store.lookup_by_access_key(access_key_id).await?;
    if key.status != KeyStatus::Active {
        return None;
    }

    Some(ConsoleSession {
        username: user.username,
        is_root: false,
        user_id: user.user_id,
        access_key_id: access_key_id.to_string(),
    })
}

pub(crate) async fn session_signing_credentials(
    state: &AppState,
    session: &ConsoleSession,
) -> Option<(String, String)> {
    if session.is_root {
        return Some((
            state.config.access_key.clone(),
            state.config.secret_key.clone(),
        ));
    }

    let (_, key) = state
        .user_store
        .lookup_by_access_key(&session.access_key_id)
        .await?;
    if key.status != KeyStatus::Active {
        return None;
    }

    Some((key.access_key_id, key.secret_access_key))
}

#[derive(Clone, Debug)]
pub(crate) struct ConsoleSession {
    pub username: String,
    pub is_root: bool,
    pub user_id: String,
    pub access_key_id: String,
}

impl ConsoleSession {
    pub(crate) fn root(access_key_id: String) -> Self {
        Self {
            username: crate::iam::ROOT_USERNAME.to_string(),
            is_root: true,
            user_id: crate::iam::ROOT_CANONICAL_ID.to_string(),
            access_key_id,
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

pub(crate) fn extract_cookie(headers: &HeaderMap) -> Option<String> {
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

pub(crate) fn make_cookie(value: &str, max_age: i64, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };

    format!(
        "{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}",
        COOKIE_NAME, value, max_age, secure_flag
    )
}

pub(crate) fn cookies_require_https(state: &AppState) -> bool {
    state.config.secure_cookies && !state.config.allow_insecure_dev
}
