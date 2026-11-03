//! `music.control` and `music.now_playing` (spec §9.4). They talk to the operating system's
//! media session, not to any one app and not to the network, so they work offline. The
//! action list is fixed: nothing the model or a song title says becomes a command.

use super::ports::{MediaAction, MediaError, PlayError};
use super::{
    Prepared, Tool, ToolContext, ToolError, ToolOutput, clean_for_summary, empty_schema, parse_args,
};
use crate::ipc::types::ToolRisk;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlArgs {
    action: Action,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
enum Action {
    Play,
    Pause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Mute,
}

impl Action {
    fn media(self) -> MediaAction {
        match self {
            Self::Play => MediaAction::Play,
            Self::Pause => MediaAction::Pause,
            Self::Next => MediaAction::Next,
            Self::Previous => MediaAction::Previous,
            Self::VolumeUp => MediaAction::VolumeUp,
            Self::VolumeDown => MediaAction::VolumeDown,
            Self::Mute => MediaAction::MuteToggle,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Next => "next",
            Self::Previous => "previous",
            Self::VolumeUp => "volume_up",
            Self::VolumeDown => "volume_down",
            Self::Mute => "mute",
        }
    }
    fn summary(self) -> &'static str {
        match self {
            Self::Play => "Resume the music",
            Self::Pause => "Pause the music",
            Self::Next => "Skip to the next track",
            Self::Previous => "Go back to the previous track",
            Self::VolumeUp => "Turn the volume up",
            Self::VolumeDown => "Turn the volume down",
            Self::Mute => "Mute or unmute the sound",
        }
    }
    fn done(self) -> &'static str {
        match self {
            Self::Play => "Resumed.",
            Self::Pause => "Paused.",
            Self::Next => "Skipped to the next track.",
            Self::Previous => "Went back to the previous track.",
            Self::VolumeUp => "Turned the volume up.",
            Self::VolumeDown => "Turned the volume down.",
            Self::Mute => "Toggled mute.",
        }
    }
}

fn to_tool_error(e: MediaError) -> ToolError {
    match e {
        MediaError::Unsupported => ToolError::Unavailable(
            "Controlling music is not available on this computer yet.".into(),
        ),
        MediaError::NothingPlaying => {
            ToolError::Unavailable("Nothing is playing right now.".into())
        }
        MediaError::Refused => ToolError::Failed("The player did not accept that.".into()),
        MediaError::NotAllowed => ToolError::Unavailable(
            "macOS has not allowed JARVIS to control that player. Switch JARVIS on under System Settings, Privacy and Security, Automation, then ask again.".into(),
        ),
        MediaError::Failed => ToolError::Failed("Controlling the music failed.".into()),
    }
}

pub struct MusicControl;

#[async_trait]
impl Tool for MusicControl {
    fn name(&self) -> &'static str {
        "music.control"
    }
    fn description(&self) -> &'static str {
        "Control whatever is playing music or video on this computer: play (resume), pause, next, previous, volume_up, volume_down, or mute (toggles). Works with Spotify and any player that shows media controls. It cannot choose a song."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["play", "pause", "next", "previous", "volume_up", "volume_down", "mute"]
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }
    fn base_risk(&self) -> ToolRisk {
        ToolRisk::Low
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(5)
    }
    fn prepare(&self, args: &Value, _: &ToolContext) -> Result<Prepared, ToolError> {
        let a: ControlArgs = parse_args(args)?;
        Ok(Prepared {
            arg_risk: None,
            targets: vec![a.action.name().to_string()],
            summary: a.action.summary().to_string(),
        })
    }
    async fn run(
        &self,
        args: &Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let a: ControlArgs = parse_args(args)?;
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let media = ctx.ports.media.clone();
        let action = a.action.media();
        tokio::task::spawn_blocking(move || media.act(action))
            .await
            .map_err(|_| ToolError::Failed("Controlling the music failed.".into()))?
            .map_err(to_tool_error)?;
        Ok(ToolOutput {
            text: a.action.done().to_string(),
            external_content: false,
        })
    }
}

pub struct MusicNowPlaying;

#[async_trait]
impl Tool for MusicNowPlaying {
    fn name(&self) -> &'static str {
        "music.now_playing"
    }
    fn description(&self) -> &'static str {
        "Say which track the computer's media player is on (title, artist, and whether it is playing). The names come from the playing app and are untrusted text."
    }
    fn input_schema(&self) -> Value {
        empty_schema()
    }
    fn base_risk(&self) -> ToolRisk {
        ToolRisk::Low
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(5)
    }
    fn prepare(&self, args: &Value, _: &ToolContext) -> Result<Prepared, ToolError> {
        let _: NoArgs = parse_args(args)?;
        Ok(Prepared {
            arg_risk: None,
            targets: vec![],
            summary: "See what is playing".into(),
        })
    }
    async fn run(
        &self,
        args: &Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let _: NoArgs = parse_args(args)?;
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let media = ctx.ports.media.clone();
        let now = tokio::task::spawn_blocking(move || media.now_playing())
            .await
            .map_err(|_| ToolError::Failed("Could not read what is playing.".into()))?
            .map_err(to_tool_error)?;
        Ok(match now {
            None => ToolOutput {
                text: "Nothing is playing right now.".into(),
                external_content: false,
            },
            Some(n) => {
                let (title, artist) = (clean_for_summary(&n.title), clean_for_summary(&n.artist));
                let by = if artist.is_empty() {
                    String::new()
                } else {
                    format!(" by {artist}")
                };
                let state = if n.playing { "playing" } else { "paused" };
                ToolOutput {
                    text: format!("Now playing: {title}{by} ({state})."),
                    // Track names come from another app, so they count as outside content.
                    external_content: true,
                }
            }
        })
    }
}

