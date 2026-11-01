//! Runs one user turn end to end (spec §7.2): local intent router first, then the
//! spend check, then the cloud model, with every step cancellable. It talks to the
//! UI only through `TurnSink`, so it is testable without Tauri.
//!
//! Tools are not wired yet (M4): the model gets none, and the turn limit of five
//! tool calls does not apply until then.

use super::{Orchestrator, TransitionError};
use crate::intelligence::intents::{self, Intent};
use crate::intelligence::provider::{
    Effort, Message, ModelProvider, ProviderError, Role, StopReason as ProviderStop, TurnEvent,
    TurnRequest, Usage,
};
use crate::intelligence::spend::{CapKind, Day, Price, SpendMeter};
use crate::ipc::types::{
    AssistantPhase, ErrorCode, InterruptReason, JarvisError, Snapshot, StopReason, TurnInterrupted,
    TurnStreamEvent, UsageSummary,
};
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

const PERSONA: &str = include_str!("../../resources/prompts/persona.md");
/// Thinking tokens count toward `max_tokens`, so this is generous; short spoken replies
/// come from the persona prompt and low effort, not from a tight limit.
const MAX_OUTPUT_TOKENS: u32 = 4096;
/// Rough budget for conversation history sent to the model (~8k tokens at 4 chars each).
const HISTORY_CHAR_BUDGET: usize = 32_000;

/// Where a turn reports to. The Tauri implementation wraps a Channel and the app handle.
pub trait TurnSink: Send + Sync {
    fn event(&self, e: TurnStreamEvent);
    fn snapshot(&self, s: Snapshot);
    /// Called after spend is recorded (the `usage.updated` event).
    fn usage(&self, _u: UsageSummary) {}
    /// Called when a running turn is cut short (the `turn.interrupted` event).
    fn interrupted(&self, _i: TurnInterrupted) {}
}

type ProviderFactory = dyn Fn() -> Option<Arc<dyn ModelProvider>> + Send + Sync;

pub struct TurnConfig {
    pub model: String,
    pub price: Price,
}

pub struct TurnRunner {
    orch: Arc<Orchestrator>,
    spend: Arc<SpendMeter>,
    provider: Box<ProviderFactory>,
    config: Box<dyn Fn() -> TurnConfig + Send + Sync>,
    now_secs: Box<dyn Fn() -> u64 + Send + Sync>,
    history: Mutex<Vec<Message>>,
    last_reply: Mutex<Option<String>>,
    tokens: Mutex<HashMap<String, CancellationToken>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn err(code: ErrorCode, message: &str, retryable: bool, id: Option<&str>) -> JarvisError {
    JarvisError {
        code,
        message: message.into(),
        retryable,
        request_id: id.map(str::to_owned),
    }
}

fn map_provider_error(e: &ProviderError, id: &str) -> JarvisError {
    match e {
        ProviderError::Auth => err(
            ErrorCode::ProviderAuth,
            "The API key was rejected.",
            false,
            Some(id),
        ),
        ProviderError::RateLimit => err(
            ErrorCode::ProviderRateLimit,
            "The provider is rate limiting requests.",
            true,
            Some(id),
        ),
        ProviderError::Unavailable(_) => err(
            ErrorCode::ProviderUnavailable,
            "The provider did not respond in time.",
            true,
            Some(id),
        ),
        ProviderError::Offline => err(
            ErrorCode::NetworkOffline,
            "No network connection.",
            true,
            Some(id),
        ),
        ProviderError::Protocol(_) | ProviderError::Cancelled => err(
            ErrorCode::Internal,
            "The provider sent something unexpected.",
            false,
            Some(id),
        ),
    }
}

impl TurnRunner {
    pub fn new(
        orch: Arc<Orchestrator>,
        spend: Arc<SpendMeter>,
        provider: Box<ProviderFactory>,
        config: Box<dyn Fn() -> TurnConfig + Send + Sync>,
        now_secs: Box<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        Self {
            orch,
            spend,
            provider,
            config,
            now_secs,
            history: Mutex::new(Vec::new()),
            last_reply: Mutex::new(None),
            tokens: Mutex::new(HashMap::new()),
        }
    }

    fn today(&self) -> Day {
        Day::from_unix_secs((self.now_secs)())
    }

    /// Cancels a turn's whole tree. Returns false if it is not active (idempotent).
    /// Emits `turn.interrupted` only when a running turn was actually cut short.
    pub fn cancel(&self, request_id: &str, reason: InterruptReason, sink: &dyn TurnSink) -> bool {
        let token = lock(&self.tokens).get(request_id).cloned();
        if let Some(t) = &token {
            t.cancel();
        }
        match self.orch.cancel(request_id) {
            Ok(s) => {
                sink.snapshot(s);
                sink.interrupted(TurnInterrupted {
                    request_id: request_id.to_owned(),
                    reason,
                });
                true
            }
            Err(_) => token.is_some(),
        }
    }

