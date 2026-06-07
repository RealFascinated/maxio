use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum AclPermission {
    Read,
    Write,
    ReadAcp,
    WriteAcp,
    FullControl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Grantee {
    CanonicalUser {
        id: String,
        display_name: Option<String>,
    },
    Group {
        uri: String,
    },
}

impl Grantee {
    pub const ALL_USERS_URI: &'static str = "http://acs.amazonaws.com/groups/global/AllUsers";
    pub const AUTHENTICATED_USERS_URI: &'static str =
        "http://acs.amazonaws.com/groups/global/AuthenticatedUsers";

    pub fn all_users() -> Self {
        Self::Group {
            uri: Self::ALL_USERS_URI.to_string(),
        }
    }

    pub fn authenticated_users() -> Self {
        Self::Group {
            uri: Self::AUTHENTICATED_USERS_URI.to_string(),
        }
    }

    pub fn canonical(id: &str, display_name: Option<&str>) -> Self {
        Self::CanonicalUser {
            id: id.to_string(),
            display_name: display_name.map(|s| s.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AclGrant {
    pub grantee: Grantee,
    pub permission: AclPermission,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Acl {
    pub owner_id: String,
    pub owner_display_name: String,
    pub grants: Vec<AclGrant>,
}

impl Acl {
    pub fn private(owner_id: &str, owner_display_name: &str) -> Self {
        Self {
            owner_id: owner_id.to_string(),
            owner_display_name: owner_display_name.to_string(),
            grants: vec![AclGrant {
                grantee: Grantee::canonical(owner_id, Some(owner_display_name)),
                permission: AclPermission::FullControl,
            }],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CannedAcl {
    Private,
    PublicRead,
    PublicReadWrite,
    AuthenticatedRead,
    BucketOwnerRead,
    BucketOwnerFullControl,
}

impl CannedAcl {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "private" => Some(Self::Private),
            "public-read" => Some(Self::PublicRead),
            "public-read-write" => Some(Self::PublicReadWrite),
            "authenticated-read" => Some(Self::AuthenticatedRead),
            "bucket-owner-read" => Some(Self::BucketOwnerRead),
            "bucket-owner-full-control" => Some(Self::BucketOwnerFullControl),
            _ => None,
        }
    }

    /// Build an ACL for a bucket or object.
    pub fn to_acl(
        self,
        owner_id: &str,
        owner_display_name: &str,
        bucket_owner_id: Option<&str>,
        bucket_owner_display_name: Option<&str>,
    ) -> Acl {
        let mut grants = vec![AclGrant {
            grantee: Grantee::canonical(owner_id, Some(owner_display_name)),
            permission: AclPermission::FullControl,
        }];

        match self {
            Self::Private => {}
            Self::PublicRead => {
                grants.push(AclGrant {
                    grantee: Grantee::all_users(),
                    permission: AclPermission::Read,
                });
            }
            Self::PublicReadWrite => {
                grants.push(AclGrant {
                    grantee: Grantee::all_users(),
                    permission: AclPermission::Read,
                });
                grants.push(AclGrant {
                    grantee: Grantee::all_users(),
                    permission: AclPermission::Write,
                });
            }
            Self::AuthenticatedRead => {
                grants.push(AclGrant {
                    grantee: Grantee::authenticated_users(),
                    permission: AclPermission::Read,
                });
            }
            Self::BucketOwnerRead => {
                if let (Some(bid), Some(bname)) = (bucket_owner_id, bucket_owner_display_name) {
                    grants.push(AclGrant {
                        grantee: Grantee::canonical(bid, Some(bname)),
                        permission: AclPermission::Read,
                    });
                }
            }
            Self::BucketOwnerFullControl => {
                if let (Some(bid), Some(bname)) = (bucket_owner_id, bucket_owner_display_name) {
                    grants.push(AclGrant {
                        grantee: Grantee::canonical(bid, Some(bname)),
                        permission: AclPermission::FullControl,
                    });
                }
            }
        }

        Acl {
            owner_id: owner_id.to_string(),
            owner_display_name: owner_display_name.to_string(),
            grants,
        }
    }
}

/// Check whether an ACL grants the given permission to a principal.
pub fn acl_allows(
    acl: &Acl,
    principal: &super::principal::Principal,
    permission: AclPermission,
) -> bool {
    if principal.is_root {
        return true;
    }
    if principal.is_anonymous {
        return acl.grants.iter().any(|g| {
            matches!(g.grantee, Grantee::Group { ref uri } if uri == Grantee::ALL_USERS_URI)
                && permission_implies(g.permission, permission)
        });
    }

    acl.grants.iter().any(|g| match &g.grantee {
        Grantee::CanonicalUser { id, .. } => {
            (id == &principal.canonical_id || id == &principal.user_id)
                && permission_implies(g.permission, permission)
        }
        Grantee::Group { uri } if uri == Grantee::AUTHENTICATED_USERS_URI => {
            permission_implies(g.permission, permission)
        }
        _ => false,
    })
}

fn permission_implies(granted: AclPermission, needed: AclPermission) -> bool {
    if granted == AclPermission::FullControl {
        return true;
    }
    granted == needed
}

/// Map S3 action to ACL permission needed.
pub fn action_to_acl_permission(action: &str) -> Option<AclPermission> {
    match action {
        "s3:GetObject" | "s3:ListBucket" => Some(AclPermission::Read),
        "s3:PutObject" | "s3:DeleteObject" => Some(AclPermission::Write),
        "s3:GetBucketAcl" | "s3:GetObjectAcl" => Some(AclPermission::ReadAcp),
        "s3:PutBucketAcl" | "s3:PutObjectAcl" => Some(AclPermission::WriteAcp),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iam::principal::Principal;

    #[test]
    fn private_acl_only_owner() {
        let acl = Acl::private("owner1", "owner");
        let owner = Principal {
            username: "alice".into(),
            user_id: "owner1".into(),
            display_name: "alice".into(),
            canonical_id: "owner1".into(),
            is_root: false,
            is_anonymous: false,
        };
        assert!(acl_allows(&acl, &owner, AclPermission::Read));
        let other = Principal {
            username: "bob".into(),
            user_id: "other".into(),
            display_name: "bob".into(),
            canonical_id: "other".into(),
            is_root: false,
            is_anonymous: false,
        };
        assert!(!acl_allows(&acl, &other, AclPermission::Read));
    }
}
