diesel::table! {
    buckets (id) {
        id -> Uuid,
        name -> Text,
        created_at -> Timestamptz,
        region -> Text,
        versioning -> Bool,
        owner_id -> Text,
        owner_display_name -> Text,
    }
}

diesel::table! {
    bucket_policies (bucket_id) {
        bucket_id -> Uuid,
        document -> Text,
    }
}

diesel::table! {
    bucket_cors_rules (id) {
        id -> Uuid,
        bucket_id -> Uuid,
        allowed_origins -> Array<Text>,
        allowed_methods -> Array<Text>,
        allowed_headers -> Array<Text>,
        expose_headers -> Array<Text>,
        max_age_seconds -> Nullable<Integer>,
    }
}

diesel::table! {
    bucket_acl_grants (id) {
        id -> Uuid,
        bucket_id -> Uuid,
        grantee_type -> Text,
        grantee_id -> Nullable<Text>,
        grantee_uri -> Nullable<Text>,
        grantee_display_name -> Nullable<Text>,
        permission -> Text,
    }
}

diesel::table! {
    objects (id) {
        id -> Uuid,
        bucket_id -> Uuid,
        key -> Text,
        size -> Int8,
        etag -> Text,
        content_type -> Text,
        last_modified -> Timestamptz,
        owner_id -> Text,
        owner_display_name -> Text,
        version_id -> Nullable<Text>,
        is_delete_marker -> Bool,
        storage_format -> Nullable<Text>,
        is_folder_marker -> Bool,
        part_sizes -> Nullable<Array<Int8>>,
    }
}

diesel::table! {
    object_tags (object_id, tag_key) {
        object_id -> Uuid,
        tag_key -> Text,
        tag_value -> Text,
    }
}

diesel::table! {
    object_acl_grants (id) {
        id -> Uuid,
        object_id -> Uuid,
        grantee_type -> Text,
        grantee_id -> Nullable<Text>,
        grantee_uri -> Nullable<Text>,
        grantee_display_name -> Nullable<Text>,
        permission -> Text,
    }
}

diesel::table! {
    object_checksums (object_id) {
        object_id -> Uuid,
        algorithm -> Text,
        value -> Text,
    }
}

diesel::table! {
    object_versions (id) {
        id -> Uuid,
        bucket_id -> Uuid,
        key -> Text,
        version_id -> Text,
        size -> Int8,
        etag -> Text,
        content_type -> Text,
        last_modified -> Timestamptz,
        owner_id -> Text,
        owner_display_name -> Text,
        is_delete_marker -> Bool,
        storage_format -> Nullable<Text>,
        is_folder_marker -> Bool,
        part_sizes -> Nullable<Array<Int8>>,
        is_current -> Bool,
    }
}

diesel::table! {
    object_version_tags (object_version_id, tag_key) {
        object_version_id -> Uuid,
        tag_key -> Text,
        tag_value -> Text,
    }
}

diesel::table! {
    object_version_acl_grants (id) {
        id -> Uuid,
        object_version_id -> Uuid,
        grantee_type -> Text,
        grantee_id -> Nullable<Text>,
        grantee_uri -> Nullable<Text>,
        grantee_display_name -> Nullable<Text>,
        permission -> Text,
    }
}

diesel::table! {
    object_version_checksums (object_version_id) {
        object_version_id -> Uuid,
        algorithm -> Text,
        value -> Text,
    }
}

diesel::table! {
    multipart_uploads (upload_id) {
        upload_id -> Text,
        bucket_id -> Uuid,
        key -> Text,
        content_type -> Text,
        initiated -> Timestamptz,
        checksum_algorithm -> Nullable<Text>,
    }
}

diesel::table! {
    multipart_parts (upload_id, part_number) {
        upload_id -> Text,
        part_number -> Int4,
        etag -> Text,
        size -> Int8,
        last_modified -> Timestamptz,
        checksum_algorithm -> Nullable<Text>,
        checksum_value -> Nullable<Text>,
    }
}

diesel::table! {
    iam_users (username) {
        username -> Text,
        user_id -> Text,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    iam_access_keys (access_key_id) {
        access_key_id -> Text,
        user_username -> Text,
        secret_access_key -> Text,
        status -> Text,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    iam_managed_policies (policy_name) {
        policy_name -> Text,
        policy_id -> Text,
        arn -> Text,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    iam_managed_policy_statements (id) {
        id -> Uuid,
        policy_name -> Text,
        sid -> Nullable<Text>,
        effect -> Text,
        actions -> Array<Text>,
        resources -> Array<Text>,
        principal -> Nullable<Jsonb>,
        condition -> Nullable<Jsonb>,
    }
}

diesel::table! {
    iam_user_inline_policies (user_username, policy_name) {
        user_username -> Text,
        policy_name -> Text,
        document -> Jsonb,
    }
}

diesel::table! {
    iam_user_policy_attachments (user_username, policy_name) {
        user_username -> Text,
        policy_name -> Text,
    }
}

diesel::joinable!(bucket_policies -> buckets (bucket_id));
diesel::joinable!(bucket_cors_rules -> buckets (bucket_id));
diesel::joinable!(bucket_acl_grants -> buckets (bucket_id));
diesel::joinable!(objects -> buckets (bucket_id));
diesel::joinable!(object_tags -> objects (object_id));
diesel::joinable!(object_acl_grants -> objects (object_id));
diesel::joinable!(object_checksums -> objects (object_id));
diesel::joinable!(object_versions -> buckets (bucket_id));
diesel::joinable!(multipart_uploads -> buckets (bucket_id));
diesel::joinable!(multipart_parts -> multipart_uploads (upload_id));
diesel::joinable!(iam_access_keys -> iam_users (user_username));
diesel::joinable!(iam_user_inline_policies -> iam_users (user_username));
diesel::joinable!(iam_user_policy_attachments -> iam_users (user_username));

diesel::allow_tables_to_appear_in_same_query!(
    buckets,
    bucket_policies,
    bucket_cors_rules,
    bucket_acl_grants,
    objects,
    object_tags,
    object_acl_grants,
    object_checksums,
    object_versions,
    object_version_tags,
    object_version_acl_grants,
    object_version_checksums,
    multipart_uploads,
    multipart_parts,
    iam_users,
    iam_access_keys,
    iam_managed_policies,
    iam_managed_policy_statements,
    iam_user_inline_policies,
    iam_user_policy_attachments,
);
