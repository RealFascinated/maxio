use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use super::policy::{parse_policy_document, PolicyDocument};
use super::types::*;

pub struct UserStore {
    data: RwLock<StoreInner>,
    path: PathBuf,
}

struct StoreInner {
    users: HashMap<String, IamUser>,
    managed_policies: HashMap<String, ManagedPolicy>,
    key_index: HashMap<String, String>,
}

impl UserStore {
    pub async fn load(data_dir: &str) -> Result<Self, std::io::Error> {
        let path = Path::new(data_dir).join(".maxio-users.json");
        let users_data = if path.exists() {
            let raw = tokio::fs::read_to_string(&path).await?;
            serde_json::from_str(&raw).unwrap_or_default()
        } else {
            UsersData::default()
        };
        let store = Self {
            data: RwLock::new(StoreInner {
                users: users_data.users,
                managed_policies: users_data.managed_policies,
                key_index: HashMap::new(),
            }),
            path,
        };
        store.rebuild_key_index();
        Ok(store)
    }

    fn rebuild_key_index(&self) {
        let mut data = self.data.write().unwrap();
        data.key_index.clear();
        let users: Vec<(String, IamUser)> = data
            .users
            .iter()
            .map(|(u, user)| (u.clone(), user.clone()))
            .collect();
        for (username, user) in users {
            for key in &user.access_keys {
                if key.status == KeyStatus::Active {
                    data.key_index
                        .insert(key.access_key_id.clone(), username.clone());
                }
            }
        }
    }

    async fn persist(&self) -> Result<(), std::io::Error> {
        let users_data = {
            let data = self.data.read().unwrap();
            UsersData {
                users: data.users.clone(),
                managed_policies: data.managed_policies.clone(),
            }
        };
        let json = serde_json::to_string_pretty(&users_data).map_err(std::io::Error::other)?;
        let tmp = self.path.with_extension("json.tmp");
        tokio::fs::write(&tmp, &json).await?;
        tokio::fs::rename(&tmp, &self.path).await?;
        Ok(())
    }

    pub fn lookup_by_access_key(&self, access_key_id: &str) -> Option<(IamUser, AccessKey)> {
        let data = self.data.read().unwrap();
        let username = data.key_index.get(access_key_id)?;
        let user = data.users.get(username)?;
        let key = user
            .access_keys
            .iter()
            .find(|k| k.access_key_id == access_key_id && k.status == KeyStatus::Active)?;
        Some((user.clone(), key.clone()))
    }

    pub fn lookup_by_credentials(
        &self,
        access_key_id: &str,
        secret_access_key: &str,
    ) -> Option<IamUser> {
        let (user, key) = self.lookup_by_access_key(access_key_id)?;
        if crate::auth::signature_v4::constant_time_eq(
            secret_access_key.as_bytes(),
            key.secret_access_key.as_bytes(),
        ) {
            Some(user)
        } else {
            None
        }
    }

    pub fn get_user(&self, username: &str) -> Option<IamUser> {
        self.data.read().unwrap().users.get(username).cloned()
    }

    pub fn list_users(&self) -> Vec<IamUser> {
        let data = self.data.read().unwrap();
        let mut users: Vec<_> = data.users.values().cloned().collect();
        users.sort_by(|a, b| a.username.cmp(&b.username));
        users
    }

    pub fn effective_policies(&self, user: &IamUser) -> Vec<PolicyDocument> {
        let data = self.data.read().unwrap();
        let mut docs = Vec::new();
        for inline in &user.inline_policies {
            if let Ok(doc) = parse_policy_document(&inline.document) {
                docs.push(doc);
            }
        }
        for arn in &user.attached_policies {
            for policy in data.managed_policies.values() {
                if policy.arn == *arn {
                    if let Ok(doc) = parse_policy_document(&policy.document) {
                        docs.push(doc);
                    }
                }
            }
        }
        docs
    }

