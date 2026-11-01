//! Memory and history storage (spec §8): SQLite through `rusqlite`, in the backend only.
//! Nothing here is reachable from the webview; the typed commands come in the next step.
//!
//! Step 1 of M5: the database (WAL, embedded migrations, corrupt-file recovery) and the
//! memory store (propose, accept, edit, delete, list, retrieve). History, retention and the
//! commands follow in later steps.

// Wired into the app by the next M5 step.
#![allow(dead_code)]

pub mod db;
pub mod store;
