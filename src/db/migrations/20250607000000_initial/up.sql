-- Buckets
CREATE TABLE buckets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL,
    region TEXT NOT NULL,
    versioning BOOLEAN NOT NULL DEFAULT FALSE,
    owner_id TEXT NOT NULL,
    owner_display_name TEXT NOT NULL
);

CREATE TABLE bucket_policies (
    bucket_id UUID PRIMARY KEY REFERENCES buckets(id) ON DELETE CASCADE,
    document TEXT NOT NULL
);

CREATE TABLE bucket_cors_rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    bucket_id UUID NOT NULL REFERENCES buckets(id) ON DELETE CASCADE,
    allowed_origins TEXT[] NOT NULL,
    allowed_methods TEXT[] NOT NULL,
    allowed_headers TEXT[] NOT NULL DEFAULT '{}',
    expose_headers TEXT[] NOT NULL DEFAULT '{}',
    max_age_seconds INTEGER
);

CREATE INDEX bucket_cors_rules_bucket_id_idx ON bucket_cors_rules(bucket_id);

CREATE TABLE bucket_acl_grants (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    bucket_id UUID NOT NULL REFERENCES buckets(id) ON DELETE CASCADE,
    grantee_type TEXT NOT NULL,
    grantee_id TEXT,
    grantee_uri TEXT,
    grantee_display_name TEXT,
    permission TEXT NOT NULL
);

CREATE INDEX bucket_acl_grants_bucket_id_idx ON bucket_acl_grants(bucket_id);

-- Current objects
CREATE TABLE objects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    bucket_id UUID NOT NULL REFERENCES buckets(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    size BIGINT NOT NULL,
    etag TEXT NOT NULL,
    content_type TEXT NOT NULL,
    last_modified TIMESTAMPTZ NOT NULL,
    owner_id TEXT NOT NULL,
    owner_display_name TEXT NOT NULL,
    version_id TEXT,
    is_delete_marker BOOLEAN NOT NULL DEFAULT FALSE,
    is_folder_marker BOOLEAN NOT NULL DEFAULT FALSE,
    part_sizes BIGINT[],
    UNIQUE(bucket_id, key)
);

CREATE INDEX objects_bucket_key_idx ON objects(bucket_id, key);
CREATE INDEX objects_bucket_key_pattern_idx ON objects(bucket_id, key text_pattern_ops);

CREATE TABLE object_tags (
    object_id UUID NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    tag_key TEXT NOT NULL,
    tag_value TEXT NOT NULL,
    PRIMARY KEY (object_id, tag_key)
);

CREATE TABLE object_acl_grants (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    object_id UUID NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    grantee_type TEXT NOT NULL,
    grantee_id TEXT,
    grantee_uri TEXT,
    grantee_display_name TEXT,
    permission TEXT NOT NULL
);

CREATE INDEX object_acl_grants_object_id_idx ON object_acl_grants(object_id);

CREATE TABLE object_checksums (
    object_id UUID PRIMARY KEY REFERENCES objects(id) ON DELETE CASCADE,
    algorithm TEXT NOT NULL,
    value TEXT NOT NULL
);

-- Object versions
CREATE TABLE object_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    bucket_id UUID NOT NULL REFERENCES buckets(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    version_id TEXT NOT NULL,
    size BIGINT NOT NULL,
    etag TEXT NOT NULL,
    content_type TEXT NOT NULL,
    last_modified TIMESTAMPTZ NOT NULL,
    owner_id TEXT NOT NULL,
    owner_display_name TEXT NOT NULL,
    is_delete_marker BOOLEAN NOT NULL DEFAULT FALSE,
    is_folder_marker BOOLEAN NOT NULL DEFAULT FALSE,
    part_sizes BIGINT[],
    is_current BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE(bucket_id, key, version_id)
);

CREATE INDEX object_versions_bucket_key_idx ON object_versions(bucket_id, key, version_id);

CREATE TABLE object_version_tags (
    object_version_id UUID NOT NULL REFERENCES object_versions(id) ON DELETE CASCADE,
    tag_key TEXT NOT NULL,
    tag_value TEXT NOT NULL,
    PRIMARY KEY (object_version_id, tag_key)
);

CREATE TABLE object_version_acl_grants (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    object_version_id UUID NOT NULL REFERENCES object_versions(id) ON DELETE CASCADE,
    grantee_type TEXT NOT NULL,
    grantee_id TEXT,
    grantee_uri TEXT,
    grantee_display_name TEXT,
    permission TEXT NOT NULL
);

CREATE INDEX object_version_acl_grants_version_id_idx ON object_version_acl_grants(object_version_id);

CREATE TABLE object_version_checksums (
    object_version_id UUID PRIMARY KEY REFERENCES object_versions(id) ON DELETE CASCADE,
    algorithm TEXT NOT NULL,
    value TEXT NOT NULL
);

-- Multipart uploads
CREATE TABLE multipart_uploads (
    upload_id TEXT PRIMARY KEY,
    bucket_id UUID NOT NULL REFERENCES buckets(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    content_type TEXT NOT NULL,
    initiated TIMESTAMPTZ NOT NULL,
    checksum_algorithm TEXT
);

CREATE INDEX multipart_uploads_bucket_initiated_idx ON multipart_uploads(bucket_id, initiated);

CREATE TABLE multipart_parts (
    upload_id TEXT NOT NULL REFERENCES multipart_uploads(upload_id) ON DELETE CASCADE,
    part_number INTEGER NOT NULL,
    etag TEXT NOT NULL,
    size BIGINT NOT NULL,
    last_modified TIMESTAMPTZ NOT NULL,
    checksum_algorithm TEXT,
    checksum_value TEXT,
    PRIMARY KEY (upload_id, part_number)
);

-- IAM
CREATE TABLE iam_users (
    username TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE iam_access_keys (
    access_key_id TEXT PRIMARY KEY,
    user_username TEXT NOT NULL REFERENCES iam_users(username) ON DELETE CASCADE,
    secret_access_key TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX iam_access_keys_user_idx ON iam_access_keys(user_username);

CREATE TABLE iam_managed_policies (
    policy_name TEXT PRIMARY KEY,
    policy_id TEXT NOT NULL,
    arn TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE iam_managed_policy_statements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    policy_name TEXT NOT NULL REFERENCES iam_managed_policies(policy_name) ON DELETE CASCADE,
    sid TEXT,
    effect TEXT NOT NULL,
    actions TEXT[] NOT NULL DEFAULT '{}',
    resources TEXT[] NOT NULL DEFAULT '{}',
    principal JSONB,
    condition JSONB
);

CREATE INDEX iam_managed_policy_statements_policy_idx ON iam_managed_policy_statements(policy_name);

CREATE TABLE iam_user_inline_policies (
    user_username TEXT NOT NULL REFERENCES iam_users(username) ON DELETE CASCADE,
    policy_name TEXT NOT NULL,
    document JSONB NOT NULL,
    PRIMARY KEY (user_username, policy_name)
);

CREATE TABLE iam_user_policy_attachments (
    user_username TEXT NOT NULL REFERENCES iam_users(username) ON DELETE CASCADE,
    policy_name TEXT NOT NULL REFERENCES iam_managed_policies(policy_name) ON DELETE CASCADE,
    PRIMARY KEY (user_username, policy_name)
);