    pub fn get_managed_policy(&self, name: &str) -> Option<ManagedPolicy> {
        self.data.read().unwrap().managed_policies.get(name).cloned()
    }

    pub fn list_managed_policies(&self) -> Vec<ManagedPolicy> {
        let data = self.data.read().unwrap();
        let mut policies: Vec<_> = data.managed_policies.values().cloned().collect();
        policies.sort_by(|a, b| a.policy_name.cmp(&b.policy_name));
        policies
    }

    pub async fn create_user(&self, username: &str) -> Result<IamUser, String> {
        if username.is_empty() || username == super::principal::ROOT_USERNAME {
            return Err("invalid username".into());
        }
        {
            let data = self.data.read().unwrap();
            if data.users.contains_key(username) {
                return Err(format!("user already exists: {username}"));
            }
        }
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let user = IamUser {
            user_id: generate_user_id(),
            username: username.to_string(),
            created_at: now,
            access_keys: vec![],
            inline_policies: vec![],
            attached_policies: vec![],
        };
        {
            let mut data = self.data.write().unwrap();
            data.users.insert(username.to_string(), user.clone());
        }
        self.persist().await.map_err(|e| e.to_string())?;
        Ok(user)
    }

    pub async fn delete_user(&self, username: &str) -> Result<(), String> {
        {
            let mut data = self.data.write().unwrap();
            if data.users.remove(username).is_none() {
                return Err(format!("user not found: {username}"));
            }
            data.key_index.retain(|_, u| u != username);
        }
        self.persist().await.map_err(|e| e.to_string())
    }

    pub async fn create_access_key(&self, username: &str) -> Result<AccessKey, String> {
        let key = AccessKey {
            access_key_id: generate_access_key_id(),
            secret_access_key: generate_secret_access_key(),
            status: KeyStatus::Active,
            created_at: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
        };
        {
            let mut data = self.data.write().unwrap();
            let user = data
                .users
                .get_mut(username)
                .ok_or_else(|| format!("user not found: {username}"))?;
            user.access_keys.push(key.clone());
            data.key_index
                .insert(key.access_key_id.clone(), username.to_string());
        }
        self.persist().await.map_err(|e| e.to_string())?;
        Ok(key)
    }

    pub async fn delete_access_key(&self, username: &str, access_key_id: &str) -> Result<(), String> {
        {
            let mut data = self.data.write().unwrap();
            let user = data
                .users
                .get_mut(username)
                .ok_or_else(|| format!("user not found: {username}"))?;
            let before = user.access_keys.len();
            user.access_keys
                .retain(|k| k.access_key_id != access_key_id);
            if user.access_keys.len() == before {
                return Err(format!("access key not found: {access_key_id}"));
            }
            data.key_index.remove(access_key_id);
        }
        self.persist().await.map_err(|e| e.to_string())
    }

    pub async fn update_access_key_status(
        &self,
        username: &str,
        access_key_id: &str,
        status: KeyStatus,
    ) -> Result<(), String> {
        {
            let mut data = self.data.write().unwrap();
            let user = data
                .users
                .get_mut(username)
                .ok_or_else(|| format!("user not found: {username}"))?;
            let key = user
                .access_keys
                .iter_mut()
                .find(|k| k.access_key_id == access_key_id)
                .ok_or_else(|| format!("access key not found: {access_key_id}"))?;
            key.status = status;
            data.key_index.remove(access_key_id);
            if status == KeyStatus::Active {
                data.key_index
                    .insert(access_key_id.to_string(), username.to_string());
            }
        }
        self.persist().await.map_err(|e| e.to_string())
    }

    pub async fn put_user_policy(
        &self,
        username: &str,
        policy_name: &str,
        document: PolicyDocumentRaw,
    ) -> Result<(), String> {
        {
            let mut data = self.data.write().unwrap();
            let user = data
                .users
                .get_mut(username)
                .ok_or_else(|| format!("user not found: {username}"))?;
            if let Some(existing) = user
                .inline_policies
                .iter_mut()
                .find(|p| p.policy_name == policy_name)
            {
                existing.document = document;
            } else {
                user.inline_policies.push(InlinePolicy {
                    policy_name: policy_name.to_string(),
                    document,
                });
            }
        }
        self.persist().await.map_err(|e| e.to_string())
    }

