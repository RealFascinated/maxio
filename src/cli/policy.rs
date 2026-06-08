use clap::Subcommand;

use crate::cli::load_iam_store;
use crate::iam::types::PolicyDocumentRaw;

#[derive(Subcommand, Debug)]
pub enum PolicyCmd {
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        document: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    List {
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    Show {
        #[arg(long)]
        name: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
    Delete {
        #[arg(long)]
        name: String,
        #[arg(long, env = "MAXIO_DATABASE_URL")]
        database_url: String,
    },
}

pub async fn run(cmd: PolicyCmd) -> anyhow::Result<()> {
    match cmd {
        PolicyCmd::Create {
            name,
            document,
            database_url,
        } => {
            let store = load_iam_store(&database_url).await?;
            let doc: PolicyDocumentRaw = serde_json::from_str(&document)?;
            let policy = store
                .create_managed_policy(&name, doc)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("✓ policy {} ({})", policy.policy_name, policy.arn);
        }
        PolicyCmd::List { database_url } => {
            let store = load_iam_store(&database_url).await?;
            for p in store.list_managed_policies().await {
                println!("{} {}", p.policy_name, p.arn);
            }
        }
        PolicyCmd::Show { name, database_url } => {
            let store = load_iam_store(&database_url).await?;
            let policy = store
                .get_managed_policy(&name)
                .await
                .ok_or_else(|| anyhow::anyhow!("policy not found"))?;
            println!("{}", serde_json::to_string_pretty(&policy.document)?);
        }
        PolicyCmd::Delete { name, database_url } => {
            let store = load_iam_store(&database_url).await?;
            store
                .delete_managed_policy(&name)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("✓ policy deleted");
        }
    }
    Ok(())
}
