use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::error::Error;
use std::fmt;
use std::fs::{self, DirBuilder};
use std::io;
use std::path::Path;
use std::time::Duration;

mod aggregate;
mod checkpoint;
mod error_history;
mod history;
mod legacy;
pub mod mcp;

mod migration;
mod observations;
mod skill_history;
mod timestamp;

pub use aggregate::{Aggregate, Bucket, CallCounts};
pub use checkpoint::{CallHeader, Checkpoint, CheckpointBatch, Cursor, ExpectedCursor, SelectedCheckpointReader, StoredCall};
pub use error_history::{sanitize_error_identifiers, ERROR_PREVIEW_HOURS, ERROR_PREVIEW_LIMIT};
pub use history::{HistoryItem, HistoryWrite, Namespace, Page, PageQuery, Position, Section};
pub use observations::{
    combine_observations, sql_event_id, ObservationBatch, ObservationStore, ObservationView,
    ObservationWrite,
};
pub use timestamp::{normalize_utc_stamp, parse_utc_stamp};

pub const FILE_NAME: &str = "lam-history.sqlite3";
pub const APPLICATION_ID: i64 = 0x4C414D31;
pub const SCHEMA_VERSION: i64 = 4;
const SIDECARS: [&str; 3] = ["-journal", "-wal", "-shm"];

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Sql(rusqlite::Error),
    UnsafePath,
    UnsupportedSchema { application: i64, version: i64 },
    InvalidMigration,
    InvalidLegacy,
    InvalidQuery,
}

impl fmt::Display for StoreError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(output),
            Self::Sql(error) => error.fmt(output),
            Self::UnsafePath => output.write_str("Unsafe history storage"),
            Self::UnsupportedSchema {
                application,
                version,
            } => {
                write!(
                    output,
                    "Unsupported history schema ({application:#x}, {version})"
                )
            }
            Self::InvalidQuery => output.write_str("Invalid history query"),
            Self::InvalidMigration => output.write_str("Invalid history data or migration backup"),
            Self::InvalidLegacy => output.write_str("Invalid legacy history"),
        }
    }
}

impl Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}

/// Opens LAM-owned storage with the same schema as the existing Python service.
/// All timestamps written by the caller use UTC with millisecond precision.
pub struct Store {
    db: Connection,
}

impl Store {
    /// Return None when opening a missing database for reading.
    /// A writer upgrades a recognized legacy database before serving queries.
    pub fn open(directory: &Path, access: Access) -> Result<Option<Self>> {
        check_paths(directory)?;
        if access == Access::ReadOnly && !directory.join(FILE_NAME).try_exists()? {
            return Ok(None);
        }
        if access == Access::ReadWrite {
            create_directory(directory)?;
            check_paths(directory)?;
        }
        let path = directory.canonicalize()?.join(FILE_NAME);

        let flags = OpenFlags::SQLITE_OPEN_NOFOLLOW
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | match access {
                Access::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
                Access::ReadWrite => {
                    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
                }
            };
        let mut db = Connection::open_with_flags(&path, flags)?;
        db.busy_timeout(Duration::from_millis(200))?;
        let mut header = identity(&db)?;
        if header == (0, 0) && access == Access::ReadWrite {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            header = identity(&tx)?;
            if header == (0, 0)
                && !tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master)", [], |row| {
                    row.get::<_, bool>(0)
                })?
            {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
                }
                tx.execute_batch(include_str!("schema.sql"))?;
                tx.pragma_update(None, "application_id", APPLICATION_ID)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                header = (APPLICATION_ID, SCHEMA_VERSION);
            }
            tx.commit()?;
        }
        if access == Access::ReadWrite && migration::legacy(header) {
            migration::upgrade(&mut db, &path)?;
            header = identity(&db)?;
        }
        if header != (APPLICATION_ID, SCHEMA_VERSION) {
            return Err(StoreError::UnsupportedSchema {
                application: header.0,
                version: header.1,
            });
        }
        match access {
            Access::ReadOnly => db.pragma_update(None, "query_only", true)?,
            Access::ReadWrite => {
                db.pragma_update(None, "journal_mode", "WAL")?;
                db.pragma_update(None, "synchronous", "NORMAL")?;
            }
        }
        Ok(Some(Self { db }))
    }

    /// Upgrade an existing database without creating a missing database.
    pub fn upgrade(directory: &Path) -> Result<bool> {
        check_paths(directory)?;
        if !directory.join(FILE_NAME).try_exists()? {
            return Ok(false);
        }
        let path = directory.canonicalize()?.join(FILE_NAME);
        let read = migration::connection(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let header = identity(&read)?;
        drop(read);
        if header == (APPLICATION_ID, SCHEMA_VERSION) {
            return Ok(false);
        }
        if !migration::legacy(header) {
            return Err(StoreError::UnsupportedSchema {
                application: header.0,
                version: header.1,
            });
        }
        let mut writer = migration::connection(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        migration::upgrade(&mut writer, &path)
    }
}

fn identity(db: &Connection) -> rusqlite::Result<(i64, i64)> {
    db.query_row(
        "SELECT application_id, user_version FROM pragma_application_id(), pragma_user_version()",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
}

fn check_paths(directory: &Path) -> Result<()> {
    check_path(directory)?;
    check_path(&directory.join(FILE_NAME))?;
    for suffix in SIDECARS {
        check_path(&directory.join(format!("{FILE_NAME}{suffix}")))?;
    }
    Ok(())
}

fn check_path(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            #[cfg(windows)]
            let redirected = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let redirected = metadata.is_symlink();
            if redirected || !metadata.is_dir() && !metadata.is_file() {
                return Err(StoreError::UnsafePath);
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn create_directory(path: &Path) -> io::Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}
