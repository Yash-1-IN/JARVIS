//! Conversation history, its retention and the JSON export (spec §8.2 to §8.4).
//!
//! History is on by default and kept for 30 days (D-11). A conversation ends after ten
//! minutes without a message. Messages hold what was said and one-line summaries of tool
//! calls, never file contents; key-shaped secrets are masked before anything is stored.
//! `Off` stores nothing, and "delete all" really deletes (the same `secure_delete` and
//! log-truncate rules as memories apply).

use super::store::{MemoryStore, StoreError};
use crate::ipc::types::{
    ConversationSummary, HistoryMessage, HistoryRetention, HistoryRole, Memory,
};
use rusqlite::{OptionalExtension, params};
use serde_json::json;

/// A conversation ends after this long without a message.
pub const IDLE_MS: u64 = 10 * 60 * 1000;
/// One stored message is cut at this many characters.
pub const MAX_MESSAGE_CHARS: usize = 4_000;
const RETENTION_KEY: &str = "history_retention";

fn role_str(r: HistoryRole) -> &'static str {
    match r {
        HistoryRole::User => "user",
        HistoryRole::Assistant => "assistant",
        HistoryRole::Tool => "tool",
    }
}

fn parse_role(s: &str) -> HistoryRole {
    match s {
        "assistant" => HistoryRole::Assistant,
        "tool" => HistoryRole::Tool,
        _ => HistoryRole::User, // the CHECK constraint allows nothing else
    }
}

fn retention_days(r: HistoryRetention) -> Option<u64> {
    match r {
        HistoryRetention::Off => Some(0),
        HistoryRetention::Days7 => Some(7),
        HistoryRetention::Days30 => Some(30),
        HistoryRetention::Days365 => Some(365),
        HistoryRetention::Forever => None,
    }
}

/// Masks secrets, drops control characters other than line breaks, and cuts the length.
fn clean_message(text: &str) -> String {
    let kept: String = text
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect();
    let masked = crate::logging::redact(kept.trim());
    masked.chars().take(MAX_MESSAGE_CHARS).collect()
}

/// What `record_message` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recorded {
    /// The message was written (false when history is off or the text is empty).
    pub stored: bool,
    /// This message opened a new conversation (the first ever, or after ten idle minutes).
    pub new_conversation: bool,
}

impl Recorded {
    const NOTHING: Self = Self {
        stored: false,
        new_conversation: false,
    };
}

impl MemoryStore {
    /// How long history is kept. 30 days until the user chooses otherwise.
    pub fn history_retention(&self) -> Result<HistoryRetention, StoreError> {
        let conn = self.conn();
        let raw: Option<String> = conn
            .query_row(
                "SELECT value_json FROM settings WHERE key = ?1",
                [RETENTION_KEY],
                |r| r.get(0),
            )
            .optional()?;
        Ok(raw
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or(HistoryRetention::Days30))
    }

    /// Saves the choice and applies it at once: a shorter period removes what is now too old.
    pub fn set_history_retention(
        &self,
        r: HistoryRetention,
        now_ms: u64,
    ) -> Result<(), StoreError> {
        {
            let conn = self.conn();
            conn.execute(
                "INSERT INTO settings (key, value_json) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
                params![RETENTION_KEY, serde_json::to_string(&r).unwrap_or_default()],
            )?;
        }
        self.purge_history(now_ms).map(|_| ())
    }

    /// Stores one message in the open conversation, starting a new one after ten idle minutes.
    /// Does nothing when retention is `Off`.
    pub fn record_message(
        &self,
        role: HistoryRole,
        content: &str,
        tool_call_json: Option<&str>,
        now_ms: u64,
    ) -> Result<Recorded, StoreError> {
        if self.history_retention()? == HistoryRetention::Off {
            return Ok(Recorded::NOTHING);
        }
        let content = clean_message(content);
        if content.is_empty() {
            return Ok(Recorded::NOTHING);
        }
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let last_activity = "COALESCE((SELECT MAX(ts) FROM messages m WHERE m.conversation_id = c.id), c.started_at)";
        let open: Option<(String, i64)> = tx
            .query_row(
                &format!(
                    "SELECT c.id, {last_activity} FROM conversations c
                     WHERE c.ended_at IS NULL ORDER BY c.rowid DESC LIMIT 1"
                ),
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let mut new_conversation = false;
        let current = match open {
            Some((id, last)) if now_ms.saturating_sub(last.max(0) as u64) <= IDLE_MS => id,
            _ => {
                new_conversation = true;
                let id = uuid::Uuid::now_v7().to_string();
                tx.execute(
                    "INSERT INTO conversations (id, started_at) VALUES (?1, ?2)",
                    params![id, now_ms as i64],
                )?;
                id
            }
        };
        // Every other open conversation has ended at its last message.
        tx.execute(
            &format!(
                "UPDATE conversations AS c SET ended_at = {last_activity}
                 WHERE c.ended_at IS NULL AND c.id != ?1"
            ),
            [&current],
        )?;
        if role == HistoryRole::User {
            let first_line: String = content
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(60)
                .collect();
            tx.execute(
                "UPDATE conversations SET title = ?2 WHERE id = ?1 AND title IS NULL",
                params![current, first_line],
            )?;
        }
        tx.execute(
            "INSERT INTO messages (id, conversation_id, role, content, ts, tool_call_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                uuid::Uuid::now_v7().to_string(),
                current,
                role_str(role),
                content,
                now_ms as i64,
                tool_call_json
            ],
        )?;
        tx.commit()?;
        Ok(Recorded {
            stored: true,
            new_conversation,
        })
    }

