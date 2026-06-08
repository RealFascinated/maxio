use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::auth::signing_key_cache::SigningKeyCache;
use crate::iam::iam_store::IamStore;
use crate::iam::policy::PolicyDocument;
use crate::iam::types::*;
use crate::metrics::{MetricsRegistry, cache_name};

struct KeyEntry {
    user: Arc<IamUser>,
    key: AccessKey,
    expires_at: Instant,
}

struct UserEntry {
    user: Arc<IamUser>,
    expires_at: Instant,
}

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
    key_cache: RwLock<HashMap<String, KeyEntry>>,
    user_cache: RwLock<HashMap<String, UserEntry>>,
    policies_cache: RwLock<HashMap<String, PoliciesEntry>>,
    signing_keys: Arc<SigningKeyCache>,
    metrics: Option<Arc<MetricsRegistry>>,
}

impl CachingIamStore {
    pub fn new(
        inner: Arc<dyn IamStore>,
        ttl: Duration,
        signing_keys: Arc<SigningKeyCache>,
        metrics: Option<Arc<MetricsRegistry>>,
    ) -> Self {
        Self {
            inner,
            ttl,
            key_cache: RwLock::new(HashMap::new()),
            user_cache: RwLock::new(HashMap::new()),
            policies_cache: RwLock::new(HashMap::new()),
            signing_keys,
            metrics,
        }
    }

    fn evict_key(&self, access_key_id: &str) {
        if let Ok(mut cache) = self.key_cache.write() {
            if cache.remove(access_key_id).is_some() {
                if let Some(m) = &self.metrics {
                    m.record_cache_eviction(cache_name::IAM_ACCESS_KEY);
                    m.set_cache_entries(cache_name::IAM_ACCESS_KEY, cache.len());
                }
            }
        }
        self.signing_keys.evict(access_key_id);
    }

    fn evict_user_policies(&self, username: &str) {
        if let Ok(mut cache) = self.policies_cache.write() {
            if cache.remove(username).is_some() {
                if let Some(m) = &self.metrics {
                    m.record_cache_eviction(cache_name::IAM_POLICIES);
                    m.set_cache_entries(cache_name::IAM_POLICIES, cache.len());
                }
            }
        }
    }

    fn sync_key_entries(&self, entries: usize) {
        if let Some(m) = &self.metrics {
            m.set_cache_entries(cache_name::IAM_ACCESS_KEY, entries);
        }
    }

    fn sync_user_entries(&self, entries: usize) {
        if let Some(m) = &self.metrics {
            m.set_cache_entries(cache_name::IAM_USER, entries);
        }
    }

    fn sync_policies_entries(&self, entries: usize) {
        if let Some(m) = &self.metrics {
            m.set_cache_entries(cache_name::IAM_POLICIES, entries);
        }
    }
}

#[async_trait]
impl IamStore for CachingIamStore {
    async fn lookup_by_access_key(&self, access_key_id: &str) -> Option<(IamUser, AccessKey)> {
        {
            let cache = self.key_cache.read().ok()?;
            if let Some(entry) = cache.get(access_key_id) {
                if entry.expires_at > Instant::now() {
                    if let Some(m) = &self.metrics {
                        m.record_cache_hit(cache_name::IAM_ACCESS_KEY);
                    }
                    return Some(((*entry.user).clone(), entry.key.clone()));
                }
            }
        }

        if let Some(m) = &self.metrics {
            m.record_cache_miss(cache_name::IAM_ACCESS_KEY);
        }
        let (user, key) = self.inner.lookup_by_access_key(access_key_id).await?;
        let expires_at = Instant::now() + self.ttl;
        let user_arc = Arc::new(user.clone());

        if let Ok(mut cache) = self.key_cache.write() {
            cache.insert(
                access_key_id.to_string(),
                KeyEntry {
                    user: Arc::clone(&user_arc),
                    key: key.clone(),
                    expires_at,
                },
            );
            self.sync_key_entries(cache.len());
        }
        if let Ok(mut cache) = self.user_cache.write() {
            cache.insert(
                user.username.clone(),
                UserEntry {
                    user: user_arc,
                    expires_at,
                },
            );
            self.sync_user_entries(cache.len());
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
        {
            if let Ok(cache) = self.user_cache.read() {
                if let Some(entry) = cache.get(username) {
                    if entry.expires_at > Instant::now() {
                        if let Some(m) = &self.metrics {
                            m.record_cache_hit(cache_name::IAM_USER);
                        }
                        return Some((*entry.user).clone());
                    }
                }
            }
        }

        if let Some(m) = &self.metrics {
            m.record_cache_miss(cache_name::IAM_USER);
        }
        let user = self.inner.get_user(username).await?;
        let expires_at = Instant::now() + self.ttl;
        if let Ok(mut cache) = self.user_cache.write() {
            cache.insert(
                username.to_string(),
                UserEntry {
                    user: Arc::new(user.clone()),
                    expires_at,
                },
            );
            self.sync_user_entries(cache.len());
        }
        Some(user)
    }

    async fn list_users(&self) -> Vec<IamUser> {
        self.inner.list_users().await
    }

    async fn effective_policies(&self, user: &IamUser) -> Vec<PolicyDocument> {
        {
            if let Ok(cache) = self.policies_cache.read() {
                if let Some(entry) = cache.get(&user.username) {
                    if entry.expires_at > Instant::now() {
                        if let Some(m) = &self.metrics {
                            m.record_cache_hit(cache_name::IAM_POLICIES);
                        }
                        return (*entry.policies).clone();
                    }
                }
            }
        }

        if let Some(m) = &self.metrics {
            m.record_cache_miss(cache_name::IAM_POLICIES);
        }
        let policies = self.inner.effective_policies(user).await;
        let expires_at = Instant::now() + self.ttl;
        if let Ok(mut cache) = self.policies_cache.write() {
            cache.insert(
                user.username.clone(),
                PoliciesEntry {
                    policies: Arc::new(policies.clone()),
                    expires_at,
                },
            );
            self.sync_policies_entries(cache.len());
        }
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
