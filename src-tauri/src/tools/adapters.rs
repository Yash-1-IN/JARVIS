//! Connects the tool traits (`ports.rs`) to the real platform code in `platform/`. No
//! OS-specific code lives here: it only forwards and converts types.

use super::ports::{
    AppLauncher, MediaAction, MediaControl, MediaError, NowPlaying, Ports, StatusSource,
    SystemStatus,
};
use crate::platform::media;
use crate::platform::status::{self, BatteryState};
use std::io;
use std::sync::Arc;

pub struct PlatformLauncher;

impl AppLauncher for PlatformLauncher {
    fn launch(&self, app_id: &str) -> io::Result<()> {
        crate::platform::launch::launch(app_id)
    }
}

pub struct PlatformMedia;

impl MediaControl for PlatformMedia {
    fn act(&self, action: MediaAction) -> Result<(), MediaError> {
        media::send(match action {
            MediaAction::Play => media::Command::Play,
            MediaAction::Pause => media::Command::Pause,
            MediaAction::Next => media::Command::Next,
            MediaAction::Previous => media::Command::Previous,
            MediaAction::VolumeUp => media::Command::VolumeUp,
            MediaAction::VolumeDown => media::Command::VolumeDown,
            MediaAction::MuteToggle => media::Command::MuteToggle,
        })
        .map_err(convert_error)
    }

    fn now_playing(&self) -> Result<Option<NowPlaying>, MediaError> {
        media::now_playing()
            .map(|n| {
                n.map(|n| NowPlaying {
                    title: n.title,
                    artist: n.artist,
                    playing: n.playing,
                })
            })
            .map_err(convert_error)
    }
}

fn convert_error(e: media::MediaError) -> MediaError {
    match e {
        media::MediaError::Unsupported => MediaError::Unsupported,
        media::MediaError::NothingPlaying => MediaError::NothingPlaying,
        media::MediaError::Refused => MediaError::Refused,
        media::MediaError::NotAllowed => MediaError::NotAllowed,
        media::MediaError::Failed => MediaError::Failed,
    }
}

pub struct PlatformStatus;

impl StatusSource for PlatformStatus {
    fn sample(&self) -> Option<SystemStatus> {
        Some(convert(&status::sample()))
    }
}

/// Bytes become whole megabytes; a battery that cannot report charging leaves it unknown.
pub fn convert(s: &status::SystemStatus) -> SystemStatus {
    const MB: u64 = 1024 * 1024;
    SystemStatus {
        cpu_percent: Some(s.cpu_percent),
        memory_used_mb: Some(s.memory_used_bytes / MB),
        memory_total_mb: Some(s.memory_total_bytes / MB),
        battery_percent: s.battery.map(|b| b.percent.round().clamp(0.0, 100.0) as u8),
        charging: s.battery.and_then(|b| match b.state {
            BatteryState::Charging => Some(true),
            BatteryState::Discharging => Some(false),
            BatteryState::Full | BatteryState::Unknown => None,
        }),
        online: Some(s.network_up),
    }
}

/// The real adapters; `memory` is where proposed memories go.
pub fn platform_ports(
    memory: Arc<dyn super::ports::MemoryInbox>,
    music: Arc<dyn super::ports::MusicPlayer>,
) -> Ports {
    Ports {
        launcher: Arc::new(PlatformLauncher),
        status: Arc::new(PlatformStatus),
        memory,
        media: Arc::new(PlatformMedia),
        music,
    }
}

