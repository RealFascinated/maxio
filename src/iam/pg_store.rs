use std::sync::Arc;

use async_trait::async_trait;

use crate::db::repos::IamRepo;
use crate::db::DbPool;

use super::iam_store::IamStore;
use super::policy::PolicyDocument;
use super::types::*;

pub struct PgIamStore {
    repo: IamRepo,
}

impl PgIamStore {
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self {
            repo: IamRepo::new((*pool).clone()),
        }
    }
}

#[async_trait]
impl IamStore for PgIamStore {
    async fn lookup_by_access_key(
        &self,
        access_key_id: &str,
    ) -> Option<(IamUser, AccessKey)> {
        self.repo
            .lookup_by_access_key(access_key_id)
            .await
            .ok()
            .flatten()
    }

    async fn lookup_by_credentials(
        &self,
        access_key_id: &str,
        secret_access_key: &str,
    ) -> Option<IamUser> {
        self.repo
            .lookup_by_credentials(access_key_id, secret_access_key)
            .await
            .ok()
            .flatten()
    }

    async fn get_user(&self, username: &str) -> Option<IamUser> {
        self.repo.get_user(username).await.ok().flatten()
    }

    async fn list_users(&self) -> Vec<IamUser> {
        self.repo.list_users().await.unwrap_or_default()
    }

    async fn effective_policies(&self, user: &IamUser) -> Vec<PolicyDocument> {
        self.repo
            .effective_policies(user)
            .await
            .unwrap_or_default()
    }

    async fn get_managed_policy(&self, name: &str) -> Option<ManagedPolicy> {
        self.repo.get_managed_policy(name).await.ok().flatten()
    }

    async fn list_managed_policies(&self) -> Vec<ManagedPolicy> {
        self.repo.list_managed_policies().await.unwrap_or_default()
    }

    async fn create_user(&self, username: &str) -> Result<IamUser, String> {
        self.repo.create_user(username).await
    }

    async fn delete_user(&self, username: &str) -> Result<(), String> {
        self.repo.delete_user(username).await
    }

    async fn create_access_key(&self, username: &str) -> Result<AccessKey, String> {
        self.repo.create_access_key(username).await
    }

    async fn delete_access_key(&self, username: &str, access_key_id: &str) -> Result<(), String> {
        self.repo.delete_access_key(username, access_key_id).await
    }

    async fn update_access_key_status(
        &self,
        username: &str,
        access_key_id: &str,
        status: KeyStatus,
    ) -> Result<(), String> {
        self.repo
            .update_access_key_status(username, access_key_id, status)
            .await
    }

    async fn put_user_policy(
        &self,
        username: &str,
        policy_name: &str,
        document: PolicyDocumentRaw,
    ) -> Result<(), String> {
        self.repo
            .put_user_policy(username, policy_name, document)
            .await
    }

    async fn delete_user_policy(&self, username: &str, policy_name: &str) -> Result<(), String> {
        self.repo.delete_user_policy(username, policy_name).await
    }

    async fn attach_user_policy(&self, username: &str, policy_arn: &str) -> Result<(), String> {
        self.repo.attach_user_policy(username, policy_arn).await
    }

    async fn detach_user_policy(&self, username: &str, policy_arn: &str) -> Result<(), String> {
        self.repo.detach_user_policy(username, policy_arn).await
    }

    async fn create_managed_policy(
        &self,
        name: &str,
        document: PolicyDocumentRaw,
    ) -> Result<ManagedPolicy, String> {
        self.repo.create_managed_policy(name, document).await
    }

    async fn delete_managed_policy(&self, name: &str) -> Result<(), String> {
        self.repo.delete_managed_policy(name).await
    }

    async fn add_user_with_keys(
        &self,
        username: &str,
        access_key_id: &str,
        secret_access_key: &str,
    ) -> Result<IamUser, String> {
        self.repo
            .add_user_with_keys(username, access_key_id, secret_access_key)
            .await
    }
}
