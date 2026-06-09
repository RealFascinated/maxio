use std::sync::Arc;

use crate::storage::blob::BlobStorage;
use crate::storage::{
    BucketMeta, ByteStream, MetadataStore, ObjectStorage, PgMetadataStore, Storage,
};
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

use super::objects::{
    normalize_presign_host, parent_folder_prefix_for_deleted_object,
    preserve_empty_parent_folder_after_object_delete,
};

async fn test_storage(
    data_dir: &str,
) -> Result<(Arc<dyn Storage>, testcontainers::ContainerAsync<Postgres>), Box<dyn std::error::Error>>
{
    let postgres = Postgres::default().with_tag("18-alpine").start().await?;
    let port = postgres.get_host_port_ipv4(5432).await?;
    let database_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    crate::db::run_migrations(&database_url).await?;
    let pool = crate::db::create_pool(&database_url, Default::default()).await?;
    let meta: Arc<dyn MetadataStore> = Arc::new(PgMetadataStore::new(
        Arc::new(pool),
        crate::config::MemoryCacheLimits::default(),
    ));
    let blobs = BlobStorage::new(data_dir).await?;
    Ok((Arc::new(ObjectStorage::new(blobs, meta)), postgres))
}

async fn create_test_bucket(storage: &dyn Storage, bucket: &str) {
    storage
        .create_bucket(&BucketMeta {
            name: bucket.to_string(),
            created_at: "2026-05-18T00:00:00.000Z".to_string(),
            versioning: false,
            cors_rules: None,
            owner_id: crate::iam::ROOT_CANONICAL_ID.to_string(),
            owner_display_name: crate::iam::ROOT_DISPLAY_NAME.to_string(),
            acl: Some(crate::iam::Acl::private(
                crate::iam::ROOT_CANONICAL_ID,
                crate::iam::ROOT_DISPLAY_NAME,
            )),
            policy: None,
            public_read: false,
            public_list: false,
        })
        .await
        .unwrap();
}

fn bytes(data: &'static [u8]) -> ByteStream {
    Box::pin(data)
}

#[test]
fn normalize_presign_host_strips_default_ports() {
    assert_eq!(
        normalize_presign_host("s3.example.com:443", "https"),
        "s3.example.com"
    );
    assert_eq!(
        normalize_presign_host("s3.example.com:80", "http"),
        "s3.example.com"
    );
    assert_eq!(
        normalize_presign_host("s3.example.com:9000", "https"),
        "s3.example.com:9000"
    );
}

#[test]
fn parent_folder_prefix_ignores_root_files_and_folder_markers() {
    assert_eq!(parent_folder_prefix_for_deleted_object("file.txt"), None);
    assert_eq!(parent_folder_prefix_for_deleted_object("folder/"), None);
    assert_eq!(
        parent_folder_prefix_for_deleted_object("folder/file.txt"),
        Some("folder/".to_string())
    );
    assert_eq!(
        parent_folder_prefix_for_deleted_object("a/b/file.txt"),
        Some("a/b/".to_string())
    );
}

#[tokio::test]
async fn deleting_last_console_file_preserves_parent_folder_marker() {
    let temp = tempfile::tempdir().unwrap();
    let (storage, _pg) = test_storage(temp.path().to_str().unwrap()).await.unwrap();
    create_test_bucket(storage.as_ref(), "bucket").await;

    storage
        .put_object(
            "bucket",
            "folder/file.txt",
            "text/plain",
            bytes(b"hello"),
            None,
        )
        .await
        .unwrap();

    storage
        .delete_object("bucket", "folder/file.txt")
        .await
        .unwrap();
    preserve_empty_parent_folder_after_object_delete(storage.as_ref(), "bucket", "folder/file.txt")
        .await
        .unwrap();

    let objects = crate::storage::list_objects_all(storage.as_ref(), "bucket", "folder/")
        .await
        .unwrap();
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].key, "folder/");
    assert_eq!(objects[0].content_type, "application/x-directory");
}

#[tokio::test]
async fn deleting_folder_marker_does_not_recreate_it() {
    let temp = tempfile::tempdir().unwrap();
    let (storage, _pg) = test_storage(temp.path().to_str().unwrap()).await.unwrap();
    create_test_bucket(storage.as_ref(), "bucket").await;

    storage
        .put_object(
            "bucket",
            "folder/",
            "application/x-directory",
            Box::pin(tokio::io::empty()),
            None,
        )
        .await
        .unwrap();

    storage.delete_object("bucket", "folder/").await.unwrap();
    preserve_empty_parent_folder_after_object_delete(storage.as_ref(), "bucket", "folder/")
        .await
        .unwrap();

    let objects = crate::storage::list_objects_all(storage.as_ref(), "bucket", "folder/")
        .await
        .unwrap();
    assert!(objects.is_empty());
}