    pub async fn delete_user_policy(&self, username: &str, policy_name: &str) -> Result<(), String> {
        {
            let mut data = self.data.write().unwrap();
            let user = data
                .users
                .get_mut(username)
                .ok_or_else(|| format!("user not found: {username}"))?;
            let before = user.inline_policies.len();
            user.inline_policies
                .retain(|p| p.policy_name != policy_name);
            if user.inline_policies.len() == before {
                return Err(format!("policy not found: {policy_name}"));
            }
        }
        self.persist().await.map_err(|e| e.to_string())
    }

    pub async fn attach_user_policy(&self, username: &str, policy_arn: &str) -> Result<(), String> {
        {
            let data = self.data.read().unwrap();
            let exists = data.managed_policies.values().any(|p| p.arn == policy_arn);
            if !exists {
                return Err(format!("policy not found: {policy_arn}"));
            }
        }
        {
            let mut data = self.data.write().unwrap();
            let user = data
                .users
                .get_mut(username)
                .ok_or_else(|| format!("user not found: {username}"))?;
            if !user.attached_policies.contains(&policy_arn.to_string()) {
                user.attached_policies.push(policy_arn.to_string());
            }
        }
        self.persist().await.map_err(|e| e.to_string())
    }

    pub async fn detach_user_policy(&self, username: &str, policy_arn: &str) -> Result<(), String> {
        {
            let mut data = self.data.write().unwrap();
            let user = data
                .users
                .get_mut(username)
                .ok_or_else(|| format!("user not found: {username}"))?;
            let before = user.attached_policies.len();
            user.attached_policies.retain(|a| a != policy_arn);
            if user.attached_policies.len() == before {
                return Err(format!("policy not attached: {policy_arn}"));
            }
        }
        self.persist().await.map_err(|e| e.to_string())
    }

    pub async fn create_managed_policy(
        &self,
        name: &str,
        document: PolicyDocumentRaw,
    ) -> Result<ManagedPolicy, String> {
        {
            let data = self.data.read().unwrap();
            if data.managed_policies.contains_key(name) {
                return Err(format!("policy already exists: {name}"));
            }
        }
        let policy = ManagedPolicy {
            policy_id: generate_policy_id(),
            policy_name: name.to_string(),
            arn: managed_policy_arn(name),
            created_at: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            document,
        };
        {
            let mut data = self.data.write().unwrap();
            data.managed_policies
                .insert(name.to_string(), policy.clone());
        }
        self.persist().await.map_err(|e| e.to_string())?;
        Ok(policy)
    }

    pub async fn delete_managed_policy(&self, name: &str) -> Result<(), String> {
        let arn = managed_policy_arn(name);
        {
            let mut data = self.data.write().unwrap();
            if data.managed_policies.remove(name).is_none() {
                return Err(format!("policy not found: {name}"));
            }
            for user in data.users.values_mut() {
                user.attached_policies.retain(|a| a != &arn);
            }
        }
        self.persist().await.map_err(|e| e.to_string())
    }

    pub async fn add_user_with_keys(
        &self,
        username: &str,
        access_key_id: &str,
        secret_access_key: &str,
    ) -> Result<IamUser, String> {
        self.create_user(username).await?;
        let key = AccessKey {
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            status: KeyStatus::Active,
            created_at: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
        };
        {
            let mut data = self.data.write().unwrap();
            let user = data
                .users
                .get_mut(username)
                .ok_or_else(|| format!("user not found: {username}"))?;
            user.access_keys.push(key);
            data.key_index
                .insert(access_key_id.to_string(), username.to_string());
        }
        self.persist().await.map_err(|e| e.to_string())?;
        self.get_user(username).ok_or_else(|| "user not found".into())
    }
}