    /// Validates and starts a turn, then streams on `tokio::spawn`. Returns the request id
    /// at once; events follow on the sink and always end with exactly one `done`.
    pub fn start(
        self: &Arc<Self>,
        text: String,
        sink: Arc<dyn TurnSink>,
    ) -> Result<String, JarvisError> {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return Err(err(ErrorCode::Internal, "Empty message.", false, None));
        }
        // Commands that must work even while a turn is running (stop, mute) are not turns.
        if let Some(intent @ (Intent::Stop | Intent::Mute | Intent::Unmute)) = intents::route(&text)
        {
            return Ok(self.run_control(intent, sink.as_ref()));
        }
        let (id, snap) = self.orch.begin_turn().map_err(|e| match e {
            TransitionError::Illegal { .. } => err(
                ErrorCode::Internal,
                "JARVIS is busy with another request.",
                false,
                None,
            ),
            _ => err(
                ErrorCode::Internal,
                "Could not start the request.",
                false,
                None,
            ),
        })?;
        sink.snapshot(snap);
        let token = CancellationToken::new();
        lock(&self.tokens).insert(id.clone(), token.clone());
        let me = Arc::clone(self);
        let rid = id.clone();
        tokio::spawn(async move {
            me.run(&rid, text, token, sink.as_ref()).await;
            lock(&me.tokens).remove(&rid);
        });
        Ok(id)
    }

    fn run_control(&self, intent: Intent, sink: &dyn TurnSink) -> String {
        let id = uuid::Uuid::now_v7().to_string();
        let reply = match intent {
            Intent::Mute => {
                sink.snapshot(self.orch.set_mute(true));
                "Muted."
            }
            Intent::Unmute => {
                sink.snapshot(self.orch.set_mute(false));
                "Microphone back on."
            }
            _ => {
                let active = self.orch.snapshot().active_request_id;
                match active {
                    Some(a) => {
                        self.cancel(&a, InterruptReason::UserCancel, sink);
                        "Stopped."
                    }
                    None => "Nothing to stop.",
                }
            }
        };
        sink.event(TurnStreamEvent::TextDelta { text: reply.into() });
        sink.event(TurnStreamEvent::Done {
            stop_reason: StopReason::End,
        });
        id
    }

    async fn run(&self, id: &str, text: String, cancel: CancellationToken, sink: &dyn TurnSink) {
        // Local intents never pay cloud latency or cost (spec §3.3).
        if let Some(intent) = intents::route(&text) {
            let reply = self.local_reply(&intent);
            return self.finish_text(id, &reply, sink);
        }
        if let Err(c) = self.spend.check(self.today()) {
            let what = match c.kind {
                CapKind::Daily => "daily",
                CapKind::Monthly => "monthly",
            };
            return self.fail(
                id,
                err(
                    ErrorCode::SpendCapReached,
                    &format!("The {what} spend cap has been reached."),
                    false,
                    Some(id),
                ),
                StopReason::SpendCap,
                sink,
            );
        }
        let Some(provider) = (self.provider)() else {
            return self.fail(
                id,
                err(
                    ErrorCode::ProviderAuth,
                    "No API key is set.",
                    false,
                    Some(id),
                ),
                StopReason::Error,
                sink,
            );
        };
        let messages = {
            let mut h = lock(&self.history);
            h.push(Message::text(Role::User, text));
            trim_history(&mut h);
            h.clone()
        };
        sink.snapshot(self.orch.set_cloud_in_use(true));
        let cfg = (self.config)();
        tracing::debug!(provider = provider.id(), model = %cfg.model, "starting cloud turn");
        let req = TurnRequest {
            model: cfg.model.clone(),
            system: PERSONA.to_owned(),
            messages,
            tools: vec![],
            max_output_tokens: MAX_OUTPUT_TOKENS,
            effort: Effort::Low,
        };
        let input_chars: usize = req.messages.iter().map(Message::size).sum();
        let mut stream = match provider.stream_turn(req, cancel.clone()).await {
            Ok(s) => s,
            Err(e) => {
                self.drop_dangling_user();
                return self.provider_failed(id, &e, sink);
            }
        };
        let mut reply = String::new();
        let mut reported: Option<Usage> = None;
        let mut stop: Option<ProviderStop> = None;
        while let Some(item) = stream.next().await {
            match item {
                Ok(TurnEvent::TextDelta(t)) => {
                    reply.push_str(&t);
                    sink.event(TurnStreamEvent::TextDelta { text: t });
                }
                Ok(TurnEvent::ToolCall { .. }) => {} // no tools are offered yet
                Ok(TurnEvent::Done {
                    usage: u,
                    stop: why,
                }) => {
                    reported = Some(u);
                    stop = Some(why);
                }
                Err(e) => {
                    self.record(reported, input_chars, reply.len(), cfg.price, sink);
                    self.drop_dangling_user();
                    return self.provider_failed(id, &e, sink);
                }
            }
        }
        self.record(reported, input_chars, reply.len(), cfg.price, sink);
        match stop {
            // The provider declined: say so plainly, and do not keep the request in history.
            Some(ProviderStop::Refusal) => {
                self.drop_dangling_user();
                if reply.trim().is_empty() {
                    return self.finish_text(id, "I'm not able to help with that.", sink);
                }
                return self.finish(id, StopReason::End, sink);
            }
            // All of the budget went on thinking: nothing to show, so report it for a retry.
            Some(ProviderStop::MaxTokens) if reply.trim().is_empty() => {
                self.drop_dangling_user();
                return self.fail(
                    id,
                    err(
                        ErrorCode::ProviderUnavailable,
                        "The model ran out of room before it answered. Try again.",
                        true,
                        Some(id),
                    ),
                    StopReason::Error,
                    sink,
                );
            }
            _ => {}
        }
        lock(&self.history).push(Message::text(Role::Assistant, reply.clone()));
        *lock(&self.last_reply) = Some(reply);
        self.finish(id, StopReason::End, sink);
    }

