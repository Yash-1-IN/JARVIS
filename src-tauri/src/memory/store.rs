//! The memory store (spec §8.3, §8.4).
//!
//! Nothing becomes a retrievable memory without the user: every new row is `proposed`, and
//! only `accept` makes it `active`. Retrieval returns `active` rows only. Delete is a hard
//! delete (the index entry goes with it, and `secure_delete` overwrites the text).

use crate::ipc::types::{Memory, MemoryStatus};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::sync::Mutex;

/// Longest memory text, counted in Unicode code points (contract: `chars().count()`).
pub const MAX_MEMORY_CHARS: usize = 500;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("That memory no longer exists.")]
    NotFound,
    /// The text is empty, too long or otherwise refused. Safe to show.
    #[error("{0}")]
    Invalid(String),
    #[error("The memory database failed.")]
    Db(String),
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        // The detail goes to the log by the caller; it is never shown.
        Self::Db(e.to_string())
    }
}

pub struct MemoryStore {
    conn: Mutex<Connection>,
}

fn status_str(s: MemoryStatus) -> &'static str {
    match s {
        MemoryStatus::Proposed => "proposed",
        MemoryStatus::Active => "active",
        MemoryStatus::Archived => "archived",
    }
}

fn parse_status(s: &str) -> MemoryStatus {
    match s {
        "active" => MemoryStatus::Active,
        "archived" => MemoryStatus::Archived,
        _ => MemoryStatus::Proposed, // the CHECK constraint allows nothing else
    }
}

fn row(r: &Row<'_>) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: r.get(0)?,
        content: r.get(1)?,
        status: parse_status(&r.get::<_, String>(2)?),
        created_at: r.get::<_, i64>(3)?.max(0) as u64,
        updated_at: r.get::<_, i64>(4)?.max(0) as u64,
    })
}

const COLUMNS: &str = "id, content, status, created_at, updated_at";

/// One line, no control characters, secrets masked, within the limit; or why not.
pub fn clean(text: &str) -> Result<String, StoreError> {
    let spaced: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let one_line = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    // Keys and tokens are never stored (spec §8.3); the user sees the masked text.
    let masked = crate::logging::redact(&one_line);
    if masked.is_empty() {
        return Err(StoreError::Invalid("The memory text is empty.".into()));
    }
    if masked.chars().count() > MAX_MEMORY_CHARS {
        return Err(StoreError::Invalid(format!(
            "A memory can be at most {MAX_MEMORY_CHARS} characters."
        )));
    }
    Ok(masked)
}

/// A safe full-text query from free text: each word quoted, joined with OR. `None` when
/// there is nothing to search for.
fn fts_query(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2)
        .take(12)
        .map(|w| format!("\"{}\"", w.to_lowercase()))
        .collect();
    (!words.is_empty()).then(|| words.join(" OR "))
}

impl MemoryStore {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Folds the write-ahead log into the main file and empties it, so text that was just
    /// changed or deleted does not linger in the `-wal` file of a long-running app.
    fn scrub(conn: &Connection) {
        let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    }

    fn get(conn: &Connection, id: &str) -> Result<Memory, StoreError> {
        conn.query_row(
            &format!("SELECT {COLUMNS} FROM memories WHERE id = ?1"),
            [id],
            row,
        )
        .optional()?
        .ok_or(StoreError::NotFound)
    }

    /// Stores a new `proposed` memory. The same text already proposed or active returns
    /// that row instead of a duplicate.
    pub fn propose(
        &self,
        content: &str,
        source_message_id: Option<&str>,
        now_ms: u64,
    ) -> Result<Memory, StoreError> {
        let content = clean(content)?;
        let conn = self.conn();
        let existing = conn
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM memories
                     WHERE content = ?1 AND status IN ('proposed', 'active')"
                ),
                [&content],
                row,
            )
            .optional()?;
        if let Some(m) = existing {
            return Ok(m);
        }
        let id = uuid::Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO memories (id, content, status, source_message_id, created_at, updated_at)
             VALUES (?1, ?2, 'proposed', ?3, ?4, ?4)",
            params![id, content, source_message_id, now_ms as i64],
        )?;
        Self::get(&conn, &id)
    }

    /// `proposed` becomes `active`. Accepting an `active` memory changes nothing.
    pub fn accept(&self, id: &str, now_ms: u64) -> Result<Memory, StoreError> {
        let conn = self.conn();
        let m = Self::get(&conn, id)?;
        if m.status == MemoryStatus::Active {
            return Ok(m);
        }
        conn.execute(
            "UPDATE memories SET status = 'active', updated_at = ?2 WHERE id = ?1",
            params![id, now_ms as i64],
        )?;
        Self::get(&conn, id)
    }

    /// Changes the text of a memory of any status; the status is kept.
    pub fn edit(&self, id: &str, content: &str, now_ms: u64) -> Result<Memory, StoreError> {
        let content = clean(content)?;
        let conn = self.conn();
        Self::get(&conn, id)?;
        let same: Option<String> = conn
            .query_row(
                "SELECT id FROM memories
                 WHERE content = ?1 AND id != ?2 AND status IN ('proposed', 'active')",
                params![content, id],
                |r| r.get(0),
            )
            .optional()?;
        if same.is_some() {
            return Err(StoreError::Invalid("You already have that memory.".into()));
        }
        conn.execute(
            "UPDATE memories SET content = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, content, now_ms as i64],
        )?;
        Self::scrub(&conn);
        Self::get(&conn, id)
    }

    /// Hard delete, for any status. Denying a proposal is this call.
    pub fn delete(&self, id: &str) -> Result<(), StoreError> {
        let conn = self.conn();
        let n = conn.execute("DELETE FROM memories WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(StoreError::NotFound);
        }
        Self::scrub(&conn);
        Ok(())
    }

    /// Newest first (by last change), optionally one status only.
    pub fn list(&self, status: Option<MemoryStatus>) -> Result<Vec<Memory>, StoreError> {
        let conn = self.conn();
        let (sql, filter) = match status {
            Some(s) => (
                format!(
                    "SELECT {COLUMNS} FROM memories WHERE status = ?1
                     ORDER BY updated_at DESC, rowid DESC"
                ),
                Some(status_str(s)),
            ),
            None => (
                format!("SELECT {COLUMNS} FROM memories ORDER BY updated_at DESC, rowid DESC"),
                None,
            ),
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows = match filter {
            Some(f) => stmt.query_map([f], row)?.collect::<Result<Vec<_>, _>>()?,
            None => stmt.query_map([], row)?.collect::<Result<Vec<_>, _>>()?,
        };
        Ok(rows)
    }

    /// Up to `limit` **active** memories that match the words of `query`, best match first,
    /// ties broken by recency. Proposed and archived memories are never returned.
    pub fn retrieve(&self, query: &str, limit: usize) -> Result<Vec<Memory>, StoreError> {
        let Some(q) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT m.id, m.content, m.status, m.created_at, m.updated_at
             FROM memories_fts f JOIN memories m ON m.rowid = f.rowid
             WHERE memories_fts MATCH ?1 AND m.status = 'active'
             ORDER BY f.rank, m.updated_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![q, limit.clamp(1, 20) as i64], row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Everything, for the export (any status).
    pub fn all(&self) -> Result<Vec<Memory>, StoreError> {
        self.list(None)
    }
}

#[cfg(test)]
mod tests;
