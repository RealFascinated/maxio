CREATE INDEX objects_bucket_key_idx ON objects(bucket_id, key);
CREATE INDEX object_versions_bucket_key_idx ON object_versions(bucket_id, key, version_id);
