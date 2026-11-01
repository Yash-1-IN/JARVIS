//! Provider-neutral turn types and the single `ModelProvider` trait (spec §7.1).

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

/// A structured part of a message, used when the model calls tools.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

/// One message. Plain chat uses `content` alone; a tool round trip adds `blocks`, which then
/// replace `content` on the wire (`content` keeps the text part for size accounting).
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub blocks: Vec<Block>,
}

impl Message {
    pub fn text(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            blocks: Vec::new(),
        }
    }

    /// Characters the message carries, counting tool arguments and results.
    pub fn size(&self) -> usize {
        self.content.len()
            + self
                .blocks
                .iter()
                .map(|b| match b {
                    Block::Text { .. } => 0,
                    Block::ToolUse { input, name, .. } => name.len() + input.to_string().len(),
                    Block::ToolResult { content, .. } => content.len(),
                })
                .sum::<usize>()
    }
}

impl Serialize for Message {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("Message", 2)?;
        st.serialize_field("role", &self.role)?;
        if self.blocks.is_empty() {
            st.serialize_field("content", &self.content)?;
        } else {
            st.serialize_field("content", &self.blocks)?;
        }
        st.end()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema for the arguments.
    pub input_schema: serde_json::Value,
}

/// How hard the model thinks. Short spoken replies use `Low`: lower latency and cost.
/// More levels are added when something needs them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    Low,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TurnRequest {
    /// From config, never hard-coded.
    pub model: String,
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    /// Thinking tokens count toward this limit, so keep it generous.
    pub max_output_tokens: u32,
    pub effort: Effort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    /// The provider declined the request.
    Refusal,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TurnEvent {
    TextDelta(String),
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
    Done {
        stop: StopReason,
        usage: Usage,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("invalid or missing API key")]
    Auth,
    #[error("rate limited")]
    RateLimit,
    #[error("provider unavailable: {0}")]
    Unavailable(String),
    #[error("network offline")]
    Offline,
    #[error("cancelled")]
    Cancelled,
    #[error("malformed provider response: {0}")]
    Protocol(String),
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn stream_turn(
        &self,
        req: TurnRequest,
        cancel: CancellationToken,
    ) -> Result<BoxStream<'static, Result<TurnEvent, ProviderError>>, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plain_message_serialises_as_a_string() {
        let v = serde_json::to_value(Message::text(Role::User, "hi")).unwrap();
        assert_eq!(v, json!({"role": "user", "content": "hi"}));
    }

    #[test]
    fn tool_round_trip_serialises_as_blocks() {
        let m = Message {
            role: Role::Assistant,
            content: String::new(),
            blocks: vec![Block::ToolUse {
                id: "t1".into(),
                name: "time.now".into(),
                input: json!({}),
            }],
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["content"][0]["type"], "tool_use");
        let r = Message {
            role: Role::User,
            content: String::new(),
            blocks: vec![Block::ToolResult {
                tool_use_id: "t1".into(),
                content: "ok".into(),
                is_error: false,
            }],
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap()["content"][0]["type"],
            "tool_result"
        );
        assert!(r.size() >= 2);
    }
}
