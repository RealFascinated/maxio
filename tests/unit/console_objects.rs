use std::sync::Arc;

use maxio::api::console::{
    ObjectGetOp, ObjectGetQuery, folder_delete_stats, normalize_folder_prefix,
    normalize_presign_host, parent_folder_prefix_for_deleted_object,
    preserve_empty_parent_folder_after_object_delete, sanitize_filename,
};
use maxio::config::MemoryCacheLimits;
use maxio::iam::{ROOT_CANONICAL_ID, ROOT_DISPLAY_NAME};
use maxio::storage::blob::BlobStorage;
use maxio::storage::{
    BucketMeta, ByteStream, MetadataStore, ObjectStorage, PgMetadataStore, Storage,
    batch_delete_keys,
};
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

async fn test_storage(
    data_dir: &str,
) -> Result<(Arc<dyn Storage>, testcontainers::ContainerAsync<Postgres>), Box<dyn std::error::Error>>
{
    let postgres = Postgres::default().with_tag("18-alpine").start().await?;
    let port = postgres.get_host_port_ipv4(5432).await?;
    let database_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    maxio::db::run_migrations(&database_url).await?;
    let pool = maxio::db::create_pool(&database_url, Default::default()).await?;
    let meta: Arc<dyn MetadataStore> = Arc::new(PgMetadataStore::new(
        Arc::new(pool),
        MemoryCacheLimits::default(),
    ));
    let blobs = BlobStorage::new(data_dir).await?;
    Ok((Arc::new(ObjectStorage::new(blobs, meta)), postgres))
}

async fn create_test_bucket(storage: &dyn Storage, bucket: &str) {
    storage
        .create_bucket(&BucketMeta::new_for_owner(
            bucket.to_string(),
            ROOT_CANONICAL_ID.to_string(),
            ROOT_DISPLAY_NAME.to_string(),
        ))
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
fn normalize_folder_prefix_trims_and_appends_slash() {
    assert_eq!(
        normalize_folder_prefix("photos"),
        Some("photos/".to_string())
    );
    assert_eq!(
        normalize_folder_prefix("photos/vacation/"),
        Some("photos/vacation/".to_string())
    );
    assert_eq!(normalize_folder_prefix("  "), None);
    assert_eq!(normalize_folder_prefix("/"), None);
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
async fn folder_delete_stats_counts_nested_objects() {
    let temp = tempfile::tempdir().unwrap();
    let (storage, _pg) = test_storage(temp.path().to_str().unwrap()).await.unwrap();
    create_test_bucket(storage.as_ref(), "bucket").await;

    storage
        .put_object(
            "bucket",
            "photos/",
            "application/x-directory",
            Box::pin(tokio::io::empty()),
            None,
        )
        .await
        .unwrap();
    storage
        .put_object(
            "bucket",
            "photos/vacation.jpg",
            "image/jpeg",
            bytes(b"photo-data"),
            None,
        )
        .await
        .unwrap();
    storage
        .put_object(
            "bucket",
            "photos/nested/note.txt",
            "text/plain",
            bytes(b"note"),
            None,
        )
        .await
        .unwrap();

    let (count, size_bytes) =
        folder_delete_stats(storage.as_ref(), "bucket", &[String::from("photos/")])
            .await
            .unwrap();
    assert_eq!(count, 3);
    assert_eq!(size_bytes, 14);
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

    let objects = maxio::storage::list_objects_all(storage.as_ref(), "bucket", "folder/")
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

    let objects = maxio::storage::list_objects_all(storage.as_ref(), "bucket", "folder/")
        .await
        .unwrap();
    assert!(objects.is_empty());
}

#[test]
fn sanitize_filename_strips_header_injection_chars() {
    assert_eq!(sanitize_filename("file\"name.txt"), "filename.txt");
    assert_eq!(sanitize_filename("safe.txt"), "safe.txt");
}

#[test]
fn object_get_op_from_query_defaults_to_metadata() {
    let op = ObjectGetOp::from_query(&ObjectGetQuery::default()).unwrap();
    assert_eq!(op, ObjectGetOp::Metadata);
}

#[test]
fn object_get_op_from_query_download_and_presign() {
    let download = ObjectGetOp::from_query(&ObjectGetQuery {
        download: Some("1".into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(download, ObjectGetOp::Download);

    let presign = ObjectGetOp::from_query(&ObjectGetQuery {
        presign: Some("1".into()),
        expires: Some(600),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(presign, ObjectGetOp::Presign { expires_secs: 600 });
}

#[tokio::test]
async fn batch_delete_keys_removes_objects() {
    let temp = tempfile::tempdir().unwrap();
    let (storage, _pg) = test_storage(temp.path().to_str().unwrap()).await.unwrap();
    create_test_bucket(storage.as_ref(), "bucket").await;

    storage
        .put_object("bucket", "a.txt", "text/plain", bytes(b"a"), None)
        .await
        .unwrap();
    storage
        .put_object("bucket", "b.txt", "text/plain", bytes(b"b"), None)
        .await
        .unwrap();

    let outcome = batch_delete_keys(
        storage.as_ref(),
        "bucket",
        &[String::from("a.txt"), String::from("b.txt")],
        1000,
    )
    .await
    .unwrap();
    assert_eq!(outcome.succeeded.len(), 2);
    assert!(outcome.failed.is_empty());
}
