use maxio::api::object::{
    ConditionalResult, check_conditions, etag_matches, parse_delete_objects_xml,
};
use maxio::iam::{ROOT_CANONICAL_ID, ROOT_DISPLAY_NAME};
use maxio::storage::{BatchDeleteObject, ObjectMeta};

#[test]
fn parse_delete_objects_xml_reads_version_id() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<Delete>
  <Object><Key>a.txt</Key><VersionId>vid-1</VersionId></Object>
  <Object><Key>b.txt</Key></Object>
</Delete>"#;
    let objects = parse_delete_objects_xml(xml).unwrap();
    assert_eq!(objects.len(), 2);
    assert_eq!(objects[0].key, "a.txt");
    assert_eq!(objects[0].version_id.as_deref(), Some("vid-1"));
    assert_eq!(objects[1].key, "b.txt");
    assert!(objects[1].version_id.is_none());
}

#[test]
fn parse_delete_objects_xml_rejects_bare_keys() {
    let xml = br#"<Delete><Key>only.txt</Key></Delete>"#;
    assert!(parse_delete_objects_xml(xml).is_err());
}

fn make_meta(etag: &str, last_modified: &str) -> ObjectMeta {
    ObjectMeta {
        key: "test.txt".into(),
        size: 42,
        etag: etag.to_string(),
        content_type: "text/plain".into(),
        last_modified: last_modified.to_string(),
        version_id: None,
        is_delete_marker: false,
        checksum_algorithm: None,
        checksum_value: None,
        tags: None,
        part_sizes: None,
        owner_id: ROOT_CANONICAL_ID.to_string(),
        owner_display_name: ROOT_DISPLAY_NAME.to_string(),
        acl: None,
    }
}

fn headers_with(pairs: &[(&str, &str)]) -> axum::http::HeaderMap {
    let mut map = axum::http::HeaderMap::new();
    for (k, v) in pairs {
        map.insert(
            http::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
            http::header::HeaderValue::from_str(v).unwrap(),
        );
    }
    map
}

const ETAG: &str = "\"abc123\"";
const OLD_DATE: &str = "Mon, 01 Jan 2024 00:00:00 GMT";
const FUTURE_DATE: &str = "Thu, 01 Jan 2099 00:00:00 GMT";
const LAST_MODIFIED: &str = "2025-06-01T12:00:00.000Z";

#[test]
fn etag_matches_exact_quoted() {
    assert!(etag_matches("\"abc123\"", "\"abc123\""));
}

#[test]
fn etag_matches_unquoted_header() {
    assert!(etag_matches("abc123", "\"abc123\""));
}

#[test]
fn etag_matches_wildcard() {
    assert!(etag_matches("*", "\"anything\""));
}

#[test]
fn etag_matches_comma_list() {
    assert!(etag_matches("\"aaa\", \"bbb\", \"abc123\"", "\"abc123\""));
}

#[test]
fn etag_no_match() {
    assert!(!etag_matches("\"wrong\"", "\"abc123\""));
}

#[test]
fn if_match_passes_returns_none() {
    let meta = make_meta(ETAG, LAST_MODIFIED);
    let h = headers_with(&[("if-match", ETAG)]);
    assert!(matches!(check_conditions(&h, &meta), None));
}

#[test]
fn if_match_fails_returns_412() {
    let meta = make_meta(ETAG, LAST_MODIFIED);
    let h = headers_with(&[("if-match", "\"wrong\"")]);
    assert!(matches!(
        check_conditions(&h, &meta),
        Some(ConditionalResult::PreconditionFailed)
    ));
}

#[test]
fn if_none_match_hit_returns_304() {
    let meta = make_meta(ETAG, LAST_MODIFIED);
    let h = headers_with(&[("if-none-match", ETAG)]);
    assert!(matches!(
        check_conditions(&h, &meta),
        Some(ConditionalResult::NotModified)
    ));
}

#[test]
fn if_none_match_miss_returns_none() {
    let meta = make_meta(ETAG, LAST_MODIFIED);
    let h = headers_with(&[("if-none-match", "\"other\"")]);
    assert!(matches!(check_conditions(&h, &meta), None));
}

#[test]
fn if_modified_since_not_modified_returns_304() {
    let meta = make_meta(ETAG, LAST_MODIFIED);
    let h = headers_with(&[("if-modified-since", FUTURE_DATE)]);
    assert!(matches!(
        check_conditions(&h, &meta),
        Some(ConditionalResult::NotModified)
    ));
}

#[test]
fn if_modified_since_was_modified_returns_none() {
    let meta = make_meta(ETAG, LAST_MODIFIED);
    let h = headers_with(&[("if-modified-since", OLD_DATE)]);
    assert!(matches!(check_conditions(&h, &meta), None));
}

#[test]
fn if_unmodified_since_unmodified_returns_none() {
    let meta = make_meta(ETAG, LAST_MODIFIED);
    let h = headers_with(&[("if-unmodified-since", FUTURE_DATE)]);
    assert!(matches!(check_conditions(&h, &meta), None));
}

#[test]
fn if_unmodified_since_was_modified_returns_412() {
    let meta = make_meta(ETAG, LAST_MODIFIED);
    let h = headers_with(&[("if-unmodified-since", OLD_DATE)]);
    assert!(matches!(
        check_conditions(&h, &meta),
        Some(ConditionalResult::PreconditionFailed)
    ));
}

#[test]
fn if_match_suppresses_if_unmodified_since() {
    let meta = make_meta(ETAG, LAST_MODIFIED);
    let h = headers_with(&[("if-match", ETAG), ("if-unmodified-since", OLD_DATE)]);
    assert!(matches!(check_conditions(&h, &meta), None));
}

#[test]
fn if_none_match_suppresses_if_modified_since() {
    let meta = make_meta(ETAG, LAST_MODIFIED);
    let h = headers_with(&[
        ("if-none-match", "\"other\""),
        ("if-modified-since", FUTURE_DATE),
    ]);
    assert!(matches!(check_conditions(&h, &meta), None));
}

#[test]
fn invalid_date_silently_ignored() {
    let meta = make_meta(ETAG, LAST_MODIFIED);
    let h = headers_with(&[("if-modified-since", "not-a-date")]);
    assert!(matches!(check_conditions(&h, &meta), None));
}
