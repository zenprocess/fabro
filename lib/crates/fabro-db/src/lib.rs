use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use chrono::{DateTime, Utc};
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

/// Parse an RFC 3339 timestamp column value into UTC.
pub fn parse_rfc3339(value: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(|timestamp| timestamp.with_timezone(&Utc))
}

/// Result of a one-time import of a legacy file or directory into SQLite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub source_path:   PathBuf,
    pub backup_path:   PathBuf,
    pub imported_rows: usize,
    pub skipped_rows:  usize,
    pub names:         Vec<String>,
}

/// Backup destination for a legacy file or directory after a one-time import
/// into SQLite. `default_name` is used when `source_path` has no file name.
pub fn legacy_backup_path(
    source_path: &Path,
    default_name: &str,
    imported_at: DateTime<Utc>,
) -> PathBuf {
    let timestamp = imported_at.format("%Y%m%dT%H%M%S%fZ");
    let mut file_name = source_path
        .file_name()
        .map_or_else(|| OsString::from(default_name), OsString::from);
    file_name.push(format!(".imported-{timestamp}.bak"));
    source_path.with_file_name(file_name)
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
