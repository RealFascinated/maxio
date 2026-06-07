use crate::db::DbPool;
use crate::db::schema::{
    iam_access_keys, iam_managed_policies, iam_managed_policy_statements, iam_user_inline_policies,
    iam_user_policy_attachments, iam_users,
};
use crate::iam::policy::{PolicyDocument, parse_policy_document};
use crate::iam::types::{
    AccessKey, IamUser, InlinePolicy, KeyStatus, ManagedPolicy, PolicyDocumentRaw, StatementRaw,
    generate_access_key_id, generate_policy_id, generate_secret_access_key, generate_user_id,
    managed_policy_arn,
};
use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use uuid::Uuid;

use super::{db_err, format_ts, get_conn, parse_ts};
use crate::storage::StorageError;

pub struct IamRepo {
    pool: DbPool,
}

impl IamRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn lookup_by_access_key(
        &self,
        access_key_id: &str,
    ) -> Result<Option<(IamUser, AccessKey)>, StorageError> {
        let mut conn = get_conn(&self.pool).await?;

        let row: Option<(String, String, String, String, chrono::DateTime<Utc>)> =
            iam_access_keys::table
                .inner_join(iam_users::table)
                .filter(iam_access_keys::access_key_id.eq(access_key_id))
                .filter(iam_access_keys::status.eq("Active"))
                .select((
                    iam_users::username,
                    iam_users::user_id,
                    iam_access_keys::access_key_id,
                    iam_access_keys::secret_access_key,
                    iam_access_keys::created_at,
                ))
                .first(&mut conn)
                .await
                .optional()
                .map_err(db_err)?;

        let Some((username, user_id, key_id, secret, created_at)) = row else {
            return Ok(None);
        };

        let user = self.load_user(&mut conn, &username, user_id).await?;
        let key = AccessKey {
            access_key_id: key_id,
            secret_access_key: secret,
            status: KeyStatus::Active,
            created_at: format_ts(created_at),
        };
        Ok(Some((user, key)))
    }

    pub async fn lookup_by_credentials(
        &self,
        access_key_id: &str,
        secret_access_key: &str,
    ) -> Result<Option<IamUser>, StorageError> {
        let Some((user, key)) = self.lookup_by_access_key(access_key_id).await? else {
            return Ok(None);
        };
        if crate::auth::signature_v4::constant_time_eq(
            secret_access_key.as_bytes(),
            key.secret_access_key.as_bytes(),
        ) {
            Ok(Some(user))
        } else {
            Ok(None)
        }
    }

    pub async fn get_user(&self, username: &str) -> Result<Option<IamUser>, StorageError> {
        let mut conn = get_conn(&self.pool).await?;
        let row: Option<(String, chrono::DateTime<Utc>)> = iam_users::table
            .filter(iam_users::username.eq(username))
            .select((iam_users::user_id, iam_users::created_at))
            .first(&mut conn)
            .await
            .optional()
            .map_err(db_err)?;

        let Some((user_id, _)) = row else {
            return Ok(None);
        };
        Ok(Some(self.load_user(&mut conn, username, user_id).await?))
    }

    pub async fn list_users(&self) -> Result<Vec<IamUser>, StorageError> {
        let mut conn = get_conn(&self.pool).await?;
        let rows: Vec<(String, String)> = iam_users::table
            .select((iam_users::username, iam_users::user_id))
            .order(iam_users::username.asc())
            .load(&mut conn)
            .await
            .map_err(db_err)?;

        let mut users = Vec::with_capacity(rows.len());
        for (username, user_id) in rows {
            users.push(self.load_user(&mut conn, &username, user_id).await?);
        }
        Ok(users)
    }

    pub async fn effective_policies(
        &self,
        user: &IamUser,
    ) -> Result<Vec<PolicyDocument>, StorageError> {
        let mut docs = Vec::new();
        for inline in &user.inline_policies {
            if let Ok(doc) = parse_policy_document(&inline.document) {
                docs.push(doc);
            }
        }
        for arn in &user.attached_policies {
            if let Some(policy) = self.get_managed_policy_by_arn(arn).await? {
                if let Ok(doc) = parse_policy_document(&policy.document) {
                    docs.push(doc);
                }
            }
        }
        Ok(docs)
    }

    pub async fn get_managed_policy(
        &self,
        name: &str,
    ) -> Result<Option<ManagedPolicy>, StorageError> {
        let mut conn = get_conn(&self.pool).await?;
        let row: Option<(String, String, String, chrono::DateTime<Utc>)> =
            iam_managed_policies::table
                .filter(iam_managed_policies::policy_name.eq(name))
                .select((
                    iam_managed_policies::policy_id,
                    iam_managed_policies::policy_name,
                    iam_managed_policies::arn,
                    iam_managed_policies::created_at,
                ))
                .first(&mut conn)
                .await
                .optional()
                .map_err(db_err)?;

        let Some((policy_id, policy_name, arn, created_at)) = row else {
            return Ok(None);
        };

        let document = load_managed_policy_document(&mut conn, &policy_name).await?;
        Ok(Some(ManagedPolicy {
            policy_id,
            policy_name,
            arn,
            created_at: format_ts(created_at),
            document,
        }))
    }

    pub async fn list_managed_policies(&self) -> Result<Vec<ManagedPolicy>, StorageError> {
        let mut conn = get_conn(&self.pool).await?;
        let rows: Vec<(String, String, String, chrono::DateTime<Utc>)> =
            iam_managed_policies::table
                .select((
                    iam_managed_policies::policy_id,
                    iam_managed_policies::policy_name,
                    iam_managed_policies::arn,
                    iam_managed_policies::created_at,
                ))
                .order(iam_managed_policies::policy_name.asc())
                .load(&mut conn)
                .await
                .map_err(db_err)?;

        let mut policies = Vec::with_capacity(rows.len());
        for (policy_id, policy_name, arn, created_at) in rows {
            let document = load_managed_policy_document(&mut conn, &policy_name).await?;
            policies.push(ManagedPolicy {
                policy_id,
                policy_name,
                arn,
                created_at: format_ts(created_at),
                document,
            });
        }
        Ok(policies)
    }

    pub async fn create_user(&self, username: &str) -> Result<IamUser, String> {
        if username.is_empty() || username == crate::iam::principal::ROOT_USERNAME {
            return Err("invalid username".into());
        }

        let mut conn = get_conn(&self.pool).await.map_err(|e| e.to_string())?;
        let exists = diesel::select(diesel::dsl::exists(
            iam_users::table.filter(iam_users::username.eq(username)),
        ))
        .get_result::<bool>(&mut conn)
        .await
        .map_err(|e| e.to_string())?;

        if exists {
            return Err(format!("user already exists: {username}"));
        }

        let now = Utc::now();
        let user_id = generate_user_id();
        diesel::insert_into(iam_users::table)
            .values((
                iam_users::username.eq(username),
                iam_users::user_id.eq(&user_id),
                iam_users::created_at.eq(now),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| e.to_string())?;

        Ok(IamUser {
            user_id,
            username: username.to_string(),
            created_at: format_ts(now),
            access_keys: vec![],
            inline_policies: vec![],
            attached_policies: vec![],
        })
    }

    pub async fn delete_user(&self, username: &str) -> Result<(), String> {
        let mut conn = get_conn(&self.pool).await.map_err(|e| e.to_string())?;
        let deleted = diesel::delete(iam_users::table.filter(iam_users::username.eq(username)))
            .execute(&mut conn)
            .await
            .map_err(|e| e.to_string())?;

        if deleted == 0 {
            return Err(format!("user not found: {username}"));
        }
        Ok(())
    }

    pub async fn create_access_key(&self, username: &str) -> Result<AccessKey, String> {
        let key = AccessKey {
            access_key_id: generate_access_key_id(),
            secret_access_key: generate_secret_access_key(),
            status: KeyStatus::Active,
            created_at: format_ts(Utc::now()),
        };

        let mut conn = get_conn(&self.pool).await.map_err(|e| e.to_string())?;
        let exists = diesel::select(diesel::dsl::exists(
            iam_users::table.filter(iam_users::username.eq(username)),
        ))
        .get_result::<bool>(&mut conn)
        .await
        .map_err(|e| e.to_string())?;

        if !exists {
            return Err(format!("user not found: {username}"));
        }

        diesel::insert_into(iam_access_keys::table)
            .values((
                iam_access_keys::access_key_id.eq(&key.access_key_id),
                iam_access_keys::user_username.eq(username),
                iam_access_keys::secret_access_key.eq(&key.secret_access_key),
                iam_access_keys::status.eq("Active"),
                iam_access_keys::created_at
                    .eq(parse_ts(&key.created_at).map_err(|e| e.to_string())?),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| e.to_string())?;

        Ok(key)
    }

    pub async fn delete_access_key(
        &self,
        username: &str,
        access_key_id: &str,
    ) -> Result<(), String> {
        let mut conn = get_conn(&self.pool).await.map_err(|e| e.to_string())?;
        let deleted = diesel::delete(
            iam_access_keys::table
                .filter(iam_access_keys::user_username.eq(username))
                .filter(iam_access_keys::access_key_id.eq(access_key_id)),
        )
        .execute(&mut conn)
        .await
        .map_err(|e| e.to_string())?;

        if deleted == 0 {
            return Err(format!("access key not found: {access_key_id}"));
        }
        Ok(())
    }

    pub async fn update_access_key_status(
        &self,
        username: &str,
        access_key_id: &str,
        status: KeyStatus,
    ) -> Result<(), String> {
        let status_str = match status {
            KeyStatus::Active => "Active",
            KeyStatus::Inactive => "Inactive",
        };

        let mut conn = get_conn(&self.pool).await.map_err(|e| e.to_string())?;
        let updated = diesel::update(
            iam_access_keys::table
                .filter(iam_access_keys::user_username.eq(username))
                .filter(iam_access_keys::access_key_id.eq(access_key_id)),
        )
        .set(iam_access_keys::status.eq(status_str))
        .execute(&mut conn)
        .await
        .map_err(|e| e.to_string())?;

        if updated == 0 {
            return Err(format!("access key not found: {access_key_id}"));
        }
        Ok(())
    }

    pub async fn put_user_policy(
        &self,
        username: &str,
        policy_name: &str,
        document: PolicyDocumentRaw,
    ) -> Result<(), String> {
        let mut conn = get_conn(&self.pool).await.map_err(|e| e.to_string())?;
        let exists = diesel::select(diesel::dsl::exists(
            iam_users::table.filter(iam_users::username.eq(username)),
        ))
        .get_result::<bool>(&mut conn)
        .await
        .map_err(|e| e.to_string())?;

        if !exists {
            return Err(format!("user not found: {username}"));
        }

        let json = serde_json::to_value(&document).map_err(|e| e.to_string())?;
        diesel::insert_into(iam_user_inline_policies::table)
            .values((
                iam_user_inline_policies::user_username.eq(username),
                iam_user_inline_policies::policy_name.eq(policy_name),
                iam_user_inline_policies::document.eq(json.clone()),
            ))
            .on_conflict((
                iam_user_inline_policies::user_username,
                iam_user_inline_policies::policy_name,
            ))
            .do_update()
            .set(iam_user_inline_policies::document.eq(json))
            .execute(&mut conn)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn delete_user_policy(
        &self,
        username: &str,
        policy_name: &str,
    ) -> Result<(), String> {
        let mut conn = get_conn(&self.pool).await.map_err(|e| e.to_string())?;
        let deleted = diesel::delete(
            iam_user_inline_policies::table
                .filter(iam_user_inline_policies::user_username.eq(username))
                .filter(iam_user_inline_policies::policy_name.eq(policy_name)),
        )
        .execute(&mut conn)
        .await
        .map_err(|e| e.to_string())?;

        if deleted == 0 {
            return Err(format!("policy not found: {policy_name}"));
        }
        Ok(())
    }

    pub async fn attach_user_policy(&self, username: &str, policy_arn: &str) -> Result<(), String> {
        let mut conn = get_conn(&self.pool).await.map_err(|e| e.to_string())?;
        let policy_name: Option<String> = iam_managed_policies::table
            .filter(iam_managed_policies::arn.eq(policy_arn))
            .select(iam_managed_policies::policy_name)
            .first(&mut conn)
            .await
            .optional()
            .map_err(|e| e.to_string())?;

        let policy_name = policy_name.ok_or_else(|| format!("policy not found: {policy_arn}"))?;

        let user_exists = diesel::select(diesel::dsl::exists(
            iam_users::table.filter(iam_users::username.eq(username)),
        ))
        .get_result::<bool>(&mut conn)
        .await
        .map_err(|e| e.to_string())?;

        if !user_exists {
            return Err(format!("user not found: {username}"));
        }

        diesel::insert_into(iam_user_policy_attachments::table)
            .values((
                iam_user_policy_attachments::user_username.eq(username),
                iam_user_policy_attachments::policy_name.eq(&policy_name),
            ))
            .on_conflict((
                iam_user_policy_attachments::user_username,
                iam_user_policy_attachments::policy_name,
            ))
            .do_nothing()
            .execute(&mut conn)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn detach_user_policy(&self, username: &str, policy_arn: &str) -> Result<(), String> {
        let mut conn = get_conn(&self.pool).await.map_err(|e| e.to_string())?;
        let policy_name: Option<String> = iam_managed_policies::table
            .filter(iam_managed_policies::arn.eq(policy_arn))
            .select(iam_managed_policies::policy_name)
            .first(&mut conn)
            .await
            .optional()
            .map_err(|e| e.to_string())?;

        let policy_name =
            policy_name.ok_or_else(|| format!("policy not attached: {policy_arn}"))?;

        let deleted = diesel::delete(
            iam_user_policy_attachments::table
                .filter(iam_user_policy_attachments::user_username.eq(username))
                .filter(iam_user_policy_attachments::policy_name.eq(&policy_name)),
        )
        .execute(&mut conn)
        .await
        .map_err(|e| e.to_string())?;

        if deleted == 0 {
            return Err(format!("policy not attached: {policy_arn}"));
        }
        Ok(())
    }

    pub async fn create_managed_policy(
        &self,
        name: &str,
        document: PolicyDocumentRaw,
    ) -> Result<ManagedPolicy, String> {
        let mut conn = get_conn(&self.pool).await.map_err(|e| e.to_string())?;
        let exists = diesel::select(diesel::dsl::exists(
            iam_managed_policies::table.filter(iam_managed_policies::policy_name.eq(name)),
        ))
        .get_result::<bool>(&mut conn)
        .await
        .map_err(|e| e.to_string())?;

        if exists {
            return Err(format!("policy already exists: {name}"));
        }

        let now = Utc::now();
        let policy = ManagedPolicy {
            policy_id: generate_policy_id(),
            policy_name: name.to_string(),
            arn: managed_policy_arn(name),
            created_at: format_ts(now),
            document: document.clone(),
        };

        diesel::insert_into(iam_managed_policies::table)
            .values((
                iam_managed_policies::policy_name.eq(name),
                iam_managed_policies::policy_id.eq(&policy.policy_id),
                iam_managed_policies::arn.eq(&policy.arn),
                iam_managed_policies::created_at.eq(now),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| e.to_string())?;

        store_managed_policy_statements(&mut conn, name, &document).await?;
        Ok(policy)
    }

    pub async fn delete_managed_policy(&self, name: &str) -> Result<(), String> {
        let mut conn = get_conn(&self.pool).await.map_err(|e| e.to_string())?;
        let deleted = diesel::delete(
            iam_managed_policies::table.filter(iam_managed_policies::policy_name.eq(name)),
        )
        .execute(&mut conn)
        .await
        .map_err(|e| e.to_string())?;

        if deleted == 0 {
            return Err(format!("policy not found: {name}"));
        }
        Ok(())
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
            created_at: format_ts(Utc::now()),
        };

        let mut conn = get_conn(&self.pool).await.map_err(|e| e.to_string())?;
        diesel::insert_into(iam_access_keys::table)
            .values((
                iam_access_keys::access_key_id.eq(&key.access_key_id),
                iam_access_keys::user_username.eq(username),
                iam_access_keys::secret_access_key.eq(&key.secret_access_key),
                iam_access_keys::status.eq("Active"),
                iam_access_keys::created_at
                    .eq(parse_ts(&key.created_at).map_err(|e| e.to_string())?),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| e.to_string())?;

        self.get_user(username)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "user not found".into())
    }

    async fn get_managed_policy_by_arn(
        &self,
        arn: &str,
    ) -> Result<Option<ManagedPolicy>, StorageError> {
        let mut conn = get_conn(&self.pool).await?;
        let row: Option<(String, String, String, chrono::DateTime<Utc>)> =
            iam_managed_policies::table
                .filter(iam_managed_policies::arn.eq(arn))
                .select((
                    iam_managed_policies::policy_id,
                    iam_managed_policies::policy_name,
                    iam_managed_policies::arn,
                    iam_managed_policies::created_at,
                ))
                .first(&mut conn)
                .await
                .optional()
                .map_err(db_err)?;

        let Some((policy_id, policy_name, arn, created_at)) = row else {
            return Ok(None);
        };

        let document = load_managed_policy_document(&mut conn, &policy_name).await?;
        Ok(Some(ManagedPolicy {
            policy_id,
            policy_name,
            arn,
            created_at: format_ts(created_at),
            document,
        }))
    }

    async fn load_user(
        &self,
        conn: &mut diesel_async::AsyncPgConnection,
        username: &str,
        user_id: String,
    ) -> Result<IamUser, StorageError> {
        let created_at: chrono::DateTime<Utc> = iam_users::table
            .filter(iam_users::username.eq(username))
            .select(iam_users::created_at)
            .first(conn)
            .await
            .map_err(db_err)?;

        let key_rows: Vec<(String, String, String, chrono::DateTime<Utc>)> = iam_access_keys::table
            .filter(iam_access_keys::user_username.eq(username))
            .select((
                iam_access_keys::access_key_id,
                iam_access_keys::secret_access_key,
                iam_access_keys::status,
                iam_access_keys::created_at,
            ))
            .load(conn)
            .await
            .map_err(db_err)?;

        let access_keys = key_rows
            .into_iter()
            .map(
                |(access_key_id, secret_access_key, status, created)| AccessKey {
                    access_key_id,
                    secret_access_key,
                    status: match status.as_str() {
                        "Active" => KeyStatus::Active,
                        _ => KeyStatus::Inactive,
                    },
                    created_at: format_ts(created),
                },
            )
            .collect();

        let inline_rows: Vec<(String, serde_json::Value)> = iam_user_inline_policies::table
            .filter(iam_user_inline_policies::user_username.eq(username))
            .select((
                iam_user_inline_policies::policy_name,
                iam_user_inline_policies::document,
            ))
            .load(conn)
            .await
            .map_err(db_err)?;

        let inline_policies = inline_rows
            .into_iter()
            .filter_map(|(policy_name, document)| {
                serde_json::from_value::<PolicyDocumentRaw>(document)
                    .ok()
                    .map(|doc| InlinePolicy {
                        policy_name,
                        document: doc,
                    })
            })
            .collect();

        let attached_names: Vec<String> = iam_user_policy_attachments::table
            .filter(iam_user_policy_attachments::user_username.eq(username))
            .select(iam_user_policy_attachments::policy_name)
            .load(conn)
            .await
            .map_err(db_err)?;

        let attached_policies: Vec<String> = if attached_names.is_empty() {
            vec![]
        } else {
            iam_managed_policies::table
                .filter(iam_managed_policies::policy_name.eq_any(attached_names))
                .select(iam_managed_policies::arn)
                .load(conn)
                .await
                .map_err(db_err)?
        };

        Ok(IamUser {
            user_id,
            username: username.to_string(),
            created_at: format_ts(created_at),
            access_keys,
            inline_policies,
            attached_policies,
        })
    }
}

async fn load_managed_policy_document(
    conn: &mut diesel_async::AsyncPgConnection,
    policy_name: &str,
) -> Result<PolicyDocumentRaw, StorageError> {
    let rows: Vec<(
        Option<String>,
        String,
        Vec<String>,
        Vec<String>,
        Option<serde_json::Value>,
        Option<serde_json::Value>,
    )> = iam_managed_policy_statements::table
        .filter(iam_managed_policy_statements::policy_name.eq(policy_name))
        .select((
            iam_managed_policy_statements::sid,
            iam_managed_policy_statements::effect,
            iam_managed_policy_statements::actions,
            iam_managed_policy_statements::resources,
            iam_managed_policy_statements::principal,
            iam_managed_policy_statements::condition,
        ))
        .load(conn)
        .await
        .map_err(db_err)?;

    let statement = rows
        .into_iter()
        .map(
            |(sid, effect, actions, resources, principal, condition)| StatementRaw {
                sid,
                effect,
                action: actions,
                resource: resources,
                principal: principal.and_then(|v| serde_json::from_value(v).ok()),
                condition,
            },
        )
        .collect();

    Ok(PolicyDocumentRaw {
        version: "2012-10-17".to_string(),
        statement,
    })
}

async fn store_managed_policy_statements(
    conn: &mut diesel_async::AsyncPgConnection,
    policy_name: &str,
    document: &PolicyDocumentRaw,
) -> Result<(), String> {
    diesel::delete(
        iam_managed_policy_statements::table
            .filter(iam_managed_policy_statements::policy_name.eq(policy_name)),
    )
    .execute(conn)
    .await
    .map_err(|e| e.to_string())?;

    for stmt in &document.statement {
        diesel::insert_into(iam_managed_policy_statements::table)
            .values((
                iam_managed_policy_statements::id.eq(Uuid::new_v4()),
                iam_managed_policy_statements::policy_name.eq(policy_name),
                iam_managed_policy_statements::sid.eq(&stmt.sid),
                iam_managed_policy_statements::effect.eq(&stmt.effect),
                iam_managed_policy_statements::actions.eq(&stmt.action),
                iam_managed_policy_statements::resources.eq(&stmt.resource),
                iam_managed_policy_statements::principal.eq(stmt
                    .principal
                    .as_ref()
                    .and_then(|p| serde_json::to_value(p).ok())),
                iam_managed_policy_statements::condition.eq(&stmt.condition),
            ))
            .execute(conn)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
