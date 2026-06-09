use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::auth::signing_key_cache::SigningKeyCache;
use crate::cache::MetricsLruCache;
use crate::iam::iam_store::IamStore;
use crate::iam::policy::PolicyDocument;
use crate::iam::types::*;
use crate::metrics::{MetricsRegistry, cache_name};

#[derive(Clone)]
struct KeyEntry {
    user: Arc<IamUser>,
    key: AccessKey,
    expires_at: Instant,
}

#[derive(Clone)]
struct UserEntry {
    user: Arc<IamUser>,
    expires_at: Instant,
}

#[derive(Clone)]
struct PoliciesEntry {
    policies: Arc<Vec<PolicyDocument>>,
    expires_at: Instant,
}

/// Caches `lookup_by_access_key`, `get_user`, and `effective_policies` results with a
/// configurable TTL to avoid Postgres round-trips on every authenticated S3 request.
///
/// Policy mutations (`put_user_policy`, `delete_user_policy`, `attach_user_policy`,
/// `detach_user_policy`) invalidate the per-user policy cache entry immediately.
///
/// Key eviction also clears the companion `SigningKeyCache` so that a deactivated
/// or deleted key stops working within one TTL window.
pub struct CachingIamStore {
    inner: Arc<dyn IamStore>,
    ttl: Duration,
    key_cache: MetricsLruCache<String, KeyEntry>,
    user_cache: MetricsLruCache<String, UserEntry>,
    policies_cache: MetricsLruCache<String, PoliciesEntry>,
    signing_keys: Arc<SigningKeyCache>,
}

impl CachingIamStore {
    pub fn new(
        inner: Arc<dyn IamStore>,
        ttl: Duration,
        signing_keys: Arc<SigningKeyCache>,
        metrics: Option<Arc<MetricsRegistry>>,
        max_entries: usize,
    ) -> Self {
        Self {
            inner,
            ttl,
            key_cache: MetricsLruCache::new(
                metrics.clone(),
                cache_name::IAM_ACCESS_KEY,
                max_entries,
            ),
            user_cache: MetricsLruCache::new(metrics.clone(), cache_name::IAM_USER, max_entries),
            policies_cache: MetricsLruCache::new(metrics, cache_name::IAM_POLICIES, max_entries),
            signing_keys,
        }
    }

    fn evict_key(&self, access_key_id: &str) {
        self.key_cache.remove(access_key_id);
        self.signing_keys.evict(access_key_id);
    }

    fn evict_user_policies(&self, username: &str) {
        self.policies_cache.remove(username);
    }

    fn entry_valid(expires_at: Instant) -> bool {
        expires_at > Instant::now()
    }
}

#[async_trait]
impl IamStore for CachingIamStore {
    async fn lookup_by_access_key(&self, access_key_id: &str) -> Option<(IamUser, AccessKey)> {
        if let Some(entry) = self
            .key_cache
            .get_if(access_key_id, |e| Self::entry_valid(e.expires_at))
        {
            return Some(((*entry.user).clone(), entry.key.clone()));
        }

        self.key_cache.record_miss();
        let (user, key) = self.inner.lookup_by_access_key(access_key_id).await?;
        let expires_at = Instant::now() + self.ttl;
        let user_arc = Arc::new(user.clone());

        self.key_cache.insert(
            access_key_id.to_string(),
            KeyEntry {
                user: Arc::clone(&user_arc),
                key: key.clone(),
                expires_at,
            },
        );
        self.user_cache.insert(
            user.username.clone(),
            UserEntry {
                user: user_arc,
                expires_at,
            },
        );
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
        if let Some(entry) = self
            .user_cache
            .get_if(username, |e| Self::entry_valid(e.expires_at))
        {
            return Some((*entry.user).clone());
        }

        self.user_cache.record_miss();
        let user = self.inner.get_user(username).await?;
        let expires_at = Instant::now() + self.ttl;
        self.user_cache.insert(
            username.to_string(),
            UserEntry {
                user: Arc::new(user.clone()),
                expires_at,
            },
        );
        Some(user)
    }

    async fn list_users(&self) -> Vec<IamUser> {
        self.inner.list_users().await
    }

    async fn effective_policies(&self, user: &IamUser) -> Vec<PolicyDocument> {
        if let Some(entry) = self
            .policies_cache
            .get_if(&user.username, |e| Self::entry_valid(e.expires_at))
        {
            return (*entry.policies).clone();
        }

        self.policies_cache.record_miss();
        let policies = self.inner.effective_policies(user).await;
        let expires_at = Instant::now() + self.ttl;
        self.policies_cache.insert(
            user.username.clone(),
            PoliciesEntry {
                policies: Arc::new(policies.clone()),
                expires_at,
            },
        );
        policies
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
            self.evict_key(access_key_id);
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
            self.evict_key(access_key_id);
        }
        result
    }

    async fn put_user_policy(
        &self,
        username: &str,
        policy_name: &str,
        document: PolicyDocumentRaw,
    ) -> Result<(), String> {
        let result = self
            .inner
            .put_user_policy(username, policy_name, document)
            .await;
        if result.is_ok() {
            self.evict_user_policies(username);
        }
        result
    }

    async fn delete_user_policy(&self, username: &str, policy_name: &str) -> Result<(), String> {
        let result = self.inner.delete_user_policy(username, policy_name).await;
        if result.is_ok() {
            self.evict_user_policies(username);
        }
        result
    }

    async fn attach_user_policy(&self, username: &str, policy_arn: &str) -> Result<(), String> {
        let result = self.inner.attach_user_policy(username, policy_arn).await;
        if result.is_ok() {
            self.evict_user_policies(username);
        }
        result
    }

    async fn detach_user_policy(&self, username: &str, policy_arn: &str) -> Result<(), String> {
        let result = self.inner.detach_user_policy(username, policy_arn).await;
        if result.is_ok() {
            self.evict_user_policies(username);
        }
        result
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
