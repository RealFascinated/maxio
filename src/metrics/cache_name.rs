pub const OBJECT_DISK: &str = "object_disk";
pub const BUCKET: &str = "bucket";
pub const OBJECT_READ: &str = "object_read";
pub const SIGNING_KEY: &str = "signing_key";
pub const IAM_ACCESS_KEY: &str = "iam_access_key";
pub const IAM_USER: &str = "iam_user";
pub const IAM_POLICIES: &str = "iam_policies";

pub const ALL: &[&str] = &[
    OBJECT_DISK,
    BUCKET,
    OBJECT_READ,
    SIGNING_KEY,
    IAM_ACCESS_KEY,
    IAM_USER,
    IAM_POLICIES,
];

pub fn display_name(name: &str) -> &'static str {
    match name {
        OBJECT_DISK => "Object disk",
        BUCKET => "Bucket metadata",
        OBJECT_READ => "Object read metadata",
        SIGNING_KEY => "Signing key",
        IAM_ACCESS_KEY => "IAM access key",
        IAM_USER => "IAM user",
        IAM_POLICIES => "IAM policies",
        _ => "Unknown",
    }
}
