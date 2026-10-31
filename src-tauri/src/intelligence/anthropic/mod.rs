//! Own Anthropic Messages client on reqwest + SSE (ADR-011). No official Rust SDK exists.

use super::provider::*;
use super::sse::{SseEvent, SseParser};
use async_trait::async_trait;
use futures::channel::mpsc;
use futures::stream::BoxStream;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const API_VERSION: &str = "2023-06-01";

#[derive(Clone)]
pub struct AnthropicConfig {
    pub base_url: String,
    pub api_key: String,
    pub connect_timeout: Duration,
    pub first_token_timeout: Duration,
    pub turn_timeout: Duration,
    /// Retries for 429/5xx before the first token (spec §7.3: twice).
    pub max_retries: u32,
    pub backoff_base: Duration,
}

impl AnthropicConfig {
    pub fn new(api_key: String) -> Self {
        Self {
            base_url: "https://api.anthropic.com".into(),
            api_key,
            connect_timeout: Duration::from_secs(5),
            first_token_timeout: Duration::from_secs(10),
            turn_timeout: Duration::from_secs(60),
            max_retries: 2,
            backoff_base: Duration::from_millis(400),
        }
    }
}

// The key must never reach logs.
impl std::fmt::Debug for AnthropicConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"***")
            .finish_non_exhaustive()
    }
}

pub struct AnthropicProvider {
    cfg: AnthropicConfig,
    http: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(cfg: AnthropicConfig) -> Result<Self, ProviderError> {
        let http = reqwest::Client::builder()
            .connect_timeout(cfg.connect_timeout)
            .build()
            .map_err(|e| ProviderError::Unavailable(e.to_string()))?;
        Ok(Self { cfg, http })
    }

    fn body(req: &TurnRequest) -> Value {
        let mut b = json!({
            "model": req.model,
            "max_tokens": req.max_output_tokens,
            "system": req.system,
            "messages": req.messages,
            "stream": true,
        });
        if !req.tools.is_empty() {
            b["tools"] = json!(req.tools);
        }
        b
    }

    async fn send_once(&self, body: &Value) -> Result<reqwest::Response, ProviderError> {
        let resp = self
            .http
            .post(format!("{}/v1/messages", self.cfg.base_url))
            .header("x-api-key", &self.cfg.api_key)
            .header("anthropic-version", API_VERSION)
            .json(body)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() {
                    ProviderError::Offline
                } else {
                    ProviderError::Unavailable(e.without_url().to_string())
                }
            })?;
        match resp.status().as_u16() {
            200..=299 => Ok(resp),
            401 | 403 => Err(ProviderError::Auth),
            429 => Err(ProviderError::RateLimit),
            s if s >= 500 => Err(ProviderError::Unavailable(format!("HTTP {s}"))),
            s => Err(ProviderError::Protocol(format!("unexpected HTTP {s}"))),
        }
    }

    async fn send_with_retry(
        &self,
        body: &Value,
        cancel: &CancellationToken,
    ) -> Result<reqwest::Response, ProviderError> {
        let mut attempt = 0;
        loop {
            let res = tokio::select! {
                _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
                r = self.send_once(body) => r,
            };
            match res {
                Err(e @ (ProviderError::RateLimit | ProviderError::Unavailable(_)))
                    if attempt < self.cfg.max_retries =>
                {
                    attempt += 1;
                    // Exponential backoff plus jitter (clock noise, no rand dependency).
                    let base_ms = self.cfg.backoff_base.as_millis() as u64;
                    let noise = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos() as u64)
                        .unwrap_or(0);
                    let wait = self.cfg.backoff_base * 2u32.pow(attempt - 1)
                        + Duration::from_millis(noise % base_ms.max(1));
                    tracing::warn!(error = %e, attempt, "provider error before first token, retrying");
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
                        _ = tokio::time::sleep(wait) => {}
                    }
                }
                other => return other,
            }
        }
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    async fn stream_turn(
        &self,
        req: TurnRequest,
        cancel: CancellationToken,
    ) -> Result<BoxStream<'static, Result<TurnEvent, ProviderError>>, ProviderError> {
        let body = Self::body(&req);
        let resp = self.send_with_retry(&body, &cancel).await?;
        let (mut tx, rx) = mpsc::channel::<Result<TurnEvent, ProviderError>>(32);
        let first_token = self.cfg.first_token_timeout;
        let total = self.cfg.turn_timeout;

        tokio::spawn(async move {
            let mut bytes = resp.bytes_stream();
            let mut parser = SseParser::default();
            let mut acc = Accumulator::default();
            let deadline = tokio::time::Instant::now() + total;
            let mut got_data = false;
            loop {
                let wait = if got_data {
                    deadline.saturating_duration_since(tokio::time::Instant::now())
                } else {
                    first_token
                };
                let next = tokio::select! {
                    _ = cancel.cancelled() => {
                        let _ = tx.send(Err(ProviderError::Cancelled)).await;
                        return;
                    }
                    r = tokio::time::timeout(wait, bytes.next()) => r,
                };
                let chunk = match next {
                    Err(_) => {
                        let msg = if got_data {
                            "turn timed out"
                        } else {
                            "no first token"
                        };
                        let _ = tx.send(Err(ProviderError::Unavailable(msg.into()))).await;
                        return;
                    }
                    Ok(None) => {
                        let _ = tx
                            .send(Err(ProviderError::Protocol("stream ended early".into())))
                            .await;
                        return;
                    }
                    Ok(Some(Err(e))) => {
                        let _ = tx
                            .send(Err(ProviderError::Unavailable(e.without_url().to_string())))
                            .await;
                        return;
                    }
                    Ok(Some(Ok(c))) => c,
                };
                got_data = true;
                for ev in parser.push(&chunk) {
                    match acc.handle(ev) {
                        Ok(items) => {
                            for item in items {
                                let done = matches!(item, TurnEvent::Done { .. });
                                if tx.send(Ok(item)).await.is_err() {
                                    return; // receiver dropped
                                }
                                if done {
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e)).await;
                            return;
                        }
                    }
                }
            }
        });
        Ok(rx.boxed())
    }
}

