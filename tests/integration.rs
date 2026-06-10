#![allow(
    clippy::needless_borrows_for_generic_args,
    clippy::useless_vec,
    clippy::needless_range_loop,
    clippy::manual_repeat_n
)]

#[path = "integration/common.rs"]
mod common;

#[path = "integration/acl.rs"]
mod acl;
#[path = "integration/auth.rs"]
mod auth;
#[path = "integration/bucket.rs"]
mod bucket;
#[path = "integration/checksum.rs"]
mod checksum;
#[path = "integration/chunked_upload.rs"]
mod chunked_upload;
#[path = "integration/conditionals.rs"]
mod conditionals;
#[path = "integration/console.rs"]
mod console;
#[path = "integration/console_admin.rs"]
mod console_admin;
#[path = "integration/console_upload.rs"]
mod console_upload;
#[path = "integration/copy.rs"]
mod copy;
#[path = "integration/cors.rs"]
mod cors;
#[path = "integration/default_buckets.rs"]
mod default_buckets;
#[path = "integration/delete_objects.rs"]
mod delete_objects;
#[path = "integration/folder_marker.rs"]
mod folder_marker;
#[path = "integration/iam.rs"]
mod iam;
#[path = "integration/list_v1.rs"]
mod list_v1;
#[path = "integration/list_v2.rs"]
mod list_v2;
#[path = "integration/metrics.rs"]
mod metrics;
#[path = "integration/multipart.rs"]
mod multipart;
#[path = "integration/object.rs"]
mod object;
#[path = "integration/policy.rs"]
mod policy;
#[path = "integration/presigned.rs"]
mod presigned;
#[path = "integration/range.rs"]
mod range;
#[path = "integration/server.rs"]
mod server;
#[path = "integration/storage.rs"]
mod storage;
#[path = "integration/tagging.rs"]
mod tagging;
#[path = "integration/upload_part_copy.rs"]
mod upload_part_copy;
#[path = "integration/versioning.rs"]
mod versioning;
