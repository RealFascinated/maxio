use std::collections::HashSet;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use maxio::metrics::MetricsRegistry;
use maxio::metrics::cache_name::OBJECT_DISK;
use maxio::storage::blob::{BlobStorage, IO_BUFFER_SIZE, stream_to_writer};
use maxio::storage::cache::{CacheLayer, decode_index, encode_index};
use maxio::storage::{ObjectMeta, PartMeta, validate_bucket_name};
use tempfile::TempDir;
use tokio::io::{AsyncRead, BufWriter, ReadBuf};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

#[test]
fn rejects_path_like_bucket_names() {
    for name in [
        "../evil",
        "a/b",
        "ab",
        "evil..bucket",
        "Uppercase",
        "a.-b",
        "a-.b",
        "192.168.0.1",
    ] {
        assert!(
            validate_bucket_name(name).is_err(),
            "{name} should be invalid"
        );
    }
}

#[test]
fn accepts_s3_style_bucket_name() {
    assert!(validate_bucket_name("prod-logs.2026").is_ok());
}

#[test]
fn cache_index_roundtrip() {
    let entries = vec![
        ("bucket-a".into(), "path/obj.txt".into(), 42u64),
        ("bucket-b".into(), "folder/".into(), 0u64),
    ];
    let mut dirty = HashSet::new();
    dirty.insert(("bucket-a".into(), "path/obj.txt".into()));
    let data = encode_index(&entries, &dirty).unwrap();
    let (decoded_entries, decoded_dirty) = decode_index(&data).unwrap();
    assert_eq!(decoded_entries, entries);
    assert_eq!(decoded_dirty, dirty);
}

#[tokio::test]
async fn cache_flush_dirty_after_restart() {
    let cache_root = TempDir::new().unwrap();
    let data_root = TempDir::new().unwrap();
    let data_buckets = data_root.path().join("buckets");
    tokio::fs::create_dir_all(&data_buckets).await.unwrap();

    let cache_path = cache_root
        .path()
        .join("buckets")
        .join("bucket-a")
        .join("obj.txt");
    tokio::fs::create_dir_all(cache_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&cache_path, b"cached payload")
        .await
        .unwrap();

    let layer = Arc::new(
        CacheLayer::new(
            cache_root.path().to_str().unwrap(),
            data_buckets.clone(),
            1024 * 1024,
            true,
            Duration::from_secs(30),
        )
        .await
        .unwrap(),
    );
    layer.clone().spawn_scan_task();

    let data_path = data_buckets.join("bucket-a").join("obj.txt");
    assert!(
        !tokio::fs::try_exists(&data_path).await.unwrap(),
        "data dir should not have the object before flush"
    );

    layer.flush_dirty().await.unwrap();

    let data = tokio::fs::read(&data_path).await.unwrap();
    assert_eq!(data, b"cached payload");
}

fn blob_meta(size: u64) -> ObjectMeta {
    ObjectMeta {
        key: "obj.txt".to_string(),
        size,
        etag: "\"abc\"".to_string(),
        content_type: "text/plain".to_string(),
        last_modified: "2025-01-01T00:00:00.000Z".to_string(),
        owner_id: "owner".to_string(),
        owner_display_name: "Owner".to_string(),
        acl: None,
        version_id: None,
        is_delete_marker: false,
        checksum_algorithm: None,
        checksum_value: None,
        tags: None,
        part_sizes: None,
    }
}

