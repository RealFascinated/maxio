DROP INDEX IF EXISTS objects_bucket_last_modified_idx;
DROP INDEX IF EXISTS object_versions_noncurrent_expire_idx;
ALTER TABLE object_versions DROP COLUMN IF EXISTS noncurrent_since;
DROP INDEX IF EXISTS bucket_lifecycle_rules_bucket_idx;
DROP TABLE IF EXISTS bucket_lifecycle_rules;
