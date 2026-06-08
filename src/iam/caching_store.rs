use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::auth::signing_key_cache::SigningKeyCache;
use crate::iam::iam_store::IamStore;
use crate::iam::policy::PolicyDocument;
use crate::iam::types::*;

struct Entry {
    user: IamUser,
    key: AccessKey,
    expires_at: Instant,
}

/// Caches `lookup_by_access_key` results with a configurable TTL to avoid a
/// Postgres round-trip on every authenticated S3 request.
///
/// Key eviction also clears the companion `SigningKeyCache` so that a deactivated
/// or deleted key stops working within one TTL window.
pub struct CachingIamStore {
    inner: Arc<dyn IamStore>,
    ttl: Duration,
    cache: RwLock<HashMap<String, Entry>>,
    signing_keys: Arc<SigningKeyCache>,
}

impl CachingIamStore {
    pub fn new(
        inner: Arc<dyn IamStore>,
        ttl: Duration,
        signing_keys: Arc<SigningKeyCache>,
    ) -> Self {
        Self {
            inner,
            ttl,
            cache: RwLock::new(HashMap::new()),
            signing_keys,
        }
    }

    fn evict(&self, access_key_id: &str) {
        if let Ok(mut cache) = self.cache.write() {
            cache.remove(access_key_id);
        }
        self.signing_keys.evict(access_key_id);
    }
}

#[async_trait]
impl IamStore for CachingIamStore {
    async fn lookup_by_access_key(&self, access_key_id: &str) -> Option<(IamUser, AccessKey)> {
        {
            let cache = self.cache.read().ok()?;
            if let Some(entry) = cache.get(access_key_id) {
                if entry.expires_at > Instant::now() {
                    return Some((entry.user.clone(), entry.key.clone()));
                }
            }
        }

        let (user, key) = self.inner.lookup_by_access_key(access_key_id).await?;
        let expires_at = Instant::now() + self.ttl;
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(
                access_key_id.to_string(),
                Entry {
                    user: user.clone(),
                    key: key.clone(),
                    expires_at,
                },
            );
        }
        Some((user, key))
    }

    async fn lookup_by_credentials(
        &self,
        access_key_id: &str,
        secret_access_key: &str,
    ) -> Option<IamUser> {
        self.inner
            .lookup_by_credentials(access_key_id, secret_access_key)
            .await
    }

    async fn get_user(&self, username: &str) -> Option<IamUser> {
        self.inner.get_user(username).await
    }

    async fn list_users(&self) -> Vec<IamUser> {
        self.inner.list_users().await
    }

    async fn effective_policies(&self, user: &IamUser) -> Vec<PolicyDocument> {
        self.inner.effective_policies(user).await
    }

    async fn get_managed_policy(&self, name: &str) -> Option<ManagedPolicy> {
        self.inner.get_managed_policy(name).await
    }

    async fn list_managed_policies(&self) -> Vec<ManagedPolicy> {
        self.inner.list_managed_policies().await
    }

    async fn create_user(&self, username: &str) -> Result<IamUser, String> {
        self.inner.create_user(username).await
    }

    async fn delete_user(&self, username: &str) -> Result<(), String> {
        self.inner.delete_user(username).await
    }

    async fn create_access_key(&self, username: &str) -> Result<AccessKey, String> {
        self.inner.create_access_key(username).await
    }

    async fn delete_access_key(&self, username: &str, access_key_id: &str) -> Result<(), String> {
        let result = self.inner.delete_access_key(username, access_key_id).await;
        if result.is_ok() {
            self.evict(access_key_id);
        }
        result
    }

    async fn update_access_key_status(
        &self,
        username: &str,
        access_key_id: &str,
        status: KeyStatus,
    ) -> Result<(), String> {
        let result = self
            .inner
            .update_access_key_status(username, access_key_id, status)
            .await;
        if result.is_ok() {
            self.evict(access_key_id);
        }
        result
    }

    async fn put_user_policy(
        &self,
        username: &str,
        policy_name: &str,
        document: PolicyDocumentRaw,
    ) -> Result<(), String> {
        self.inner
            .put_user_policy(username, policy_name, document)
            .await
    }

    async fn delete_user_policy(&self, username: &str, policy_name: &str) -> Result<(), String> {
        self.inner.delete_user_policy(username, policy_name).await
    }

    async fn attach_user_policy(&self, username: &str, policy_arn: &str) -> Result<(), String> {
        self.inner.attach_user_policy(username, policy_arn).await
    }

    async fn detach_user_policy(&self, username: &str, policy_arn: &str) -> Result<(), String> {
        self.inner.detach_user_policy(username, policy_arn).await
    }

    async fn create_managed_policy(
        &self,
        name: &str,
        document: PolicyDocumentRaw,
    ) -> Result<ManagedPolicy, String> {
        self.inner.create_managed_policy(name, document).await
    }

    async fn delete_managed_policy(&self, name: &str) -> Result<(), String> {
        self.inner.delete_managed_policy(name).await
    }

    async fn add_user_with_keys(
        &self,
        username: &str,
        access_key_id: &str,
        secret_access_key: &str,
    ) -> Result<IamUser, String> {
        self.inner
            .add_user_with_keys(username, access_key_id, secret_access_key)
            .await
    }
}
