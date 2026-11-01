//! Opening the database and applying the embedded migrations (spec §8.1).
//!
//! - WAL mode, foreign keys on, and `secure_delete` on so deleted text is overwritten in the
//!   file rather than left in free pages (spec §8.4: a deleted memory is really gone).
//! - Migrations are embedded in the binary and run one transaction each. A failing migration
//!   rolls back, so the file keeps the schema it had, and start-up stops with the error.
//! - A file that is not a database (or is corrupt) is renamed aside and replaced by a fresh
//!   one; the caller gets the backup path so the UI can say so.

use rusqlite::{Connection, ErrorCode};
use std::path::{Path, PathBuf};

/// Each entry takes the schema from version `i` to `i + 1`. Never edit a shipped entry.
const MIGRATIONS: &[&str] = &[include_str!("migrations/001_initial.sql")];

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("the database could not be opened: {0}")]
    Open(String),
    #[error("a database migration failed (version {version}): {message}")]
    Migration { version: usize, message: String },
    #[error(
        "the database was written by a newer version of JARVIS (schema {found}, this build knows {known})"
    )]
    TooNew { found: usize, known: usize },
}

pub struct Opened {
    pub conn: Connection,
    /// Set when an unreadable file was moved aside; the new database is empty.
    pub recovered_from: Option<PathBuf>,
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    // journal_mode returns a row, so it is read rather than executed.
    conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0))?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; PRAGMA secure_delete = ON; PRAGMA busy_timeout = 5000;",
    )
}

fn schema_version(conn: &Connection) -> rusqlite::Result<usize> {
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .map(|v| v.max(0) as usize)
}

/// Applies every migration the file has not seen yet.
pub fn migrate_with(conn: &mut Connection, migrations: &[&str]) -> Result<(), DbError> {
    let known = migrations.len();
    let found = schema_version(conn).map_err(|e| DbError::Open(e.to_string()))?;
    if found > known {
        return Err(DbError::TooNew { found, known });
    }
    for (i, sql) in migrations.iter().enumerate().skip(found) {
        let step = |conn: &mut Connection| -> rusqlite::Result<()> {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()
        };
        step(conn).map_err(|e| DbError::Migration {
            version: i + 1,
            message: e.to_string(),
        })?;
    }
    Ok(())
}

/// Why the database could not be prepared for use.
enum Prepare {
    /// SQLite says the file is not a database or is damaged (by error code, never by text).
    Corrupt(DbError),
    Other(DbError),
}

fn is_corrupt_code(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt)
    )
}

fn classify(e: rusqlite::Error) -> Prepare {
    let corrupt = is_corrupt_code(&e);
    let err = DbError::Open(e.to_string());
    if corrupt {
        Prepare::Corrupt(err)
    } else {
        Prepare::Other(err)
    }
}

/// Opens the file and reads its schema version. Only a failure here can mean a damaged
/// file; a failing migration step never does (it rolls back and is reported as it is).
fn prepare(path: &Path) -> Result<Connection, Prepare> {
    let conn = Connection::open(path).map_err(classify)?;
    configure(&conn).map_err(classify)?;
    schema_version(&conn).map_err(classify)?;
    Ok(conn)
}

fn open_and_migrate(path: &Path) -> Result<Connection, Prepare> {
    let mut conn = prepare(path)?;
    migrate_with(&mut conn, MIGRATIONS).map_err(Prepare::Other)?;
    Ok(conn)
}

fn into_error(p: Prepare) -> DbError {
    match p {
        Prepare::Corrupt(e) | Prepare::Other(e) => e,
    }
}

/// Opens (creating if needed) the database at `path`. `now_ms` names a backup file.
pub fn open(path: &Path, now_ms: u64) -> Result<Opened, DbError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| DbError::Open(e.to_string()))?;
    }
    match open_and_migrate(path) {
        Ok(conn) => Ok(Opened {
            conn,
            recovered_from: None,
        }),
        Err(Prepare::Corrupt(_)) => {
            let mut backup = path.as_os_str().to_owned();
            backup.push(format!(".corrupt-{now_ms}"));
            let backup = PathBuf::from(backup);
            std::fs::rename(path, &backup).map_err(|e| DbError::Open(e.to_string()))?;
            // The write-ahead files belong to the broken database.
            for ext in ["-wal", "-shm"] {
                let mut side = path.as_os_str().to_owned();
                side.push(ext);
                let _ = std::fs::remove_file(PathBuf::from(side));
            }
            let conn = open_and_migrate(path).map_err(into_error)?;
            Ok(Opened {
                conn,
                recovered_from: Some(backup),
            })
        }
        Err(p) => Err(into_error(p)),
    }
}

/// A private in-memory database with the full schema (tests).
pub fn open_in_memory() -> Result<Connection, DbError> {
    let mut conn = Connection::open_in_memory().map_err(|e| DbError::Open(e.to_string()))?;
    configure(&conn).map_err(|e| DbError::Open(e.to_string()))?;
    migrate_with(&mut conn, MIGRATIONS)?;
    Ok(conn)
}

#[cfg(test)]
mod tests;
