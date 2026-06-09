use std::sync::Arc;

use crate::storage::blob::BlobStorage;
use crate::storage::orphans;
use crate::storage::{MetadataStore, PgMetadataStore};

pub async fn run(
    delete: bool,
    database_url: &str,
    data_dir: &str,
    cache_dir: Option<&str>,
) -> anyhow::Result<()> {
    crate::db::run_migrations(database_url).await?;
    let pool = Arc::new(crate::db::create_pool(database_url).await?);
    let blobs = BlobStorage::new(data_dir).await?;
    let orphans = orphans::scan_orphaned_meta(Arc::clone(&pool), &blobs, cache_dir).await?;

    if orphans.is_empty() {
        println!("no orphaned metadata");
        return Ok(());
    }

    for entry in &orphans {
        match &entry.source {
            crate::db::repos::MetaBlobSource::Current => {
                println!("{}/{}", entry.bucket, entry.key);
            }
            crate::db::repos::MetaBlobSource::Version(version_id) => {
                println!("{}/{}?versionId={}", entry.bucket, entry.key, version_id);
            }
        }
    }
    println!("{} orphaned metadata row(s)", orphans.len());

    if delete {
        let meta: Arc<dyn MetadataStore> = Arc::new(PgMetadataStore::new(
            pool,
            crate::config::MemoryCacheLimits::default(),
        ));
        let removed = orphans::delete_orphaned_meta(meta.as_ref(), &orphans).await?;
        println!("✓ deleted {removed} orphaned metadata row(s)");
    }

    Ok(())
}
