use maxio::auth::signature_v4::{
    constant_time_eq, derive_signing_key, parse_authorization_header, parse_presigned_query,
};

#[test]
fn parse_authorization_header_valid() {
    let header = "AWS4-HMAC-SHA256 Credential=maxioadmin/20250610/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=abc123";
    let parsed = parse_authorization_header(header).unwrap();
    assert_eq!(parsed.access_key, "maxioadmin");
    assert_eq!(parsed.date, "20250610");
    assert_eq!(parsed.region, "us-east-1");
    assert_eq!(parsed.signed_headers, vec!["host", "x-amz-date"]);
    assert_eq!(parsed.signature, "abc123");
}

#[test]
fn parse_authorization_header_compact_no_spaces() {
    let header = "AWS4-HMAC-SHA256 Credential=maxioadmin/20250610/us-east-1/s3/aws4_request,SignedHeaders=host;x-amz-date,Signature=abc123";
    let parsed = parse_authorization_header(header).unwrap();
    assert_eq!(parsed.access_key, "maxioadmin");
    assert_eq!(parsed.signature, "abc123");
}

#[test]
fn parse_authorization_header_rejects_bad_algorithm() {
    assert!(parse_authorization_header("AWS4-HMAC-SHA1 foo").is_err());
}

#[test]
fn parse_presigned_query_extracts_fields() {
    let query = "X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=maxioadmin%2F20250610%2Fus-east-1%2Fs3%2Faws4_request&X-Amz-Date=20250610T120000Z&X-Amz-Expires=3600&X-Amz-SignedHeaders=host&X-Amz-Signature=deadbeef";
    let (parsed, timestamp, expires) = parse_presigned_query(query).unwrap();
    assert_eq!(parsed.access_key, "maxioadmin");
    assert_eq!(timestamp, "20250610T120000Z");
    assert_eq!(expires, 3600);
    assert_eq!(parsed.signature, "deadbeef");
}

#[test]
fn parse_presigned_query_rejects_expires_over_max() {
    let query = "X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=a%2F20250610%2Fus-east-1%2Fs3%2Faws4_request&X-Amz-Date=20250610T120000Z&X-Amz-Expires=604801&X-Amz-SignedHeaders=host&X-Amz-Signature=x";
    assert!(parse_presigned_query(query).is_err());
}

#[test]
fn derive_signing_key_is_deterministic() {
    let k1 = derive_signing_key("secret", "20250610", "us-east-1");
    let k2 = derive_signing_key("secret", "20250610", "us-east-1");
    assert_eq!(k1, k2);
    assert_ne!(k1, derive_signing_key("other", "20250610", "us-east-1"));
}

#[test]
fn constant_time_eq_matches_equal_slices() {
    assert!(constant_time_eq(b"abc", b"abc"));
    assert!(!constant_time_eq(b"abc", b"abd"));
    assert!(!constant_time_eq(b"ab", b"abc"));
}
