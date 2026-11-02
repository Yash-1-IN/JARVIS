//! `music.control` and `music.now_playing` (spec §9.4). They talk to the operating system's
//! media session, not to any one app and not to the network, so they work offline. The
//! action list is fixed: nothing the model or a song title says becomes a command.

use super::ports::{MediaAction, MediaError};
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