/// "Play X" through the Spotify client.
pub struct SpotifyPlayer {
    spotify: Arc<crate::spotify::client::Spotify>,
    /// Starts the Spotify app on this computer (Windows) when Spotify lists no device. Answers
    /// whether it was started.
    start_app: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl SpotifyPlayer {
    /// The real one: starts the installed Spotify app on Windows, does nothing elsewhere.
    pub fn new(spotify: Arc<crate::spotify::client::Spotify>) -> Self {
        Self {
            spotify,
            start_app: Arc::new(|| media::start_spotify_app().is_ok()),
        }
    }
}

/// Everything "play X" may take, as the `music.play` tool allows 30 s. Kept under it so a slow
/// Spotify ends with a trace of what it answered, not a bare timeout.
const PLAY_BUDGET: std::time::Duration = std::time::Duration::from_secs(26);

use crate::platform::media::MediaError as LocalError;
use crate::spotify::client::{Found, SpotifyError as E};

fn map_error(e: E) -> super::ports::PlayError {
    use super::ports::PlayError as P;
    match e {
        E::NotConfigured | E::NotConnected => P::NotConnected,
        E::Offline => P::Offline,
        E::Forbidden => P::Forbidden,
        E::NoDevice => P::NoDevice,
        E::NoResults => P::NoResults,
        E::NotPlaying => P::NotPlaying,
        E::NeedsReconnect => P::NeedsReconnect,
        E::Cancelled => P::Cancelled,
        E::TimedOut | E::Denied | E::PortBusy | E::Failed => P::Failed,
    }
}

/// How long to wait for the Spotify app on this computer to start and take a song (macOS), and
/// how often to ask it meanwhile.
const LOCAL_START_WAIT: std::time::Duration = std::time::Duration::from_secs(12);
const LOCAL_POLL: std::time::Duration = std::time::Duration::from_millis(700);

/// Calls `attempt` until it says yes, up to `total`, `every` apart. A cancel ends it at once.
pub(crate) async fn retry_until<F, Fut>(
    mut attempt: F,
    total: std::time::Duration,
    every: std::time::Duration,
    cancel: &tokio_util::sync::CancellationToken,
) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let end = std::time::Instant::now() + total;
    loop {
        if attempt().await {
            return true;
        }
        if cancel.is_cancelled() || std::time::Instant::now() + every >= end {
            return false;
        }
        tokio::select! {
            _ = cancel.cancelled() => return false,
            _ = tokio::time::sleep(every) => {}
        }
    }
}

impl SpotifyPlayer {
    async fn local(uri: String) -> Result<Result<(), LocalError>, tokio::task::JoinError> {
        tokio::task::spawn_blocking(move || crate::platform::media::play_spotify_uri(&uri)).await
    }

    /// The last resort: the song inside its album (the album's next song then follows).
    async fn local_in_album(uri: String) -> Result<Result<(), LocalError>, tokio::task::JoinError> {
        tokio::task::spawn_blocking(move || crate::platform::media::play_spotify_uri_in_album(&uri))
            .await
    }

    fn here(f: &Found) -> super::ports::PlayedTrack {
        super::ports::PlayedTrack {
            title: f.title.clone(),
            artist: f.artist.clone(),
            device: Some("the Spotify app on this computer".into()),
        }
    }

    /// Plays what a search or lookup found: in the Spotify app on this computer when that is
    /// possible, and otherwise through Spotify's servers.
    async fn play_found(
        &self,
        found: Found,
        began: std::time::Instant,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<super::ports::PlayedTrack, super::ports::PlayError> {
        // The app on this computer first: it starts at once and needs neither Premium nor a device
        // from Spotify's servers.
        let first = Self::local(found.uri.clone()).await;
        self.spotify
            .note(format!("this computer's Spotify app: {first:?}"));
        if matches!(first, Ok(Ok(()))) {
            return Ok(Self::here(&found));
        }
        // Open, but it refused the way that plays a song on its own: play it inside its album.
        if matches!(first, Ok(Err(LocalError::Failed))) {
            let album = Self::local_in_album(found.uri.clone()).await;
            self.spotify.note(format!(
                "this computer's Spotify app, album form: {album:?}"
            ));
            if matches!(album, Ok(Ok(()))) {
                return Ok(Self::here(&found));
            }
        }
        // Closed, and this computer can start it (macOS; Windows is handled below): the person
        // asked for a song, so start the app, give it a few seconds to load, and play.
        if matches!(first, Ok(Err(LocalError::NothingPlaying))) {
            let start = self.start_app.clone();
            let started = tokio::task::spawn_blocking(move || start())
                .await
                .unwrap_or(false);
            if started {
                self.spotify.note("started the Spotify app".into());
                let uri = found.uri.clone();
                let taken = retry_until(
                    || {
                        let uri = uri.clone();
                        async move { matches!(Self::local(uri).await, Ok(Ok(()))) }
                    },
                    LOCAL_START_WAIT,
                    LOCAL_POLL,
                    cancel,
                )
                .await;
                if taken {
                    return Ok(Self::here(&found));
                }
                // Still refusing after the wait: play it inside its album rather than not at all.
                let album = Self::local_in_album(found.uri.clone()).await;
                self.spotify.note(format!(
                    "the Spotify app did not take the song in time; album form: {album:?}"
                ));
                if matches!(album, Ok(Ok(()))) {
                    return Ok(Self::here(&found));
                }
            }
        }
        if cancel.is_cancelled() {
            return Err(super::ports::PlayError::Cancelled);
        }
        // Not open here (or not a Mac): ask Spotify's servers, which can wake a device. The
        // account route also starts the Spotify app first when nothing is open (Windows).
        let budget = PLAY_BUDGET
            .saturating_sub(began.elapsed())
            .max(std::time::Duration::from_secs(8));
        let p = match self
            .spotify
            .play_found_or_start_app(&found, cancel, self.start_app.clone(), budget)
            .await
        {
            Ok(p) => p,
            // The servers took it but could not start it. The cloud route may have just opened
            // the app, so ask the app on this computer once more.
            Err(e @ (E::NotPlaying | E::NoDevice)) => {
                let again = Self::local(found.uri.clone()).await;
                self.spotify
                    .note(format!("this computer's Spotify app again: {again:?}"));
                if matches!(again, Ok(Ok(()))) {
                    return Ok(Self::here(&found));
                }
                return Err(map_error(e));
            }
            Err(e) => return Err(map_error(e)),
        };
        Ok(super::ports::PlayedTrack {
            title: p.title,
            artist: p.artist,
            device: p.device,
        })
    }
}

#[async_trait::async_trait]
impl super::ports::MusicPlayer for SpotifyPlayer {
    fn detail(&self) -> Option<String> {
        Some(self.spotify.last_trace()).filter(|t| !t.is_empty())
    }

