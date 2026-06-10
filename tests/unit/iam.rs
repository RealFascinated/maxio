use maxio::iam::acl::{Acl, AclPermission, acl_allows};
use maxio::iam::authz::{bucket_arn, object_arn};
use maxio::iam::policy::{AuthDecision, evaluate, parse_policy_document};
use maxio::iam::principal::Principal;
use maxio::iam::types::{PolicyDocumentRaw, StatementRaw};

#[test]
fn arn_format() {
    assert_eq!(bucket_arn("my-bucket"), "arn:aws:s3:::my-bucket");
    assert_eq!(
        object_arn("my-bucket", "path/file.txt"),
        "arn:aws:s3:::my-bucket/path/file.txt"
    );
}

#[test]
fn private_acl_only_owner() {
    let acl = Acl::private("owner1", "owner");
    let owner = Principal {
        username: "alice".into(),
        user_id: "owner1".into(),
        display_name: "alice".into(),
        canonical_id: "owner1".into(),
        is_root: false,
        is_anonymous: false,
    };
    assert!(acl_allows(&acl, &owner, AclPermission::Read));
    let other = Principal {
        username: "bob".into(),
        user_id: "other".into(),
        display_name: "bob".into(),
        canonical_id: "other".into(),
        is_root: false,
        is_anonymous: false,
    };
    assert!(!acl_allows(&acl, &other, AclPermission::Read));
}

#[test]
fn allow_read_object() {
    let doc = parse_policy_document(&PolicyDocumentRaw {
        version: "2012-10-17".into(),
        statement: vec![StatementRaw {
            sid: None,
            effect: "Allow".to_string(),
            action: vec!["s3:GetObject".to_string()],
            resource: vec!["arn:aws:s3:::my-bucket/*".to_string()],
            principal: None,
            condition: None,
        }],
    })
    .unwrap();
    let p = Principal {
        username: "alice".into(),
        user_id: "AIDA123".into(),
        display_name: "alice".into(),
        canonical_id: "AIDA123".into(),
        is_root: false,
        is_anonymous: false,
    };
    assert_eq!(
        evaluate(
            &p,
            "s3:GetObject",
            "arn:aws:s3:::my-bucket/file.txt",
            &[doc],
            None
        ),
        AuthDecision::Allow
    );
}

#[test]
fn deny_overrides_allow() {
    let allow = parse_policy_document(&PolicyDocumentRaw {
        version: "2012-10-17".into(),
        statement: vec![StatementRaw {
            sid: None,
            effect: "Allow".to_string(),
            action: vec!["s3:*".to_string()],
            resource: vec!["*".to_string()],
            principal: None,
            condition: None,
        }],
    })
    .unwrap();
    let deny = parse_policy_document(&PolicyDocumentRaw {
        version: "2012-10-17".into(),
        statement: vec![StatementRaw {
            sid: None,
            effect: "Deny".to_string(),
            action: vec!["s3:DeleteObject".to_string()],
            resource: vec!["arn:aws:s3:::secret/*".to_string()],
            principal: None,
            condition: None,
        }],
    })
    .unwrap();
    let p = Principal {
        username: "alice".into(),
        user_id: "AIDA123".into(),
        display_name: "alice".into(),
        canonical_id: "AIDA123".into(),
        is_root: false,
        is_anonymous: false,
    };
    assert_eq!(
        evaluate(
            &p,
            "s3:DeleteObject",
            "arn:aws:s3:::secret/key",
            &[allow, deny],
            None
        ),
        AuthDecision::Deny
    );
}
