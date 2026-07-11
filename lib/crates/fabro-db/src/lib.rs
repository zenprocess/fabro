use std::path::Path;
use std::time::Duration;

use anyhow::Context as _;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use tokio::fs;
#[cfg(unix)]
use tokio::task::spawn_blocking;

pub type DbPool = sqlx::SqlitePool;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct Database {
    pool: DbPool,
}

impl Database {
    pub async fn connect(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.with_context(|| {
                format!("creating SQLite database directory {}", parent.display())
            })?;
        }

        prepare_private_database_file(path).await?;

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .with_context(|| format!("opening SQLite database {}", path.display()))?;

        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> anyhow::Result<()> {
        MIGRATOR
            .run(&self.pool)
            .await
            .context("running SQLite migrations")
    }

    pub async fn health_check(&self) -> anyhow::Result<()> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .context("checking SQLite database health")?;
        Ok(())
    }

    pub fn pool(&self) -> &DbPool {
        &self.pool
    }

    pub fn clone_pool(&self) -> DbPool {
        self.pool.clone()
    }
}

#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "SQLite file permissions must be established synchronously before opening the pool"
)]
async fn prepare_private_database_file(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

    let path = path.to_path_buf();
    spawn_blocking(move || {
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(&path)
            .with_context(|| format!("creating private SQLite database {}", path.display()))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("setting private SQLite permissions on {}", path.display()))
    })
    .await
    .context("joining SQLite permission setup task")?
}

#[cfg(not(unix))]
async fn prepare_private_database_file(path: &Path) -> anyhow::Result<()> {
    fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .await
        .with_context(|| format!("creating SQLite database {}", path.display()))?;
    Ok(())
}
