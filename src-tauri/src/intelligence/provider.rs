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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
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
