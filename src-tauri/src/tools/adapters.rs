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
        use super::ports::PlayError as P;
        use crate::spotify::client::SpotifyError as E;
        let map = |e: E| match e {
            E::NotConfigured | E::NotConnected => P::NotConnected,
            E::Offline => P::Offline,
            E::Forbidden => P::Forbidden,
            E::NoDevice => P::NoDevice,
            E::NoResults => P::NoResults,
            E::NotPlaying => P::NotPlaying,
            E::Cancelled => P::Cancelled,
            E::TimedOut | E::Denied | E::PortBusy | E::Failed => P::Failed,
        };
        // Search first, then prefer the Spotify app on this computer when it is open: it
        // starts at once and needs neither Premium nor a device from Spotify's servers.
        let began = std::time::Instant::now();
        let found = self.spotify.find(words, cancel).await.map_err(map)?;
        let local = |uri: String| async move {
            tokio::task::spawn_blocking(move || crate::platform::media::play_spotify_uri(&uri))
                .await
        };
        let here = |f: &crate::spotify::client::Found| super::ports::PlayedTrack {
            title: f.title.clone(),
            artist: f.artist.clone(),
            device: Some("the Spotify app on this computer".into()),
        };
        let first = local(found.uri.clone()).await;
        self.spotify
            .note(format!("this computer's Spotify app: {first:?}"));
        if matches!(first, Ok(Ok(()))) {
            return Ok(here(&found));
        }
        // Not open here (or not a Mac): ask Spotify's servers, which can wake a device.
        // The account route also starts the Spotify app first when nothing is open (Windows).
        let budget = PLAY_BUDGET
            .saturating_sub(began.elapsed())
            .max(std::time::Duration::from_secs(8));
        let p = match self
            .spotify
            .play_found_or_start_app(&found, cancel, self.start_app.clone(), budget)
            .await
        {
            Ok(p) => p,
            // The servers took the song but could not start it. The cloud route may have
            // just opened the app, so ask the app on this computer once more.
            Err(e @ (E::NotPlaying | E::NoDevice)) => {
                let again = local(found.uri.clone()).await;
                self.spotify
                    .note(format!("this computer's Spotify app again: {again:?}"));
                if matches!(again, Ok(Ok(()))) {
                    return Ok(here(&found));
                }
                return Err(map(e));
            }
            Err(e) => return Err(map(e)),
        };
        Ok(super::ports::PlayedTrack {
            title: p.title,
            artist: p.artist,
            device: p.device,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::status::Battery;

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