#[tokio::test]
async fn blob_read_miss_populates_cache_and_records_metrics() {
    let data_root = TempDir::new().unwrap();
    let cache_root = TempDir::new().unwrap();
    let data_buckets = data_root.path().join("buckets");
    let data_path = data_buckets.join("bucket-a").join("obj.txt");
    tokio::fs::create_dir_all(data_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&data_path, b"payload").await.unwrap();

    let metrics = Arc::new(MetricsRegistry::new().unwrap());
    let cache = Arc::new(
        CacheLayer::new(
            cache_root.path().to_str().unwrap(),
            data_buckets.clone(),
            1024 * 1024,
            true,
            Duration::from_secs(30),
        )
        .await
        .unwrap()
        .with_metrics(Arc::clone(&metrics)),
    );
    cache.clone().spawn_scan_task();
    let blobs = BlobStorage::new(data_root.path().to_str().unwrap())
        .await
        .unwrap()
        .with_cache(cache);

    let object_meta = blob_meta(7);
    blobs
        .open_object("bucket-a", "obj.txt", &object_meta)
        .await
        .unwrap();
    let disk = metrics
        .snapshot()
        .caches
        .into_iter()
        .find(|c| c.id == OBJECT_DISK)
        .unwrap();
    assert_eq!(disk.misses, 1);
    assert_eq!(disk.hits, 0);

    blobs
        .open_object("bucket-a", "obj.txt", &object_meta)
        .await
        .unwrap();
    let disk = metrics
        .snapshot()
        .caches
        .into_iter()
        .find(|c| c.id == OBJECT_DISK)
        .unwrap();
    assert_eq!(disk.misses, 1);
    assert_eq!(disk.hits, 1);
}

#[tokio::test]
async fn blob_stale_cache_file_is_skipped_for_read() {
    let data_root = TempDir::new().unwrap();
    let cache_root = TempDir::new().unwrap();
    let data_buckets = data_root.path().join("buckets");
    let data_path = data_buckets.join("bucket-a").join("obj.txt");
    tokio::fs::create_dir_all(data_path.parent().unwrap())
        .await
        .unwrap();
    let payload = b"full-payload";
    tokio::fs::write(&data_path, payload).await.unwrap();

    let cache_path = cache_root
        .path()
        .join("buckets")
        .join("bucket-a")
        .join("obj.txt");
    tokio::fs::create_dir_all(cache_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&cache_path, b"partial").await.unwrap();

    let cache = Arc::new(
        CacheLayer::new(
            cache_root.path().to_str().unwrap(),
            data_buckets.clone(),
            1024 * 1024,
            true,
            Duration::from_secs(30),
        )
        .await
        .unwrap(),
    );
    cache.clone().spawn_scan_task();
    let blobs = BlobStorage::new(data_root.path().to_str().unwrap())
        .await
        .unwrap()
        .with_cache(cache);

    let mut stream = blobs
        .open_object("bucket-a", "obj.txt", &blob_meta(payload.len() as u64))
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    assert_eq!(buf, payload);
    let cached = tokio::fs::read(&cache_path).await.unwrap();
    assert_eq!(
        cached, payload,
        "stale cache file should be replaced from data"
    );
}

#[tokio::test]
async fn blob_writeback_unlink_clears_cache_immediately() {
    let data_root = TempDir::new().unwrap();
    let cache_root = TempDir::new().unwrap();
    let data_buckets = data_root.path().join("buckets");
    let data_path = data_buckets.join("bucket-a").join("obj.txt");
    let cache_path = cache_root
        .path()
        .join("buckets")
        .join("bucket-a")
        .join("obj.txt");
    tokio::fs::create_dir_all(cache_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&cache_path, b"cached-only").await.unwrap();
    tokio::fs::create_dir_all(data_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&data_path, b"cached-only").await.unwrap();

    let cache = Arc::new(
        CacheLayer::new(
            cache_root.path().to_str().unwrap(),
            data_buckets.clone(),
            1024 * 1024,
            true,
            Duration::from_secs(30),
        )
        .await
        .unwrap(),
    );
    cache.clone().spawn_scan_task();
    cache.mark_dirty("bucket-a", "obj.txt", 11).await;
    let blobs = BlobStorage::new(data_root.path().to_str().unwrap())
        .await
        .unwrap()
        .with_cache(cache);

    blobs.unlink_object("bucket-a", "obj.txt").await.unwrap();

    assert!(
        !tokio::fs::try_exists(&cache_path).await.unwrap(),
        "cache file should be removed before returning"
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !tokio::fs::try_exists(&data_path).await.unwrap(),
        "data file should be removed in background"
    );
}

