use maxio::db::object_read_cache::{ObjectReadCache, ReadCacheLookup};
use maxio::db::{BucketCache, CachedBucketEntry};
use maxio::storage::ObjectMeta;
use uuid::Uuid;

fn sample_bucket_entry() -> CachedBucketEntry {
    CachedBucketEntry {
        id: Uuid::new_v4(),
        versioning: false,
        versioning_suspended: false,
        owner_id: "owner".into(),
        owner_display_name: "Owner".into(),
        policy: None,
        acl: None,
        cors_rules: vec![],
    }
}

fn sample_meta(key: &str) -> ObjectMeta {
    ObjectMeta {
        key: key.to_string(),
        size: 42,
        etag: "\"abc\"".into(),
        content_type: "text/plain".into(),
        last_modified: "Mon, 01 Jan 2024 00:00:00 GMT".into(),
        owner_id: "owner".into(),
        owner_display_name: "Owner".into(),
        acl: None,
        version_id: None,
        is_delete_marker: false,
        checksum_algorithm: None,
        checksum_value: None,
        tags: None,
        part_sizes: None,
    }
}

#[test]
fn bucket_cache_stores_put_context() {
    let cache = BucketCache::new(None, 256);
    let entry = sample_bucket_entry();
    let id = entry.id;
    cache.insert("bench", entry);

    let cached = cache.get("bench").expect("entry");
    assert_eq!(cached.id, id);
}

#[test]
fn bucket_cache_updates_versioning_in_place() {
    let cache = BucketCache::new(None, 256);
    cache.insert("b", sample_bucket_entry());
    cache.set_versioning_state("b", true, false);
    assert!(cache.get("b").unwrap().versioning);
    assert!(!cache.get("b").unwrap().versioning_suspended);
}

#[test]
fn object_read_cache_returns_hit() {
    let cache = ObjectReadCache::new(None, 256);
    let meta = sample_meta("obj.txt");
    cache.insert("bench", "obj.txt", meta.clone());

    assert!(matches!(
        cache.lookup("bench", "obj.txt"),
        ReadCacheLookup::Hit(hit) if hit.key == meta.key && hit.etag == meta.etag
    ));
}

#[test]
fn object_read_cache_absent_tombstone() {
    let cache = ObjectReadCache::new(None, 256);
    cache.mark_absent("bench", "gone.txt");
    assert!(matches!(
        cache.lookup("bench", "gone.txt"),
        ReadCacheLookup::Absent
    ));
}

#[test]
fn object_read_cache_insert_clears_absent() {
    let cache = ObjectReadCache::new(None, 256);
    cache.mark_absent("bench", "obj.txt");
    let meta = sample_meta("obj.txt");
    cache.insert("bench", "obj.txt", meta.clone());
    assert!(matches!(
        cache.lookup("bench", "obj.txt"),
        ReadCacheLookup::Hit(hit) if hit.key == meta.key
    ));
}

#[test]
fn object_read_cache_remove_bucket_purges_keys() {
    let cache = ObjectReadCache::new(None, 256);
    cache.insert("b1", "a", sample_meta("a"));
    cache.insert("b1", "b", sample_meta("b"));
    cache.insert("b2", "c", sample_meta("c"));
    cache.mark_absent("b1", "missing");

    cache.remove_bucket("b1");

    assert!(matches!(cache.lookup("b1", "a"), ReadCacheLookup::Miss));
    assert!(matches!(cache.lookup("b1", "b"), ReadCacheLookup::Miss));
    assert!(matches!(
        cache.lookup("b1", "missing"),
        ReadCacheLookup::Miss
    ));
    assert!(matches!(cache.lookup("b2", "c"), ReadCacheLookup::Hit(_)));
}

#[test]
fn object_read_cache_evicts_lru() {
    let cache = ObjectReadCache::new(None, 2);
    cache.insert("b", "first", sample_meta("first"));
    cache.insert("b", "second", sample_meta("second"));
    let _ = cache.lookup("b", "first");
    cache.insert("b", "third", sample_meta("third"));

    assert!(matches!(
        cache.lookup("b", "first"),
        ReadCacheLookup::Hit(_)
    ));
    assert!(matches!(cache.lookup("b", "second"), ReadCacheLookup::Miss));
    assert!(matches!(
        cache.lookup("b", "third"),
        ReadCacheLookup::Hit(_)
    ));
}
