ALTER TABLE objects RESET (fillfactor);

CREATE INDEX objects_bucket_last_modified_idx
    ON objects(bucket_id, last_modified)
    WHERE is_delete_marker = FALSE AND is_folder_marker = FALSE;

CREATE INDEX objects_bucket_key_null_version_idx
    ON objects(bucket_id, key)
    WHERE version_id IS NULL;
