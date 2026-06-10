-- Redundant with UNIQUE(bucket_id, key).
DROP INDEX IF EXISTS objects_bucket_key_idx;

-- Redundant with UNIQUE(bucket_id, key, version_id).
DROP INDEX IF EXISTS object_versions_bucket_key_idx;
