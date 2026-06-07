pub mod bucket_cache;
pub mod context;
pub mod repos;
pub mod schema;

pub use bucket_cache::{BucketCache, CachedBucketEntry};
pub use context::DbContext;

use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use diesel_async::async_connection_wrapper::AsyncConnectionWrapper;
use diesel_async::pooled_connection::deadpool::Pool;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("src/db/migrations");

pub type DbPool = Pool<AsyncPgConnection>;

pub async fn create_pool(database_url: &str) -> Result<DbPool, anyhow::Error> {
    let config = AsyncDieselConnectionManager::<AsyncPgConnection>::new(database_url);
    let pool = Pool::builder(config).max_size(64).build()?;
    Ok(pool)
}

pub async fn run_migrations(database_url: &str) -> Result<(), anyhow::Error> {
    let conn = AsyncPgConnection::establish(database_url).await?;
    tokio::task::spawn_blocking(move || {
        let mut wrapper: AsyncConnectionWrapper<AsyncPgConnection> =
            AsyncConnectionWrapper::from(conn);
        wrapper
            .run_pending_migrations(MIGRATIONS)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("migration failed: {e}"))
    })
    .await??;
    Ok(())
}

pub async fn health_check(pool: &DbPool) -> bool {
    use diesel::sql_query;

    let Ok(mut conn) = pool.get().await else {
        return false;
    };
    RunQueryDsl::execute(sql_query("SELECT 1"), &mut conn)
        .await
        .is_ok()
}
