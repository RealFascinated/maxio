use maxio::api::cors::{
    console_permissive_cors_rules, cors_has_console_permissive, extract_bucket_from_path,
    find_matching_rule, origin_matches,
};
use maxio::storage::CorsRule;

fn make_rule(origins: &[&str], methods: &[&str]) -> CorsRule {
    CorsRule {
        allowed_origins: origins.iter().map(|s| s.to_string()).collect(),
        allowed_methods: methods.iter().map(|s| s.to_string()).collect(),
        allowed_headers: vec![],
        expose_headers: vec![],
        max_age_seconds: None,
    }
}

#[test]
fn test_extract_bucket_from_path() {
    assert_eq!(extract_bucket_from_path("/my-bucket"), Some("my-bucket"));
    assert_eq!(
        extract_bucket_from_path("/my-bucket/key/path"),
        Some("my-bucket")
    );
    assert_eq!(extract_bucket_from_path("/"), None);
    assert_eq!(extract_bucket_from_path("/api/buckets"), None);
    assert_eq!(extract_bucket_from_path("/ui/index.html"), None);
}

#[test]
fn origin_matches_patterns() {
    assert!(origin_matches("*", "http://example.com"));
    assert!(origin_matches("http://example.com", "http://example.com"));
    assert!(!origin_matches("http://other.com", "http://example.com"));
}

#[test]
fn find_matching_rule_wildcard_origin() {
    let rules = vec![make_rule(&["*"], &["GET", "PUT"])];
    assert!(find_matching_rule(&rules, "http://example.com", "GET").is_some());
    assert!(find_matching_rule(&rules, "http://example.com", "DELETE").is_none());
}

#[test]
fn find_matching_rule_exact_origin() {
    let rules = vec![make_rule(&["http://example.com"], &["GET"])];
    assert!(find_matching_rule(&rules, "http://example.com", "GET").is_some());
    assert!(find_matching_rule(&rules, "http://other.com", "GET").is_none());
}

#[test]
fn find_matching_rule_no_rules() {
    let rules: Vec<CorsRule> = vec![];
    assert!(find_matching_rule(&rules, "http://example.com", "GET").is_none());
}

#[test]
fn console_permissive_cors_detection() {
    assert!(!cors_has_console_permissive(&[]));
    assert!(cors_has_console_permissive(&console_permissive_cors_rules()));
    let mut other = console_permissive_cors_rules();
    other[0].allowed_methods.push("PUT".to_string());
    assert!(!cors_has_console_permissive(&other));
}
