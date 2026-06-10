use axum::{body::Body, http::HeaderMap, response::Response};

/// Convert ISO 8601 timestamp to HTTP date (RFC 7231) for Last-Modified header.
pub(super) fn to_http_date(iso: &str) -> String {
    chrono::DateTime::parse_from_str(iso, "%Y-%m-%dT%H:%M:%S%.3fZ")
        .or_else(|_| chrono::DateTime::parse_from_rfc3339(iso))
        .map(|dt| dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
        .unwrap_or_else(|_| iso.to_string())
}

pub enum ConditionalResult {
    NotModified,
    PreconditionFailed,
}

/// Returns true if `header_value` (the value of If-Match or If-None-Match)
/// matches `object_etag`. Handles `*`, quoted/unquoted ETags, and
/// comma-separated lists.
pub fn etag_matches(header_value: &str, object_etag: &str) -> bool {
    let value = header_value.trim();
    if value == "*" {
        return true;
    }
    let obj = object_etag.trim_matches('"');
    for part in value.split(',') {
        if part.trim().trim_matches('"') == obj {
            return true;
        }
    }
    false
}

/// Parse an RFC 7231 HTTP-date string (e.g. "Sun, 06 Nov 1994 08:49:37 GMT").
/// Returns None on invalid input — callers silently skip the condition.
fn parse_http_date(s: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc2822(s).ok()
}

/// Parse the ISO 8601 timestamp stored in ObjectMeta.last_modified.
fn parse_object_date(iso: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_str(iso, "%Y-%m-%dT%H:%M:%S%.3fZ")
        .or_else(|_| chrono::DateTime::parse_from_rfc3339(iso))
        .ok()
}

/// Evaluate conditional request headers against object metadata, following
/// S3/RFC 7232 precedence rules. Returns Some(result) if the request should
/// be short-circuited, or None if it should proceed normally.
pub fn check_conditions(
    headers: &HeaderMap,
    meta: &crate::storage::ObjectMeta,
) -> Option<ConditionalResult> {
    let if_match = headers.get("if-match").and_then(|v| v.to_str().ok());
    let if_none_match = headers.get("if-none-match").and_then(|v| v.to_str().ok());
    let if_modified_since = headers
        .get("if-modified-since")
        .and_then(|v| v.to_str().ok());
    let if_unmodified_since = headers
        .get("if-unmodified-since")
        .and_then(|v| v.to_str().ok());

    // Step 1: If-Match
    if let Some(value) = if_match {
        if !etag_matches(value, &meta.etag) {
            return Some(ConditionalResult::PreconditionFailed);
        }
        // ETag matched — If-Unmodified-Since is skipped per RFC 7232 §6
    } else if let Some(value) = if_unmodified_since {
        // Step 2: If-Unmodified-Since (only when If-Match is absent)
        if let (Some(threshold), Some(obj_date)) = (
            parse_http_date(value),
            parse_object_date(&meta.last_modified),
        ) {
            if obj_date > threshold {
                return Some(ConditionalResult::PreconditionFailed);
            }
        }
    }

    // Step 3: If-None-Match
    if let Some(value) = if_none_match {
        if etag_matches(value, &meta.etag) {
            return Some(ConditionalResult::NotModified);
        }
        // Present but no match — If-Modified-Since is skipped per RFC 7232 §6
    } else if let Some(value) = if_modified_since {
        // Step 4: If-Modified-Since (only when If-None-Match is absent)
        if let (Some(threshold), Some(obj_date)) = (
            parse_http_date(value),
            parse_object_date(&meta.last_modified),
        ) {
            if obj_date <= threshold {
                return Some(ConditionalResult::NotModified);
            }
        }
    }

    None
}

pub(super) fn not_modified_response(meta: &crate::storage::ObjectMeta) -> Response<Body> {
    Response::builder()
        .status(axum::http::StatusCode::NOT_MODIFIED)
        .header("ETag", &meta.etag)
        .header("Last-Modified", to_http_date(&meta.last_modified))
        .body(Body::empty())
        .unwrap()
}

/// Parse an HTTP Range header value into (start, end_inclusive) byte positions.
/// Returns Ok(Some((start, end))) for valid ranges, Ok(None) for unparseable/ignored,
/// Err(()) for syntactically valid but unsatisfiable ranges.
pub(super) fn parse_range(header: &str, file_size: u64) -> Result<Option<(u64, u64)>, ()> {
    let header = header.trim();
    let spec = match header.strip_prefix("bytes=") {
        Some(s) => s.trim(),
        None => return Ok(None),
    };
    // S3 doesn't support multi-range
    if spec.contains(',') {
        return Ok(None);
    }
    let (start_str, end_str) = match spec.split_once('-') {
        Some(parts) => parts,
        None => return Ok(None),
    };

    if file_size == 0 {
        return Err(());
    }

    if start_str.is_empty() {
        // Suffix: bytes=-N
        let suffix: u64 = end_str.parse().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        let start = file_size.saturating_sub(suffix);
        Ok(Some((start, file_size - 1)))
    } else if end_str.is_empty() {
        // Open end: bytes=N-
        let start: u64 = start_str.parse().map_err(|_| ())?;
        if start >= file_size {
            return Err(());
        }
        Ok(Some((start, file_size - 1)))
    } else {
        // Explicit: bytes=N-M
        let start: u64 = start_str.parse().map_err(|_| ())?;
        let end: u64 = end_str.parse().map_err(|_| ())?;
        if start >= file_size {
            return Err(());
        }
        let end = end.min(file_size - 1);
        if start > end {
            return Err(());
        }
        Ok(Some((start, end)))
    }
}
