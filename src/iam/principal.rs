use super::types::IamUser;

pub const ROOT_USERNAME: &str = "root";
pub const ROOT_CANONICAL_ID: &str = "maxio-root";
pub const ROOT_DISPLAY_NAME: &str = "root";
pub const ACCOUNT_ID: &str = "maxio";

/// Authenticated caller attached to each S3 request after auth middleware.
#[derive(Debug, Clone)]
pub struct Principal {
    pub username: String,
    pub user_id: String,
    pub display_name: String,
    pub canonical_id: String,
    pub is_root: bool,
    pub is_anonymous: bool,
}

impl Principal {
    pub fn root() -> Self {
        Self {
            username: ROOT_USERNAME.to_string(),
            user_id: ROOT_CANONICAL_ID.to_string(),
            display_name: ROOT_DISPLAY_NAME.to_string(),
            canonical_id: ROOT_CANONICAL_ID.to_string(),
            is_root: true,
            is_anonymous: false,
        }
    }

    pub fn anonymous() -> Self {
        Self {
            username: String::new(),
            user_id: String::new(),
            display_name: String::new(),
            canonical_id: String::new(),
            is_root: false,
            is_anonymous: true,
        }
    }

    pub fn from_user(user: &IamUser) -> Self {
        Self {
            username: user.username.clone(),
            user_id: user.user_id.clone(),
            display_name: user.username.clone(),
            canonical_id: user.user_id.clone(),
            is_root: false,
            is_anonymous: false,
        }
    }

    pub fn arn(&self) -> String {
        if self.is_root {
            format!("arn:aws:iam::{ACCOUNT_ID}:root")
        } else {
            format!("arn:aws:iam::{ACCOUNT_ID}:user/{}", self.username)
        }
    }
}
