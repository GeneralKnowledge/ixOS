//! An embedded SQLite database for an app's own data (feature `sqlite`).
//!
//! [`Database::open`] gives an app one file with:
//!
//! - **Migrations.** The app lists its schema as SQL steps; step `i` brings
//!   the file from version `i` to `i + 1` (kept in SQLite's `user_version`).
//!   Pending steps run in one transaction, so a failed upgrade leaves the file
//!   as it was. A file from a newer build is refused, never changed.
//! - **Transactions** ([`Database::transaction`]): all or nothing.
//! - **Full-text search.** SQLite is built with FTS5, so an app can declare
//!   `CREATE VIRTUAL TABLE … USING fts5(…)` in a migration.
//! - **Recovery.** A file SQLite reports as damaged (not a database,
//!   corrupt, or failing its quick check) is moved aside (kept, never
//!   deleted) and a fresh database is created; [`Opened::Recovered`] says
//!   so, so the app can tell the user and refill it from its server. A file
//!   that is only locked by another connection or unreadable for now is an
//!   error, never a recovery.
//!
//! SQLite is compiled from source (rusqlite's `bundled` feature), so it needs
//! no system library on iOS or Android. Apps use the re-exported
//! [`rusqlite`] for their queries, so they share this crate's version.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, Transaction};

use crate::StorageError;

/// How [`Database::open`] found the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    /// There was no file; a new one was created.
    Created,
    /// The file was there and readable (and is now up to date).
    Existing,
    /// SQLite reported the file as damaged (not a database, corrupt, or a
    /// failed quick check). It was moved to `moved_to` and a new, empty
    /// database was created in its place. A file that is only locked or
    /// unreadable for now is never recovered: `open` returns an error.
    Recovered { moved_to: PathBuf, reason: String },
}

/// One SQLite database.
pub struct Database {
    connection: Connection,
    path: Option<PathBuf>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("path", &self.path)
            .finish()
    }
}

