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

/// What can be asked of whatever is playing media. A fixed list: no text reaches the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    Play,
    Pause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    /// The system mute key toggles.
    MuteToggle,
}

/// The track the system reports. The strings come from the playing app: untrusted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaError {
    /// This build or operating system has no adapter yet.
    Unsupported,
    /// No app has a media session open.
    NothingPlaying,
    /// An app has a session but did not accept the command.
    Refused,
    /// The operating system has not let JARVIS control the app (macOS Automation permission).
    NotAllowed,
    Failed,
}

/// Pause, skip, volume and "what is playing", through the operating system. Blocking; the
/// tools call it on a blocking thread.
pub trait MediaControl: Send + Sync {
    fn act(&self, action: MediaAction) -> Result<(), MediaError>;
    /// `Ok(None)` means nothing is playing.
    fn now_playing(&self) -> Result<Option<NowPlaying>, MediaError>;
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

pub struct NoMedia;
impl MediaControl for NoMedia {
    fn act(&self, _: MediaAction) -> Result<(), MediaError> {
        Err(MediaError::Unsupported)
    }
    fn now_playing(&self) -> Result<Option<NowPlaying>, MediaError> {
        Err(MediaError::Unsupported)
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

/// The track Spotify started. The words come from Spotify: untrusted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayedTrack {
    pub title: String,
    pub artist: String,
    /// The Spotify device playing it, when known.
    pub device: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayError {
    /// No Client ID saved, or not signed in to Spotify.
    NotConnected,
    Offline,
    /// Spotify says this needs Premium.
    Forbidden,
    /// No Spotify app or device is open.
    NoDevice,
    NoResults,
    /// Spotify accepted the song but nothing is playing.
    NotPlaying,
    /// Signed in before JARVIS asked to read playlists: sign in again to allow it.
    NeedsReconnect,
    Cancelled,
    Failed,
}

/// "Play <words>" through the person's own Spotify account (music step 2, issue #68).
#[async_trait::async_trait]
pub trait MusicPlayer: Send + Sync {
    async fn play(
        &self,
        words: &str,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<PlayedTrack, PlayError>;
    /// "Play my workout playlist": one of the person's own playlists, found by name.
    async fn play_playlist(
        &self,
        _name: &str,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<PlayedTrack, PlayError> {
        Err(PlayError::NotConnected)
    }
    /// "Play my liked songs".
    async fn play_liked(
        &self,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<PlayedTrack, PlayError> {
        Err(PlayError::NotConnected)
    }
    /// A short, secret-free account of the last attempt (steps and HTTP statuses), appended
    /// to failures so they can be diagnosed.
    fn detail(&self) -> Option<String> {
        None
    }
}

pub struct NoMusicPlayer;
#[async_trait::async_trait]
impl MusicPlayer for NoMusicPlayer {
    async fn play(
        &self,
        _: &str,
        _: &tokio_util::sync::CancellationToken,
    ) -> Result<PlayedTrack, PlayError> {
        Err(PlayError::NotConnected)
    }
}

/// The adapters a tool call may use.
pub struct Ports {
    pub launcher: Arc<dyn AppLauncher>,
    pub status: Arc<dyn StatusSource>,
    pub memory: Arc<dyn MemoryInbox>,
    pub media: Arc<dyn MediaControl>,
    pub music: Arc<dyn MusicPlayer>,
}

impl Ports {
    /// Placeholders: launching, status and media control unavailable, proposals kept in memory.
    pub fn placeholders() -> Self {
        Self {
            launcher: Arc::new(NoLauncher),
            status: Arc::new(NoStatus),
            memory: Arc::new(MemoryProposals::new(100)),
            media: Arc::new(NoMedia),
            music: Arc::new(NoMusicPlayer),
        }
    }
}