    /// Conversations, newest first. `before` is a conversation id: only older ones are
    /// returned (an unknown id gives nothing). `limit` is clamped to 1..=200.
    pub fn list_conversations(
        &self,
        limit: usize,
        before: Option<&str>,
    ) -> Result<Vec<ConversationSummary>, StoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.started_at, c.ended_at, c.title,
                    (SELECT COUNT(*) FROM messages m WHERE m.conversation_id = c.id)
             FROM conversations c
             WHERE ?1 IS NULL OR c.rowid < (SELECT rowid FROM conversations WHERE id = ?1)
             ORDER BY c.rowid DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![before, limit.clamp(1, 200) as i64], |r| {
                Ok(ConversationSummary {
                    id: r.get(0)?,
                    started_at: r.get::<_, i64>(1)?.max(0) as u64,
                    ended_at: r.get::<_, Option<i64>>(2)?.map(|v| v.max(0) as u64),
                    title: r.get(3)?,
                    message_count: r.get::<_, i64>(4)?.max(0) as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The messages of one conversation, oldest first.
    pub fn get_conversation(&self, id: &str) -> Result<Vec<HistoryMessage>, StoreError> {
        let conn = self.conn();
        let exists: Option<i64> = conn
            .query_row("SELECT 1 FROM conversations WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        if exists.is_none() {
            return Err(StoreError::NotFound);
        }
        let mut stmt = conn.prepare(
            "SELECT id, role, content, ts FROM messages WHERE conversation_id = ?1
             ORDER BY ts, rowid",
        )?;
        let rows = stmt
            .query_map([id], |r| {
                Ok(HistoryMessage {
                    id: r.get(0)?,
                    role: parse_role(&r.get::<_, String>(1)?),
                    content: r.get(2)?,
                    ts: r.get::<_, i64>(3)?.max(0) as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Deletes every conversation and message. Memories are not history and stay.
    pub fn delete_all_history(&self) -> Result<(), StoreError> {
        let conn = self.conn();
        conn.execute("DELETE FROM conversations", [])?;
        Self::scrub(&conn);
        Ok(())
    }

    /// Removes conversations older than the retention period; returns how many went.
    pub fn purge_history(&self, now_ms: u64) -> Result<usize, StoreError> {
        let Some(days) = retention_days(self.history_retention()?) else {
            return Ok(0);
        };
        let cutoff = now_ms.saturating_sub(days * 86_400_000) as i64;
        let conn = self.conn();
        let n = conn.execute(
            "DELETE FROM conversations AS c WHERE
             COALESCE(c.ended_at,
                      (SELECT MAX(ts) FROM messages m WHERE m.conversation_id = c.id),
                      c.started_at) < ?1
             OR ?2 = 0",
            params![cutoff, days as i64],
        )?;
        if n > 0 {
            Self::scrub(&conn);
        }
        Ok(n)
    }

    /// Everything the app keeps about the user, as one JSON document (spec §8.4). API keys
    /// and audio are never in the database, so they are never in the export.
    pub fn export_json(&self, now_ms: u64) -> Result<String, StoreError> {
        let memories: Vec<Memory> = self.all()?;
        let conversations = self.list_conversations(200, None)?;
        // Page through everything, 200 at a time.
        let mut all = conversations;
        while all.len() % 200 == 0 && !all.is_empty() {
            let last = all.last().map(|c| c.id.clone());
            let more = self.list_conversations(200, last.as_deref())?;
            if more.is_empty() {
                break;
            }
            all.extend(more);
        }
        let mut convs = Vec::new();
        for c in all {
            let messages = self.get_conversation(&c.id)?;
            convs.push(json!({ "conversation": c, "messages": messages }));
        }
        let doc = json!({
            "format": "jarvis-export",
            "version": 1,
            "exportedAt": now_ms,
            "historyRetention": self.history_retention()?,
            "memories": memories,
            "history": convs,
        });
        serde_json::to_string_pretty(&doc)
            .map_err(|e| StoreError::Db(format!("export could not be written: {e}")))
    }
}

#[cfg(test)]
mod tests;
