use std::sync::Arc;

use crate::common::*;
use maxio::storage::blob::BlobStorage;
use maxio::storage::{MetadataStore, PgMetadataStore};
use tempfile::TempDir;

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
async fn test_list_objects_delimited_page_skips_dense_folders() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().to_str().unwrap();
    let (postgres, database_url) = start_postgres().await;
    let storage = create_storage(data_dir, &database_url).await;

    storage
        .create_bucket(&maxio::storage::BucketMeta {
            name: "dense-bucket".to_string(),
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

    for key in ["a-file.txt", "z-file.txt"] {
        storage
            .put_object(
                "dense-bucket",
                key,
                "text/plain",
                Box::pin(std::io::Cursor::new(b"x")),
                None,
            )
            .await
            .unwrap();
    }

    for i in 0..500 {
        storage
            .put_object(
                "dense-bucket",
                &format!("big-folder/item-{i:03}.txt"),
                "text/plain",
                Box::pin(std::io::Cursor::new(b"x")),
                None,
            )
            .await
            .unwrap();
    }

    for key in ["other-folder/", "third-folder/"] {
        storage
            .put_object(
                "dense-bucket",
                key,
                "application/x-directory",
                Box::pin(tokio::io::empty()),
                None,
            )
            .await
            .unwrap();
    }

    let page = storage
        .list_objects_delimited_page("dense-bucket", "", "/", None, 200, None)
        .await
        .unwrap();

    assert_eq!(
        page.prefixes,
        vec![
            "big-folder/".to_string(),
            "other-folder/".to_string(),
            "third-folder/".to_string(),
        ]
    );
    assert_eq!(
        page.files
            .iter()
            .map(|o| o.key.as_str())
            .collect::<Vec<_>>(),
        vec!["a-file.txt", "z-file.txt"]
    );
    assert!(page.next_continuation.is_none());

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

    let pool = maxio::db::create_pool(&database_url, Default::default())
        .await
        .unwrap();
    let blobs = BlobStorage::new(&data_dir).await.unwrap();
    let orphans = maxio::storage::orphans::scan_orphaned_meta(Arc::new(pool.clone()), &blobs, None)
        .await
        .unwrap();
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].bucket, "orphan-bucket");
    assert_eq!(orphans[0].key, "missing.txt");

    let meta: Arc<dyn MetadataStore> = Arc::new(PgMetadataStore::new(
        Arc::new(pool),
        maxio::config::MemoryCacheLimits::default(),
    ));
    let removed = maxio::storage::orphans::delete_orphaned_meta(meta.as_ref(), &orphans)
        .await
        .unwrap();
    assert_eq!(removed, 1);

    let blobs = BlobStorage::new(&data_dir).await.unwrap();
    let pool = maxio::db::create_pool(&database_url, Default::default())
        .await
        .unwrap();
    let orphans = maxio::storage::orphans::scan_orphaned_meta(Arc::new(pool), &blobs, None)
        .await
        .unwrap();
    assert!(orphans.is_empty());

    let _postgres = postgres;
}
