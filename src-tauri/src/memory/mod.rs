//! Memory and history storage (spec §8): SQLite through `rusqlite`, in the backend only.
//! The webview reaches it only through the typed `memory_*` and `history_*` commands.
//!
//! - `db`: opening the file (WAL, secure delete), embedded migrations, corrupt-file recovery.
//! - `store`: memories (proposed, accepted, edited, deleted, retrieved).
//! - `history`: conversations, retention, delete-all and the JSON export.
//! - `inbox`: where `memory.propose` and "remember that ..." put new proposals.

pub mod db;
pub mod history;
pub mod inbox;
pub mod store;