impl Database {
    /// Opens (or creates) the database at `path` and brings it up to
    /// `migrations.len()`. See the module docs for recovery.
    pub fn open(
        path: impl AsRef<Path>,
        migrations: &[&str],
    ) -> Result<(Self, Opened), StorageError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| StorageError::write("create the database directory", e))?;
            }
        }
        if !path.exists() {
            let database = Self::open_file(&path, migrations)?;
            return Ok((database, Opened::Created));
        }
        if let Check::Damaged(reason) = Self::check(&path)? {
            let moved_to = Self::move_aside(&path)?;
            let database = Self::open_file(&path, migrations)?;
            return Ok((database, Opened::Recovered { moved_to, reason }));
        }
        let database = Self::open_file(&path, migrations)?;
        Ok((database, Opened::Existing))
    }

    /// A database that lives only in memory (tests, previews).
    pub fn in_memory(migrations: &[&str]) -> Result<Self, StorageError> {
        let connection = Connection::open_in_memory()
            .map_err(|e| StorageError::write("open an in-memory database", e))?;
        let mut database = Self {
            connection,
            path: None,
        };
        database.configure()?;
        database.migrate(migrations)?;
        Ok(database)
    }

    /// The file, or `None` in memory.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The connection, for reads and single statements.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Runs `work` in a transaction: committed if it returns `Ok`, rolled
    /// back otherwise.
    pub fn transaction<T, E>(
        &mut self,
        work: impl FnOnce(&Transaction<'_>) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<StorageError>,
    {
        let transaction = self
            .connection
            .transaction()
            .map_err(|e| StorageError::write("begin a transaction", e))?;
        let value = work(&transaction)?;
        transaction
            .commit()
            .map_err(|e| StorageError::write("commit a transaction", e))?;
        Ok(value)
    }

    /// The schema version stored in the file.
    pub fn schema_version(&self) -> Result<u32, StorageError> {
        user_version(&self.connection)
    }

    /// SQLite's own consistency check of the whole file.
    pub fn integrity_check(&self) -> Result<(), StorageError> {
        let result: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(|e| StorageError::read("check the database", e))?;
        if result == "ok" {
            Ok(())
        } else {
            Err(StorageError::corrupt(result))
        }
    }

    /// The version of the SQLite library compiled into the app.
    pub fn sqlite_version() -> &'static str {
        rusqlite::version()
    }

    fn open_file(path: &Path, migrations: &[&str]) -> Result<Self, StorageError> {
        let connection =
            Connection::open(path).map_err(|e| StorageError::write("open the database", e))?;
        let mut database = Self {
            connection,
            path: Some(path.to_path_buf()),
        };
        database.configure()?;
        database.migrate(migrations)?;
        Ok(database)
    }

    fn configure(&self) -> Result<(), StorageError> {
        // WAL keeps readers and the writer apart and survives a kill mid-write;
        // NORMAL sync is durable across app crashes (only a power cut can lose
        // the last commit). In memory, journal_mode stays "memory".
        self.connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;",
            )
            .map_err(|e| StorageError::write("configure the database", e))?;
        self.connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| StorageError::write("configure the database", e))
    }

    fn migrate(&mut self, migrations: &[&str]) -> Result<(), StorageError> {
        let supported = migrations.len() as u32;
        let found = user_version(&self.connection)?;
        if found > supported {
            return Err(StorageError::UnsupportedSchemaVersion { found, supported });
        }
        if found == supported {
            return Ok(());
        }
        let transaction = self
            .connection
            .transaction()
            .map_err(|e| StorageError::write("begin a migration", e))?;
        for (index, step) in migrations.iter().enumerate().skip(found as usize) {
            transaction.execute_batch(step).map_err(|e| {
                StorageError::write(&format!("migrate to schema version {}", index + 1), e)
            })?;
        }
        transaction
            .pragma_update(None, "user_version", supported)
            .map_err(|e| StorageError::write("record the schema version", e))?;
        transaction
            .commit()
            .map_err(|e| StorageError::write("commit a migration", e))
    }

    /// Whether the file at `path` is damaged. Only SQLite's own verdict
    /// counts as damage: `SQLITE_CORRUPT`, `SQLITE_NOTADB`, or a failed
    /// quick check. Anything else (the file is locked by another
    /// connection, unreadable for now, or the disk failed) is an error, so
    /// a transient condition never moves the user's data aside.
    fn check(path: &Path) -> Result<Check, StorageError> {
        let connection = match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        {
            Ok(connection) => connection,
            Err(err) => return Check::classify(err),
        };
        connection
            .busy_timeout(CHECK_BUSY_TIMEOUT)
            .map_err(|e| StorageError::read("check the database", e))?;
        match connection.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0)) {
            Ok(result) if result == "ok" => Ok(Check::Healthy),
            Ok(result) => Ok(Check::Damaged(result)),
            Err(err) => Check::classify(err),
        }
    }

    /// Renames a damaged file (and its WAL files) to `<name>.corrupt-<time>`.
    fn move_aside(path: &Path) -> Result<PathBuf, StorageError> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let moved_to = PathBuf::from(format!("{}.corrupt-{stamp}", path.display()));
        std::fs::rename(path, &moved_to)
            .map_err(|e| StorageError::write("move a damaged database aside", e))?;
        for suffix in ["-wal", "-shm"] {
            let side = PathBuf::from(format!("{}{suffix}", path.display()));
            if side.exists() {
                let _ = std::fs::rename(&side, format!("{}{suffix}", moved_to.display()));
            }
        }
        Ok(moved_to)
    }
}

/// How long the damage check waits for another connection's lock before
/// reporting it as an error.
const CHECK_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// What [`Database::check`] found.
enum Check {
    Healthy,
    Damaged(String),
}

impl Check {
    /// Damage only when SQLite itself says the file is not a usable
    /// database; every other failure is returned as an error.
    fn classify(err: rusqlite::Error) -> Result<Self, StorageError> {
        match err.sqlite_error_code() {
            Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase) => {
                Ok(Self::Damaged(err.to_string()))
            }
            _ => Err(StorageError::read("check the database", err)),
        }
    }
}

