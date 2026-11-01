//! What tools need from the operating system, as traits. The real adapters live in
//! `platform/` (Developer B); until they exist the placeholders below answer "not available",
//! so no tool pretends to work.

use std::io;
use std::sync::{Arc, Mutex};

/// Starts an installed app by its stable id (macOS bundle id, Windows exe path or
/// AppUserModelID, Linux `.desktop` id). Fixed arguments only: no shell, no arguments from
/// the model. Blocking; the tool calls it on a blocking thread.
pub trait AppLauncher: Send + Sync {
    fn launch(&self, app_id: &str) -> io::Result<()>;
}

/// A reading of the machine. A field the platform cannot report is `None`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SystemStatus {
    pub cpu_percent: Option<f32>,
    pub memory_used_mb: Option<u64>,
    pub memory_total_mb: Option<u64>,
    pub battery_percent: Option<u8>,
    pub charging: Option<bool>,
    pub online: Option<bool>,
}

pub trait StatusSource: Send + Sync {
    /// `None` when this build has no way to read the system.
    fn sample(&self) -> Option<SystemStatus>;
}

/// Where `memory.propose` puts a proposal. A proposal is only ever `proposed`: nothing reads
/// it back into a prompt until the user has approved it (spec §10, a later milestone).
pub trait MemoryInbox: Send + Sync {
    /// Returns the proposal's id, or an error text that is safe to show.
    fn propose(&self, text: &str) -> Result<String, String>;
}

pub struct NoLauncher;
impl AppLauncher for NoLauncher {
    fn launch(&self, _: &str) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "launching apps is not available in this build",
        ))
    }
}

pub struct NoStatus;
impl StatusSource for NoStatus {
    fn sample(&self) -> Option<SystemStatus> {
        None
    }
}

/// Keeps proposals in memory only (capped, deduplicated). Nothing is saved to disk and
/// nothing consumes them yet.
pub struct MemoryProposals {
    cap: usize,
    items: Mutex<Vec<(String, String)>>,
}

impl MemoryProposals {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            items: Mutex::new(Vec::new()),
        }
    }

    pub fn list(&self) -> Vec<(String, String)> {
        self.items.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl MemoryInbox for MemoryProposals {
    fn propose(&self, text: &str) -> Result<String, String> {
        let mut v = self.items.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((id, _)) = v.iter().find(|(_, t)| t == text) {
            return Ok(id.clone());
        }
        if v.len() >= self.cap {
            return Err("Too many proposed memories are waiting for review.".into());
        }
        let id = uuid::Uuid::now_v7().to_string();
        v.push((id.clone(), text.to_owned()));
        Ok(id)
    }
}

/// The adapters a tool call may use.
pub struct Ports {
    pub launcher: Arc<dyn AppLauncher>,
    pub status: Arc<dyn StatusSource>,
    pub memory: Arc<dyn MemoryInbox>,
}

impl Ports {
    /// Placeholders: launching and status unavailable, proposals kept in memory.
    pub fn placeholders() -> Self {
        Self {
            launcher: Arc::new(NoLauncher),
            status: Arc::new(NoStatus),
            memory: Arc::new(MemoryProposals::new(100)),
        }
    }
}
