use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum KeyStatus {
    Active,
    Inactive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessKey {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub status: KeyStatus,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlinePolicy {
    pub policy_name: String,
    pub document: PolicyDocumentRaw,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IamUser {
    pub user_id: String,
    pub username: String,
    pub created_at: String,
    pub access_keys: Vec<AccessKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline_policies: Vec<InlinePolicy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attached_policies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedPolicy {
    pub policy_id: String,
    pub policy_name: String,
    pub arn: String,
    pub created_at: String,
    pub document: PolicyDocumentRaw,
}

/// Raw policy document as stored in JSON (AWS IAM format).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PolicyDocumentRaw {
    #[serde(default = "default_policy_version")]
    pub version: String,
    #[serde(default)]
    pub statement: Vec<StatementRaw>,
}

fn default_policy_version() -> String {
    "2012-10-17".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StatementRaw {
    #[serde(default)]
    pub sid: Option<String>,
    pub effect: String,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub action: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub resource: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal: Option<PrincipalSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PrincipalSpec {
    Star(String),
    Map(PrincipalMap),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrincipalMap {
    #[serde(default, rename = "AWS")]
    pub aws: Vec<String>,
    #[serde(default, rename = "*")]
    pub star: bool,
}

fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(vec![s]),
        serde_json::Value::Array(arr) => arr
            .into_iter()
            .map(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| D::Error::custom("expected string in array"))
            })
            .collect(),
        serde_json::Value::Null => Ok(vec![]),
        _ => Err(D::Error::custom("expected string or array")),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsersData {
    #[serde(default)]
    pub users: std::collections::HashMap<String, IamUser>,
    #[serde(default)]
    pub managed_policies: std::collections::HashMap<String, ManagedPolicy>,
}

pub fn managed_policy_arn(name: &str) -> String {
    format!("arn:aws:iam::{}:policy/{}", super::principal::ACCOUNT_ID, name)
}

pub fn generate_user_id() -> String {
    format!("AIDA{}", uuid::Uuid::new_v4().simple().to_string()[..16].to_uppercase())
}

pub fn generate_policy_id() -> String {
    format!("ANPA{}", uuid::Uuid::new_v4().simple().to_string()[..16].to_uppercase())
}

pub fn generate_access_key_id() -> String {
    format!("AKIA{}", uuid::Uuid::new_v4().simple().to_string()[..16].to_uppercase())
}

pub fn generate_secret_access_key() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 30];
    rand::rng().fill_bytes(&mut bytes);
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
}