    /// A failed or cancelled turn has no reply, so its user message is removed again:
    /// history stays alternating, and a Retry that resends the text does not duplicate it.
    fn drop_dangling_user(&self) {
        let mut h = lock(&self.history);
        if h.last().is_some_and(|m| m.role == Role::User) {
            h.pop();
        }
    }

    /// Records spend. A cancelled or failed stream never reports usage, so it is
    /// estimated (about 4 characters per token) rather than counted as zero.
    fn record(
        &self,
        reported: Option<Usage>,
        input_chars: usize,
        output_chars: usize,
        price: Price,
        sink: &dyn TurnSink,
    ) {
        let usage = reported.unwrap_or_else(|| estimate_usage(input_chars, output_chars));
        if usage != Usage::default() {
            self.spend.record(self.today(), usage, price);
            sink.usage(self.spend.summary(self.today()));
        }
    }

    fn local_reply(&self, intent: &Intent) -> String {
        match intent {
            Intent::WhatCanYouDo => "For now I can chat and handle a few quick commands such as stop and mute. Opening apps, files and system status arrive in a later milestone.".into(),
            Intent::RepeatLast => lock(&self.last_reply)
                .clone()
                .unwrap_or_else(|| "I haven't said anything yet.".into()),
            _ => "I can't do that yet; it arrives with the tools milestone.".into(),
        }
    }

    fn finish_text(&self, id: &str, reply: &str, sink: &dyn TurnSink) {
        sink.event(TurnStreamEvent::TextDelta { text: reply.into() });
        *lock(&self.last_reply) = Some(reply.to_owned());
        self.finish(id, StopReason::End, sink);
    }

    fn finish(&self, id: &str, reason: StopReason, sink: &dyn TurnSink) {
        // Only the first terminal event wins: a cancel may already have ended the turn.
        match self.orch.transition(id, AssistantPhase::Idle) {
            Ok(s) => {
                sink.snapshot(s);
                sink.event(TurnStreamEvent::Done {
                    stop_reason: reason,
                });
            }
            Err(_) => sink.event(TurnStreamEvent::Done {
                stop_reason: StopReason::Cancelled,
            }),
        }
    }

    fn provider_failed(&self, id: &str, e: &ProviderError, sink: &dyn TurnSink) {
        if *e == ProviderError::Cancelled {
            // `cancel` already returned the orchestrator to idle.
            return sink.event(TurnStreamEvent::Done {
                stop_reason: StopReason::Cancelled,
            });
        }
        self.fail(id, map_provider_error(e, id), StopReason::Error, sink);
    }

    fn fail(&self, id: &str, error: JarvisError, reason: StopReason, sink: &dyn TurnSink) {
        // Err means the turn was cancelled meanwhile: nothing to publish.
        if let Ok(s) = self.orch.fail(id, error) {
            sink.snapshot(s);
        }
        sink.event(TurnStreamEvent::Done {
            stop_reason: reason,
        });
    }
}

/// Used only when a stream ended without reporting usage (cancelled or failed). Input is
/// about 4 characters per token. Output is charged at the full output limit: hidden
/// thinking tokens are billed but never streamed, so counting only visible text would
/// undercount, and the spend cap should err on the safe side.
fn estimate_usage(input_chars: usize, output_chars: usize) -> Usage {
    let tokens = |c: usize| c.div_ceil(4).min(u32::MAX as usize) as u32;
    Usage {
        input_tokens: tokens(input_chars),
        output_tokens: tokens(output_chars).max(MAX_OUTPUT_TOKENS),
    }
}

/// Drops the oldest messages until the history fits the budget, keeping it starting on a user turn.
fn trim_history(h: &mut Vec<Message>) {
    let size = |h: &[Message]| h.iter().map(Message::size).sum::<usize>();
    while h.len() > 1 && size(h) > HISTORY_CHAR_BUDGET {
        h.remove(0);
    }
    // Start on a plain user message: never on a reply, or on a tool result whose call was cut.
    while h
        .first()
        .is_some_and(|m| m.role == Role::Assistant || !m.blocks.is_empty())
        && h.len() > 1
    {
        h.remove(0);
    }
}

#[cfg(test)]
mod tests;