    async fn play(
        &self,
        words: &str,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<super::ports::PlayedTrack, super::ports::PlayError> {
        let began = std::time::Instant::now();
        let found = self.spotify.find(words, cancel).await.map_err(map_error)?;
        self.play_found(found, began, cancel).await
    }

    async fn play_playlist(
        &self,
        name: &str,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<super::ports::PlayedTrack, super::ports::PlayError> {
        let began = std::time::Instant::now();
        let found = self
            .spotify
            .find_playlist(name, cancel)
            .await
            .map_err(map_error)?;
        self.play_found(found, began, cancel).await
    }

    async fn play_liked(
        &self,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<super::ports::PlayedTrack, super::ports::PlayError> {
        let began = std::time::Instant::now();
        let found = self.spotify.find_liked(cancel).await.map_err(map_error)?;
        self.play_found(found, began, cancel).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::status::Battery;

    #[tokio::test]
    async fn retrying_stops_on_success_on_cancel_and_at_the_end_of_the_time() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::time::Duration;
        let cancel = tokio_util::sync::CancellationToken::new();
        let n = Arc::new(AtomicU32::new(0));
        let c = n.clone();
        // Succeeds on the third try.
        assert!(
            retry_until(
                || {
                    let c = c.clone();
                    async move { c.fetch_add(1, Ordering::SeqCst) >= 2 }
                },
                Duration::from_secs(5),
                Duration::from_millis(5),
                &cancel
            )
            .await
        );
        assert_eq!(n.load(Ordering::SeqCst), 3);
        // Never succeeds: gives up when the time is up, not before trying.
        let tries = Arc::new(AtomicU32::new(0));
        let t = tries.clone();
        assert!(
            !retry_until(
                || {
                    let t = t.clone();
                    async move {
                        t.fetch_add(1, Ordering::SeqCst);
                        false
                    }
                },
                Duration::from_millis(60),
                Duration::from_millis(20),
                &cancel
            )
            .await
        );
        assert!(tries.load(Ordering::SeqCst) >= 2);
        // A cancel ends it after the current try.
        cancel.cancel();
        let started = std::time::Instant::now();
        assert!(
            !retry_until(
                || async { false },
                Duration::from_secs(30),
                Duration::from_secs(10),
                &cancel
            )
            .await
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn conversion_maps_units_and_battery_states() {
        let s = status::SystemStatus {
            cpu_percent: 12.0,
            memory_used_bytes: 6 * 1024 * 1024 * 1024,
            memory_total_bytes: 16 * 1024 * 1024 * 1024,
            battery: Some(Battery {
                percent: 80.6,
                state: BatteryState::Charging,
            }),
            network_up: true,
        };
        let c = convert(&s);
        assert_eq!(
            (c.memory_used_mb, c.memory_total_mb),
            (Some(6144), Some(16384))
        );
        assert_eq!((c.battery_percent, c.charging), (Some(81), Some(true)));
        assert_eq!(c.online, Some(true));
        let none = convert(&status::SystemStatus { battery: None, ..s });
        assert_eq!((none.battery_percent, none.charging), (None, None));
    }

    #[test]
    fn the_real_launcher_refuses_a_malformed_id_without_starting_anything() {
        let e = PlatformLauncher.launch("; rm -rf ~").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn the_real_status_source_answers() {
        assert!(PlatformStatus.sample().is_some());
    }
}
