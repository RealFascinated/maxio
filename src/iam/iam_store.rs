use async_trait::async_trait;

use super::policy::PolicyDocument;
use super::types::*;

#[async_trait]
pub trait IamStore: Send + Sync {
    async fn lookup_by_access_key(
        &self,
        access_key_id: &str,
    ) -> Option<(IamUser, AccessKey)>;
    async fn lookup_by_credentials(
        &self,
        access_key_id: &str,
        secret_access_key: &str,
    ) -> Option<IamUser>;
    async fn get_user(&self, username: &str) -> Option<IamUser>;
    async fn list_users(&self) -> Vec<IamUser>;
    async fn effective_policies(&self, user: &IamUser) -> Vec<PolicyDocument>;
    async fn get_managed_policy(&self, name: &str) -> Option<ManagedPolicy>;
    async fn list_managed_policies(&self) -> Vec<ManagedPolicy>;
    async fn create_user(&self, username: &str) -> Result<IamUser, String>;
    async fn delete_user(&self, username: &str) -> Result<(), String>;
    async fn create_access_key(&self, username: &str) -> Result<AccessKey, String>;
    async fn delete_access_key(&self, username: &str, access_key_id: &str) -> Result<(), String>;
    async fn update_access_key_status(
        &self,
        username: &str,
        access_key_id: &str,
        status: KeyStatus,
    ) -> Result<(), String>;
    async fn put_user_policy(
        &self,
        username: &str,
        policy_name: &str,
        document: PolicyDocumentRaw,
    ) -> Result<(), String>;
    async fn delete_user_policy(&self, username: &str, policy_name: &str) -> Result<(), String>;
    async fn attach_user_policy(&self, username: &str, policy_arn: &str) -> Result<(), String>;
    async fn detach_user_policy(&self, username: &str, policy_arn: &str) -> Result<(), String>;
    async fn create_managed_policy(
        &self,
        name: &str,
        document: PolicyDocumentRaw,
    ) -> Result<ManagedPolicy, String>;
    async fn delete_managed_policy(&self, name: &str) -> Result<(), String>;
    async fn add_user_with_keys(
        &self,
        username: &str,
        access_key_id: &str,
        secret_access_key: &str,
    ) -> Result<IamUser, String>;
}