struct ToolBlock {
    id: String,
    name: String,
    json: String,
}

#[derive(Default)]
struct Accumulator {
    tools: HashMap<u64, ToolBlock>,
    usage: Usage,
    stop: Option<StopReason>,
}

impl Accumulator {
    fn handle(&mut self, ev: SseEvent) -> Result<Vec<TurnEvent>, ProviderError> {
        let name = ev.event.as_deref().unwrap_or("");
        if name == "ping" || ev.data.is_empty() {
            return Ok(vec![]);
        }
        let v: Value = serde_json::from_str(&ev.data)
            .map_err(|e| ProviderError::Protocol(format!("bad event JSON: {e}")))?;
        let index = v["index"].as_u64().unwrap_or(0);
        match name {
            "message_start" => {
                self.usage.input_tokens =
                    v["message"]["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32;
                Ok(vec![])
            }
            "content_block_start" => {
                let b = &v["content_block"];
                if b["type"] == "tool_use" {
                    self.tools.insert(
                        index,
                        ToolBlock {
                            id: b["id"].as_str().unwrap_or("").into(),
                            name: b["name"].as_str().unwrap_or("").into(),
                            json: String::new(),
                        },
                    );
                }
                Ok(vec![])
            }
            "content_block_delta" => {
                let d = &v["delta"];
                match d["type"].as_str() {
                    Some("text_delta") => Ok(vec![TurnEvent::TextDelta(
                        d["text"].as_str().unwrap_or("").into(),
                    )]),
                    Some("input_json_delta") => {
                        if let Some(t) = self.tools.get_mut(&index) {
                            t.json.push_str(d["partial_json"].as_str().unwrap_or(""));
                        }
                        Ok(vec![])
                    }
                    _ => Ok(vec![]),
                }
            }
            "content_block_stop" => match self.tools.remove(&index) {
                Some(t) => {
                    let args = if t.json.trim().is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&t.json).map_err(|e| {
                            ProviderError::Protocol(format!("bad tool arguments: {e}"))
                        })?
                    };
                    Ok(vec![TurnEvent::ToolCall {
                        id: t.id,
                        name: t.name,
                        args,
                    }])
                }
                None => Ok(vec![]),
            },
            "message_delta" => {
                self.usage.output_tokens = v["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;
                self.stop = Some(match v["delta"]["stop_reason"].as_str() {
                    Some("end_turn") => StopReason::EndTurn,
                    Some("tool_use") => StopReason::ToolUse,
                    Some("max_tokens") => StopReason::MaxTokens,
                    _ => StopReason::Other,
                });
                Ok(vec![])
            }
            "message_stop" => Ok(vec![TurnEvent::Done {
                stop: self.stop.unwrap_or(StopReason::Other),
                usage: self.usage,
            }]),
            "error" => Err(match v["error"]["type"].as_str() {
                Some("authentication_error") | Some("permission_error") => ProviderError::Auth,
                Some("rate_limit_error") => ProviderError::RateLimit,
                Some(t) => ProviderError::Unavailable(t.to_string()),
                None => ProviderError::Protocol("error event".into()),
            }),
            _ => Ok(vec![]),
        }
    }
}

#[cfg(test)]
mod tests;