#[tokio::test]
async fn blob_unlink_objects_batch_prunes_empty_parent_dirs() {
    let temp = TempDir::new().unwrap();
    let blobs = BlobStorage::new(temp.path().to_str().unwrap())
        .await
        .unwrap();
    let bucket_dir = temp.path().join("buckets").join("bucket");
    tokio::fs::create_dir_all(bucket_dir.join("test/nested"))
        .await
        .unwrap();
    tokio::fs::write(bucket_dir.join("test/nested/file.txt"), b"x")
        .await
        .unwrap();
    tokio::fs::write(bucket_dir.join("test/.folder"), b"")
        .await
        .unwrap();

    blobs
        .unlink_objects_batch(
            "bucket",
            &["test/nested/file.txt".to_string(), "test/".to_string()],
            4,
        )
        .await
        .unwrap();

    assert!(
        !tokio::fs::try_exists(bucket_dir.join("test"))
            .await
            .unwrap(),
        "empty folder tree should be removed from disk"
    );
}

const FIVE_GIB: u64 = 5 * 1024 * 1024 * 1024;

struct LargePartReader {
    remaining: u64,
    max_buf_capacity: Arc<AtomicUsize>,
}

impl AsyncRead for LargePartReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let capacity = buf.remaining();
        self.max_buf_capacity.fetch_max(capacity, Ordering::Relaxed);
        if self.remaining == 0 {
            return Poll::Ready(Ok(()));
        }
        let n = capacity.min(self.remaining as usize);
        buf.put_slice(&[0x42; IO_BUFFER_SIZE][..n]);
        self.remaining -= n as u64;
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn stream_to_writer_does_not_buffer_whole_part() {
    let max_buf_capacity = Arc::new(AtomicUsize::new(0));
    let mut reader = LargePartReader {
        remaining: FIVE_GIB,
        max_buf_capacity: Arc::clone(&max_buf_capacity),
    };
    let dir = TempDir::new().unwrap();
    let out_path = dir.path().join("out.bin");
    let out = tokio::fs::File::create(&out_path).await.unwrap();
    let mut writer = BufWriter::with_capacity(IO_BUFFER_SIZE, out);
    let mut buf = vec![0u8; IO_BUFFER_SIZE];

    let copied = stream_to_writer(&mut reader, &mut writer, &mut buf)
        .await
        .unwrap();
    writer.flush().await.unwrap();

    assert_eq!(copied, FIVE_GIB);
    assert_eq!(
        tokio::fs::metadata(&out_path).await.unwrap().len(),
        FIVE_GIB
    );
    assert!(
        max_buf_capacity.load(Ordering::Relaxed) <= IO_BUFFER_SIZE,
        "peak read buffer capacity was {} bytes, expected <= {}",
        max_buf_capacity.load(Ordering::Relaxed),
        IO_BUFFER_SIZE
    );
}

