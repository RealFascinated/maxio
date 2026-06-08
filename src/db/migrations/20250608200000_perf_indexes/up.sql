-- Speed up get_managed_policy_by_arn (called per attached policy during authorization).
CREATE INDEX iam_managed_policies_arn_idx ON iam_managed_policies(arn);

-- Partial index for null-version current objects (ListObjectVersions first page).
CREATE INDEX objects_bucket_key_null_version_idx
    ON objects(bucket_id, key)
    WHERE version_id IS NULL;

-- Partial index for version pointer lookups: latest non-delete-marker per key.
-- Used by update_current_after_delete and similar ORDER BY version_id DESC queries.
CREATE INDEX object_versions_bucket_key_live_idx
    ON object_versions(bucket_id, key, version_id DESC)
    WHERE is_delete_marker = FALSE;
