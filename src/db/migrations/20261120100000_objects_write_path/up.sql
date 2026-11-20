-- Neither index is used by any hot query, but every object PUT had to maintain both.
-- The (bucket_id, key) unique index already serves the null-version lookup, and the
-- lifecycle expiry scan is a periodic background job.
DROP INDEX IF EXISTS objects_bucket_key_null_version_idx;
DROP INDEX IF EXISTS objects_bucket_last_modified_idx;

-- With no index covering any mutable column, overwrites can be HOT updates if pages keep free space.
ALTER TABLE objects SET (fillfactor = 90);
