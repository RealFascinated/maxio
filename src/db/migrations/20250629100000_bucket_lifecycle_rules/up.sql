CREATE TABLE bucket_lifecycle_rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    bucket_id UUID NOT NULL REFERENCES buckets(id) ON DELETE CASCADE,
    rule_id TEXT NOT NULL,
    enabled BOOL NOT NULL DEFAULT TRUE,
    prefix TEXT NOT NULL DEFAULT '',
    actions JSONB NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    UNIQUE (bucket_id, rule_id)
);

CREATE INDEX bucket_lifecycle_rules_bucket_idx ON bucket_lifecycle_rules(bucket_id);

ALTER TABLE object_versions ADD COLUMN noncurrent_since TIMESTAMPTZ;

CREATE INDEX object_versions_noncurrent_expire_idx
    ON object_versions(bucket_id, noncurrent_since)
    WHERE is_current = FALSE AND is_delete_marker = FALSE;

CREATE INDEX objects_bucket_last_modified_idx
    ON objects(bucket_id, last_modified)
    WHERE is_delete_marker = FALSE AND is_folder_marker = FALSE;