fn user_version(connection: &Connection) -> Result<u32, StorageError> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map(|v| v as u32)
        .map_err(|e| StorageError::read("read the schema version", e))
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        StorageError::write("database", error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &[&str] = &[
        "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL);",
        "ALTER TABLE notes ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
         CREATE VIRTUAL TABLE notes_fts USING fts5(body);",
    ];

    #[test]
    fn creates_migrates_and_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.db");
        let (mut db, opened) = Database::open(&path, &SCHEMA[..1]).unwrap();
        assert_eq!(opened, Opened::Created);
        assert_eq!(db.schema_version().unwrap(), 1);
        db.transaction(|tx| {
            tx.execute("INSERT INTO notes (body) VALUES ('first')", [])?;
            Ok::<_, StorageError>(())
        })
        .unwrap();
        drop(db);

        let (db, opened) = Database::open(&path, SCHEMA).unwrap();
        assert_eq!(opened, Opened::Existing);
        assert_eq!(db.schema_version().unwrap(), 2);
        let (body, pinned): (String, i64) = db
            .connection()
            .query_row("SELECT body, pinned FROM notes", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((body.as_str(), pinned), ("first", 0));
        db.integrity_check().unwrap();
    }

    #[test]
    fn a_failed_transaction_changes_nothing() {
        let mut db = Database::in_memory(SCHEMA).unwrap();
        let result = db.transaction(|tx| {
            tx.execute("INSERT INTO notes (body) VALUES ('kept?')", [])?;
            Err::<(), _>(StorageError::corrupt("stop"))
        });
        assert!(result.is_err());
        let count: i64 = db
            .connection()
            .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn a_failed_migration_leaves_the_file_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.db");
        drop(Database::open(&path, &SCHEMA[..1]).unwrap());
        let broken = [SCHEMA[0], "CREATE TABLE extra (x); THIS IS NOT SQL;"];
        assert!(Database::open(&path, &broken).is_err());
        let (db, _) = Database::open(&path, &SCHEMA[..1]).unwrap();
        assert_eq!(db.schema_version().unwrap(), 1);
        let extra: i64 = db
            .connection()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'extra'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(extra, 0);
    }

    #[test]
    fn a_newer_file_is_refused_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.db");
        drop(Database::open(&path, SCHEMA).unwrap());
        let error = Database::open(&path, &SCHEMA[..1]).unwrap_err();
        assert!(matches!(
            error,
            StorageError::UnsupportedSchemaVersion {
                found: 2,
                supported: 1
            }
        ));
        let (db, _) = Database::open(&path, SCHEMA).unwrap();
        assert_eq!(db.schema_version().unwrap(), 2);
    }

    #[test]
    fn a_damaged_file_is_moved_aside_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.db");
        std::fs::write(&path, b"this is not a database, it is a shopping list").unwrap();
        let (db, opened) = Database::open(&path, SCHEMA).unwrap();
        let Opened::Recovered { moved_to, .. } = opened else {
            panic!("expected recovery, got {opened:?}");
        };
        assert_eq!(
            std::fs::read(&moved_to).unwrap(),
            b"this is not a database, it is a shopping list"
        );
        assert_eq!(db.schema_version().unwrap(), 2);
    }

    #[test]
    fn full_text_search_is_built_in() {
        let db = Database::in_memory(SCHEMA).unwrap();
        db.connection()
            .execute_batch(
                "INSERT INTO notes_fts (body) VALUES ('Weekend plans at the lake');
                 INSERT INTO notes_fts (body) VALUES ('Shopping list');",
            )
            .unwrap();
        let hit: String = db
            .connection()
            .query_row(
                "SELECT body FROM notes_fts WHERE notes_fts MATCH 'lake*'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, "Weekend plans at the lake");
        assert!(Database::sqlite_version().starts_with('3'));
    }
}