/// Longest search words accepted (the Spotify client cuts at the same length).
const MAX_QUERY_CHARS: usize = 100;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayArgs {
    query: String,
}

fn play_query(q: &str) -> Result<String, ToolError> {
    let q: String = q.chars().filter(|c| !c.is_control()).collect();
    let q = q.trim();
    if q.is_empty() || q.chars().count() > MAX_QUERY_CHARS {
        return Err(ToolError::Invalid(format!(
            "Invalid arguments: query must be 1 to {MAX_QUERY_CHARS} characters."
        )));
    }
    Ok(q.to_owned())
}

fn play_error(e: PlayError) -> ToolError {
    match e {
        PlayError::NotConnected => ToolError::Unavailable(
            "Spotify is not connected. Add your Client ID and press Connect under Settings, Music."
                .into(),
        ),
        PlayError::Offline => ToolError::Unavailable("Spotify could not be reached.".into()),
        PlayError::Forbidden => ToolError::Failed(
            "Spotify did not allow that. Choosing a song needs Spotify Premium.".into(),
        ),
        PlayError::NoDevice => ToolError::Unavailable(
            "Open the Spotify app first, so there is somewhere to play.".into(),
        ),
        PlayError::NoResults => ToolError::Unavailable("I could not find that on Spotify.".into()),
        PlayError::NotPlaying => ToolError::Failed(
            "Spotify accepted the song but nothing started. Open the Spotify app, press play on any song once, then ask again."
                .into(),
        ),
        PlayError::Cancelled => ToolError::Cancelled,
        PlayError::Failed => ToolError::Failed("Playing that on Spotify failed.".into()),
    }
}

/// `music.play`: search Spotify for the words and play the top track on the person's own
/// device. Low risk like the other music tools: it only starts a song, it deletes and shares
/// nothing. What comes back (a title) is outside text, so it taints the conversation.
pub struct MusicPlay;

#[async_trait]
impl Tool for MusicPlay {
    fn name(&self) -> &'static str {
        "music.play"
    }
    fn description(&self) -> &'static str {
        "Play a song, artist or album on the person's Spotify. Give the words to search for, for example \"Bohemian Rhapsody Queen\". It plays the best match. Needs Spotify Premium and the Spotify app open."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "minLength": 1, "maxLength": MAX_QUERY_CHARS }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }
    fn base_risk(&self) -> ToolRisk {
        ToolRisk::Low
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(30)
    }
    fn prepare(&self, args: &Value, _: &ToolContext) -> Result<Prepared, ToolError> {
        let a: PlayArgs = parse_args(args)?;
        let q = play_query(&a.query)?;
        Ok(Prepared {
            arg_risk: None,
            targets: vec![clean_for_summary(&q)],
            summary: format!("Play \"{}\" on Spotify", clean_for_summary(&q)),
        })
    }
    async fn run(
        &self,
        args: &Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let a: PlayArgs = parse_args(args)?;
        let q = play_query(&a.query)?;
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let p = match ctx.ports.music.play(&q, &cancel).await {
            Ok(p) => p,
            Err(e) => {
                let mut te = play_error(e);
                // Failures that Spotify caused say what it answered, so they can be diagnosed.
                if matches!(
                    e,
                    PlayError::Failed | PlayError::NotPlaying | PlayError::NoDevice
                ) && let Some(d) = ctx.ports.music.detail()
                {
                    te = match te {
                        ToolError::Failed(m) => ToolError::Failed(format!("{m} ({d})")),
                        ToolError::Unavailable(m) => ToolError::Unavailable(format!("{m} ({d})")),
                        other => other,
                    };
                }
                return Err(te);
            }
        };
        let (title, artist) = (clean_for_summary(&p.title), clean_for_summary(&p.artist));
        let by = if artist.is_empty() {
            String::new()
        } else {
            format!(" by {artist}")
        };
        let on = match p.device.as_deref().map(clean_for_summary) {
            Some(d) if !d.is_empty() => format!(" on {d}"),
            _ => String::new(),
        };
        Ok(ToolOutput {
            text: format!("Playing {title}{by}{on}."),
            // The title comes from Spotify, so it counts as outside content.
            external_content: true,
        })
    }
}
