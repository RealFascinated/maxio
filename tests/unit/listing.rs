use maxio::db::repos::{delimited_common_prefix, delimited_direct_file};

#[test]
fn delimited_common_prefix_collapses_nested_keys() {
    assert_eq!(
        delimited_common_prefix("big-folder/item.txt", "", "/"),
        Some("big-folder/".to_string())
    );
    assert_eq!(
        delimited_common_prefix("other-folder/", "", "/"),
        Some("other-folder/".to_string())
    );
    assert_eq!(delimited_common_prefix("nested/", "nested/", "/"), None);
    assert_eq!(
        delimited_common_prefix("nested/file.txt", "nested/", "/"),
        None
    );
}

#[test]
fn delimited_direct_file_matches_only_current_level() {
    assert!(delimited_direct_file("a-file.txt", "", "/"));
    assert!(!delimited_direct_file("folder/a-file.txt", "", "/"));
    assert!(!delimited_direct_file("folder/", "", "/"));
    assert!(delimited_direct_file("nested/file.txt", "nested/", "/"));
}
