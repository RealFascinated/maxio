use clap::Subcommand;

use crate::cli::load_iam_store;
use crate::iam::types::PolicyDocumentRaw;

#[derive(Subcommand, Debug)]
pub enum UserCmd {
    Add {
        #[arg(long)]
        username: String,
        #[arg(long)]
        access_key: Option<String>,
        #[arg(long)]
        secret_key: Option<String>,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    List {
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    Delete {
        #[arg(long)]
        username: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    CreateKey {
        #[arg(long)]
        username: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    DeleteKey {
        #[arg(long)]
        username: String,
        #[arg(long)]
        access_key_id: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    PutPolicy {
        #[arg(long)]
        username: String,
        #[arg(long)]
        policy_name: String,
        #[arg(long)]
        document: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    AttachPolicy {
        #[arg(long)]
        username: String,
        #[arg(long)]
        policy_arn: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
}

pub async fn run(cmd: UserCmd) -> anyhow::Result<()> {
    match cmd {
        UserCmd::Add {
            username,
            access_key,
            secret_key,
            database_url,
        } => {
            let store = load_iam_store(&database_url).await?;
            add_user(
                store.as_ref(),
                &username,
                access_key.as_deref(),
                secret_key.as_deref(),
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
            println!("✓ user {username} created");
        }
        UserCmd::List { database_url } => {
            let store = load_iam_store(&database_url).await?;
            for u in store.list_users().await {
                println!(
                    "{} ({}) keys={}",
                    u.username,
                    u.user_id,
                    u.access_keys.len()
                );
            }
        }
        UserCmd::Delete {
            username,
            database_url,
        } => {
            let store = load_iam_store(&database_url).await?;
            store
                .delete_user(&username)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("✓ user {username} deleted");
        }
        UserCmd::CreateKey {
            username,
            database_url,
        } => {
            let store = load_iam_store(&database_url).await?;
            let key = store
                .create_access_key(&username)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("access_key_id={}", key.access_key_id);
            println!("secret_access_key={}", key.secret_access_key);
        }
        UserCmd::DeleteKey {
            username,
            access_key_id,
            database_url,
        } => {
            let store = load_iam_store(&database_url).await?;
            store
                .delete_access_key(&username, &access_key_id)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("✓ access key deleted");
        }
        UserCmd::PutPolicy {
            username,
            policy_name,
            document,
            database_url,
        } => {
            let store = load_iam_store(&database_url).await?;
            let doc: PolicyDocumentRaw = serde_json::from_str(&document)?;
            store
                .put_user_policy(&username, &policy_name, doc)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("✓ inline policy attached");
        }
        UserCmd::AttachPolicy {
            username,
            policy_arn,
            database_url,
        } => {
            let store = load_iam_store(&database_url).await?;
            store
                .attach_user_policy(&username, &policy_arn)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("✓ managed policy attached");
        }
    }
    Ok(())
}

async fn add_user(
    store: &dyn crate::iam::IamStore,
    username: &str,
    access_key: Option<&str>,
    secret_key: Option<&str>,
) -> Result<(), String> {
    if let (Some(ak), Some(sk)) = (access_key, secret_key) {
        store.add_user_with_keys(username, ak, sk).await?;
    } else {
        let user = store.create_user(username).await?;
        let _ = store.create_access_key(&user.username).await?;
    }
    Ok(())
}