async fn etag_for_file(path: &std::path::Path) -> String {
    use maxio::storage::hashing::EtagMd5;
    let mut file = tokio::fs::File::open(path).await.unwrap();
    let mut hasher = EtagMd5::new();
    let mut buf = vec![0u8; IO_BUFFER_SIZE];
    loop {
        let n = file.read(&mut buf).await.unwrap();
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    format!("\"{}\"", hex::encode(hasher.finalize()))
}

async fn write_sparse_part(path: &std::path::Path, size: u64, head: u8, tail: u8) {
    let mut file = tokio::fs::File::create(path).await.unwrap();
    file.set_len(size).await.unwrap();
    file.write_all(&[head]).await.unwrap();
    file.seek(std::io::SeekFrom::Start(size - 1)).await.unwrap();
    file.write_all(&[tail]).await.unwrap();
}

#[tokio::test]
async fn assemble_multipart_streams_five_gib_sparse_parts() {
    let data_root = TempDir::new().unwrap();
    let blobs = BlobStorage::new(data_root.path().to_str().unwrap())
        .await
        .unwrap();

    let bucket = "mp-bucket";
    let upload_id = "upload-1";
    blobs.ensure_upload_dir(bucket, upload_id).await.unwrap();

    let part1_path = blobs.part_path(bucket, upload_id, 1);
    write_sparse_part(&part1_path, FIVE_GIB, b'A', b'Z').await;
    let part1_etag = etag_for_file(&part1_path).await;

    let part2_path = blobs.part_path(bucket, upload_id, 2);
    tokio::fs::write(&part2_path, b"tail-part").await.unwrap();
    let part2_etag = etag_for_file(&part2_path).await;

    let parts = vec![
        PartMeta {
            part_number: 1,
            etag: part1_etag,
            size: FIVE_GIB,
            last_modified: "2025-01-01T00:00:00.000Z".to_string(),
            checksum_algorithm: None,
            checksum_value: None,
        },
        PartMeta {
            part_number: 2,
            etag: part2_etag,
            size: 9,
            last_modified: "2025-01-01T00:00:00.000Z".to_string(),
            checksum_algorithm: None,
            checksum_value: None,
        },
    ];

    let written = blobs
        .assemble_multipart_temp(bucket, "large.bin", upload_id, &parts)
        .await
        .unwrap();

    assert_eq!(written.size, FIVE_GIB + 9);
    assert_eq!(
        tokio::fs::metadata(&written.tmp_path).await.unwrap().len(),
        FIVE_GIB + 9
    );

    let mut assembled = tokio::fs::File::open(&written.tmp_path).await.unwrap();
    let mut head = [0u8; 1];
    assembled.read_exact(&mut head).await.unwrap();
    assert_eq!(head, [b'A']);

    assembled
        .seek(std::io::SeekFrom::Start(FIVE_GIB - 1))
        .await
        .unwrap();
    let mut part1_tail = [0u8; 1];
    assembled.read_exact(&mut part1_tail).await.unwrap();
    assert_eq!(part1_tail, [b'Z']);

    assembled
        .seek(std::io::SeekFrom::Start(FIVE_GIB))
        .await
        .unwrap();
    let mut tail = [0u8; 9];
    assembled.read_exact(&mut tail).await.unwrap();
    assert_eq!(&tail, b"tail-part");

    let _ = tokio::fs::remove_file(&written.tmp_path).await;
}

#[tokio::test]
async fn blob_small_flat_write_uses_buffered_path() {
    let data_root = TempDir::new().unwrap();
    let blobs = BlobStorage::new(data_root.path().to_str().unwrap())
        .await
        .unwrap();

    let payload = vec![0xAB; 4096];
    let written = blobs
        .write_flat_object_temp(
            "bucket-a",
            "small.bin",
            Box::pin(std::io::Cursor::new(payload.clone())),
            None,
        )
        .await
        .unwrap();

    assert_eq!(written.size, 4096);
    let data = tokio::fs::read(&written.tmp_path).await.unwrap();
    assert_eq!(data, payload);
    let _ = tokio::fs::remove_file(&written.tmp_path).await;
}

#[tokio::test]
async fn assemble_multipart_single_part_renames_without_copy() {
    let data_root = TempDir::new().unwrap();
    let blobs = BlobStorage::new(data_root.path().to_str().unwrap())
        .await
        .unwrap();
    let bucket = "bucket-a";
    let upload_id = "upload-1";
    let payload = b"single-part-payload".to_vec();

    blobs.ensure_upload_dir(bucket, upload_id).await.unwrap();
    let (etag, size, _, _) = blobs
        .write_part(
            bucket,
            upload_id,
            1,
            Box::pin(std::io::Cursor::new(payload.clone())),
            None,
        )
        .await
        .unwrap();
    assert_eq!(size, payload.len() as u64);

    let part_path = blobs.part_path(bucket, upload_id, 1);
    assert!(tokio::fs::try_exists(&part_path).await.unwrap());

    let part_meta = PartMeta {
        part_number: 1,
        etag,
        size: payload.len() as u64,
        last_modified: "2025-01-01T00:00:00.000Z".to_string(),
        checksum_algorithm: None,
        checksum_value: None,
    };
    let written = blobs
        .assemble_multipart_temp(
            bucket,
            "obj.bin",
            upload_id,
            std::slice::from_ref(&part_meta),
        )
        .await
        .unwrap();

    assert_eq!(written.size, payload.len() as u64);
    assert!(!tokio::fs::try_exists(&part_path).await.unwrap());
    let data = tokio::fs::read(&written.tmp_path).await.unwrap();
    assert_eq!(data, payload);
    let _ = tokio::fs::remove_file(&written.tmp_path).await;
}
