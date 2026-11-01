use super::*;
use crate::intelligence::provider::StopReason as ProviderStop;
use crate::intelligence::spend::Caps;
use async_trait::async_trait;
use futures::stream::BoxStream;
use std::time::Duration;

#[derive(Default)]
struct Rec {
    events: Mutex<Vec<TurnStreamEvent>>,
    snaps: Mutex<Vec<Snapshot>>,
    usages: Mutex<Vec<UsageSummary>>,
    interrupts: Mutex<Vec<TurnInterrupted>>,
}
impl TurnSink for Rec {
    fn event(&self, e: TurnStreamEvent) {
        lock(&self.events).push(e);
    }
    fn snapshot(&self, s: Snapshot) {
        lock(&self.snaps).push(s);
    }
    fn usage(&self, u: UsageSummary) {
        lock(&self.usages).push(u);
    }
    fn interrupted(&self, i: TurnInterrupted) {
        lock(&self.interrupts).push(i);
    }
}
impl Rec {
    fn text(&self) -> String {
        lock(&self.events)
            .iter()
            .filter_map(|e| match e {
                TurnStreamEvent::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
    fn dones(&self) -> Vec<StopReason> {
        lock(&self.events)
            .iter()
            .filter_map(|e| match e {
                TurnStreamEvent::Done { stop_reason } => Some(*stop_reason),
                _ => None,
            })
            .collect()
    }
    async fn wait_done(&self) {
        for _ in 0..200 {
            if !self.dones().is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("turn never finished");
    }
}

enum Script {
    Ok(Vec<&'static str>, Usage),
    Stop(Vec<&'static str>, ProviderStop),
    FailAfterText(ProviderError),
    FailToStart(ProviderError),
    HangAfterText,
}

struct Fake {
    script: Script,
    calls: Mutex<u32>,
    seen: Mutex<Vec<TurnRequest>>,
}
impl Fake {
    fn new(script: Script) -> Arc<Self> {
        Arc::new(Self {
            script,
            calls: Mutex::new(0),
            seen: Mutex::new(vec![]),
        })
    }
}
#[async_trait]
impl ModelProvider for Fake {
    fn id(&self) -> &'static str {
        "fake"
    }
    async fn stream_turn(
        &self,
        req: TurnRequest,
        cancel: CancellationToken,
    ) -> Result<BoxStream<'static, Result<TurnEvent, ProviderError>>, ProviderError> {
        *lock(&self.calls) += 1;
        lock(&self.seen).push(req);
        let text = |t: &str| Ok(TurnEvent::TextDelta(t.to_owned()));
        match &self.script {
            Script::FailToStart(e) => Err(e.clone()),
            Script::Stop(parts, why) => {
                let mut items: Vec<_> = parts.iter().map(|p| text(p)).collect();
                items.push(Ok(TurnEvent::Done {
                    stop: *why,
                    usage: Usage {
                        input_tokens: 10,
                        output_tokens: 20,
                    },
                }));
                Ok(futures::stream::iter(items).boxed())
            }
            Script::Ok(parts, usage) => {
                let mut items: Vec<_> = parts.iter().map(|p| text(p)).collect();
                items.push(Ok(TurnEvent::Done {
                    stop: ProviderStop::EndTurn,
                    usage: *usage,
                }));
                Ok(futures::stream::iter(items).boxed())
            }
            Script::FailAfterText(e) => {
                Ok(futures::stream::iter(vec![text("Par"), Err(e.clone())]).boxed())
            }
            Script::HangAfterText => Ok(futures::stream::iter(vec![text("Hi")])
                .chain(futures::stream::once(async move {
                    cancel.cancelled().await;
                    Err(ProviderError::Cancelled)
                }))
                .boxed()),
        }
    }
}

const PRICE: Price = Price {
    input_per_mtok: 1_000_000,
    output_per_mtok: 5_000_000,
};
const NOW: u64 = 1_793_491_200; // 2026-11-01 UTC

fn runner(
    provider: Option<Arc<Fake>>,
    caps: Caps,
) -> (Arc<TurnRunner>, Arc<Orchestrator>, Arc<SpendMeter>) {
    let orch = Arc::new(Orchestrator::default());
    let spend = Arc::new(SpendMeter::new(caps, Day::from_unix_secs(NOW)));
    let r = Arc::new(TurnRunner::new(
        orch.clone(),
        spend.clone(),
        Box::new(move || provider.clone().map(|p| p as Arc<dyn ModelProvider>)),
        Box::new(|| TurnConfig {
            model: "test-model".into(),
            price: PRICE,
        }),
        Box::new(|| NOW),
    ));
    (r, orch, spend)
}

#[tokio::test]
async fn cloud_turn_streams_records_spend_and_returns_to_idle() {
    let fake = Fake::new(Script::Ok(
        vec!["Good ", "evening."],
        Usage {
            input_tokens: 1000,
            output_tokens: 100,
        },
    ));
    let (r, orch, spend) = runner(Some(fake.clone()), Caps::default());
    let sink = Arc::new(Rec::default());
    let id = r.start("Tell me something".into(), sink.clone()).unwrap();
    sink.wait_done().await;
    assert_eq!(sink.text(), "Good evening.");
    assert_eq!(sink.dones(), vec![StopReason::End]);
    let s = orch.snapshot();
    assert_eq!(
        (s.phase, s.active_request_id, s.cloud_in_use),
        (AssistantPhase::Idle, None, false)
    );
    assert!(
        lock(&sink.snaps)
            .iter()
            .any(|s| s.cloud_in_use && s.active_request_id.as_deref() == Some(&id))
    );
    assert_eq!(spend.totals(Day::from_unix_secs(NOW)).today, 1_000 + 500);
    // The usage.updated payload follows the recorded spend.
    let u = lock(&sink.usages).last().cloned().expect("a usage update");
    assert_eq!(
        (
            u.today_micro_usd,
            u.today_input_tokens,
            u.today_output_tokens
        ),
        (1_500, 1000, 100)
    );
    let seen = lock(&fake.seen);
    assert_eq!(seen[0].model, "test-model");
    assert!(seen[0].system.contains("JARVIS"));
    assert!(seen[0].tools.is_empty());
}

#[tokio::test]
async fn history_is_sent_on_the_next_turn() {
    let fake = Fake::new(Script::Ok(vec!["Noted."], Usage::default()));
    let (r, _, _) = runner(Some(fake.clone()), Caps::default());
    for q in ["first question", "second question"] {
        let sink = Arc::new(Rec::default());
        r.start(q.into(), sink.clone()).unwrap();
        sink.wait_done().await;
    }
    let seen = lock(&fake.seen);
    let second: Vec<_> = seen[1]
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect();
    assert_eq!(second, vec!["first question", "Noted.", "second question"]);
}

#[tokio::test]
async fn request_uses_low_effort_and_a_generous_token_limit() {
    let fake = Fake::new(Script::Ok(vec!["Hi."], Usage::default()));
    let (r, _, _) = runner(Some(fake.clone()), Caps::default());
    let sink = Arc::new(Rec::default());
    r.start("hello there".into(), sink.clone()).unwrap();
    sink.wait_done().await;
    let seen = lock(&fake.seen);
    assert_eq!(seen[0].effort, Effort::Low);
    assert!(seen[0].max_output_tokens >= 4096);
}

#[tokio::test]
async fn refusal_gets_a_plain_message_and_is_not_kept_in_history() {
    let fake = Fake::new(Script::Stop(vec![], ProviderStop::Refusal));
    let (r, orch, _) = runner(Some(fake), Caps::default());
    let sink = Arc::new(Rec::default());
    r.start("something not allowed".into(), sink.clone())
        .unwrap();
    sink.wait_done().await;
    assert_eq!(sink.text(), "I'm not able to help with that.");
    assert_eq!(sink.dones(), vec![StopReason::End]);
    assert_eq!(orch.snapshot().phase, AssistantPhase::Idle);
    assert!(lock(&r.history).is_empty());
}

#[tokio::test]
async fn running_out_of_tokens_with_no_text_is_a_retryable_error() {
    let fake = Fake::new(Script::Stop(vec![], ProviderStop::MaxTokens));
    let (r, orch, _) = runner(Some(fake), Caps::default());
    let sink = Arc::new(Rec::default());
    r.start("think hard".into(), sink.clone()).unwrap();
    sink.wait_done().await;
    assert_eq!(sink.dones(), vec![StopReason::Error]);
    let e = orch.snapshot().last_error.unwrap();
    assert_eq!(
        (e.code, e.retryable),
        (ErrorCode::ProviderUnavailable, true)
    );
    assert!(lock(&r.history).is_empty());
}

#[tokio::test]
async fn max_tokens_with_partial_text_still_finishes_normally() {
    let fake = Fake::new(Script::Stop(vec!["Half an answ"], ProviderStop::MaxTokens));
    let (r, _, _) = runner(Some(fake), Caps::default());
    let sink = Arc::new(Rec::default());
    r.start("long question".into(), sink.clone()).unwrap();
    sink.wait_done().await;
    assert_eq!(
        (sink.text().as_str(), sink.dones()),
        ("Half an answ", vec![StopReason::End])
    );
}

#[tokio::test]
async fn failed_turn_leaves_no_dangling_user_message() {
    // First turn fails; the retry of the same text must be sent once, not twice.
    let fail = Fake::new(Script::FailAfterText(ProviderError::Offline));
    let (r, _, _) = runner(Some(fail.clone()), Caps::default());
    let a = Arc::new(Rec::default());
    r.start("same question".into(), a.clone()).unwrap();
    a.wait_done().await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        lock(&r.history).is_empty(),
        "history: {:?}",
        lock(&r.history)
    );
    let b = Arc::new(Rec::default());
    r.start("same question".into(), b.clone()).unwrap();
    b.wait_done().await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    let seen = lock(&fail.seen);
    assert_eq!(seen[1].messages.len(), 1);
    assert_eq!(seen[1].messages[0].content, "same question");
}

#[tokio::test]
async fn cancelled_turn_leaves_no_dangling_user_message() {
    let (r, _, _) = runner(Some(Fake::new(Script::HangAfterText)), Caps::default());
    let a = Arc::new(Rec::default());
    r.start("long answer please".into(), a.clone()).unwrap();
    for _ in 0..100 {
        if a.text() == "Hi" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    r.start("stop".into(), Arc::new(Rec::default())).unwrap();
    a.wait_done().await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(lock(&r.history).is_empty());
}

#[tokio::test]
async fn local_intent_never_calls_the_provider() {
    let fake = Fake::new(Script::Ok(vec!["cloud"], Usage::default()));
    let (r, _, _) = runner(Some(fake.clone()), Caps::default());
    let sink = Arc::new(Rec::default());
    r.start("what can you do?".into(), sink.clone()).unwrap();
    sink.wait_done().await;
    assert_eq!(*lock(&fake.calls), 0);
    assert!(sink.text().contains("quick commands"));
    assert_eq!(sink.dones(), vec![StopReason::End]);
}

#[tokio::test]
async fn repeat_last_says_the_previous_reply() {
    let fake = Fake::new(Script::Ok(vec!["Splendid."], Usage::default()));
    let (r, _, _) = runner(Some(fake), Caps::default());
    let a = Arc::new(Rec::default());
    r.start("hello there".into(), a.clone()).unwrap();
    a.wait_done().await;
    let b = Arc::new(Rec::default());
    r.start("say that again".into(), b.clone()).unwrap();
    b.wait_done().await;
    assert_eq!(b.text(), "Splendid.");
}

#[tokio::test]
async fn mute_and_unmute_work_without_starting_a_turn() {
    let (r, orch, _) = runner(None, Caps::default());
    let sink = Arc::new(Rec::default());
    r.start("mute".into(), sink.clone()).unwrap();
    assert_eq!(orch.snapshot().mic, crate::ipc::types::MicState::Muted);
    assert_eq!(orch.snapshot().phase, AssistantPhase::Idle);
    r.start("unmute".into(), sink.clone()).unwrap();
    assert_eq!(orch.snapshot().mic, crate::ipc::types::MicState::Off);
    assert_eq!(sink.dones(), vec![StopReason::End, StopReason::End]);
}

#[tokio::test]
async fn stop_cancels_the_running_turn() {
    let (r, orch, _) = runner(Some(Fake::new(Script::HangAfterText)), Caps::default());
    let a = Arc::new(Rec::default());
    r.start("long answer please".into(), a.clone()).unwrap();
    for _ in 0..100 {
        if a.text() == "Hi" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let b = Arc::new(Rec::default());
    r.start("stop".into(), b.clone()).unwrap();
    a.wait_done().await;
    assert_eq!(b.text(), "Stopped.");
    // The stop command cut the turn short: one turn.interrupted with its request id.
    let i = lock(&b.interrupts).clone();
    assert_eq!(i.len(), 1);
    assert_eq!(i[0].reason, InterruptReason::UserCancel);
    assert_eq!(
        Some(i[0].request_id.clone()),
        lock(&a.snaps)
            .iter()
            .find_map(|s| s.active_request_id.clone())
    );
    assert_eq!(a.dones(), vec![StopReason::Cancelled]);
    assert_eq!(orch.snapshot().phase, AssistantPhase::Idle);
    assert_eq!(orch.snapshot().active_request_id, None);
}

#[tokio::test]
async fn cancelled_turn_still_counts_estimated_spend() {
    let (r, _, spend) = runner(Some(Fake::new(Script::HangAfterText)), Caps::default());
    let a = Arc::new(Rec::default());
    r.start("long answer please".into(), a.clone()).unwrap();
    for _ in 0..100 {
        if a.text() == "Hi" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    r.start("stop".into(), Arc::new(Rec::default())).unwrap();
    a.wait_done().await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    // 18 input chars -> 5 tokens; output is charged at the full 4096-token limit
    // (hidden thinking is billed but never streamed): 5 + 20,480 micro-dollars.
    assert_eq!(spend.totals(Day::from_unix_secs(NOW)).today, 5 + 20_480);
}

#[test]
fn usage_estimate_rounds_input_up_and_floors_output() {
    assert_eq!(estimate_usage(0, 0).input_tokens, 0);
    assert_eq!(estimate_usage(1, 4).input_tokens, 1);
    assert_eq!(estimate_usage(5, 9).input_tokens, 2);
    // Output never drops below the limit, so thinking models are not undercounted.
    assert_eq!(estimate_usage(0, 4).output_tokens, MAX_OUTPUT_TOKENS);
    assert_eq!(
        estimate_usage(0, 4 * (MAX_OUTPUT_TOKENS as usize + 10)).output_tokens,
        MAX_OUTPUT_TOKENS + 10
    );
}

#[tokio::test]
async fn cancel_is_idempotent() {
    let (r, _, _) = runner(None, Caps::default());
    let sink = Rec::default();
    assert!(!r.cancel("no-such-request", InterruptReason::UserCancel, &sink));
    assert!(
        lock(&sink.interrupts).is_empty(),
        "nothing was running, so nothing was interrupted"
    );
}

#[tokio::test]
async fn spend_cap_blocks_the_cloud_call() {
    let fake = Fake::new(Script::Ok(vec!["x"], Usage::default()));
    let (r, orch, spend) = runner(
        Some(fake.clone()),
        Caps {
            daily: Some(100),
            monthly: None,
        },
    );
    spend.record(
        Day::from_unix_secs(NOW),
        Usage {
            input_tokens: 100,
            output_tokens: 0,
        },
        PRICE,
    );
    let sink = Arc::new(Rec::default());
    r.start("anything".into(), sink.clone()).unwrap();
    sink.wait_done().await;
    assert_eq!(*lock(&fake.calls), 0);
    assert_eq!(sink.dones(), vec![StopReason::SpendCap]);
    let s = orch.snapshot();
    assert_eq!(s.last_error.unwrap().code, ErrorCode::SpendCapReached);
    // Local intents still work at the cap, and a new turn can start.
    let again = Arc::new(Rec::default());
    r.start("what can you do".into(), again.clone()).unwrap();
    again.wait_done().await;
    assert_eq!(again.dones(), vec![StopReason::End]);
}

#[tokio::test]
async fn no_provider_means_provider_auth_error() {
    let (r, orch, _) = runner(None, Caps::default());
    let sink = Arc::new(Rec::default());
    r.start("hello".into(), sink.clone()).unwrap();
    sink.wait_done().await;
    assert_eq!(sink.dones(), vec![StopReason::Error]);
    assert_eq!(
        orch.snapshot().last_error.unwrap().code,
        ErrorCode::ProviderAuth
    );
}

#[tokio::test]
async fn provider_errors_map_to_codes_and_retryability() {
    let cases = [
        (ProviderError::Auth, ErrorCode::ProviderAuth, false),
        (ProviderError::RateLimit, ErrorCode::ProviderRateLimit, true),
        (
            ProviderError::Unavailable("x".into()),
            ErrorCode::ProviderUnavailable,
            true,
        ),
        (ProviderError::Offline, ErrorCode::NetworkOffline, true),
    ];
    for (pe, code, retry) in cases {
        let (r, orch, _) = runner(Some(Fake::new(Script::FailToStart(pe))), Caps::default());
        let sink = Arc::new(Rec::default());
        let id = r.start("hello".into(), sink.clone()).unwrap();
        sink.wait_done().await;
        let e = orch.snapshot().last_error.unwrap();
        assert_eq!((e.code, e.retryable, e.request_id), (code, retry, Some(id)));
        assert!(!orch.snapshot().cloud_in_use);
        // Error does not block the next turn.
        let again = Arc::new(Rec::default());
        r.start("mute".into(), again.clone()).unwrap();
        assert!(r.orch.begin_turn().is_ok());
    }
}

#[tokio::test]
async fn failure_mid_stream_ends_with_one_error_done_and_keeps_partial_text() {
    let (r, orch, _) = runner(
        Some(Fake::new(Script::FailAfterText(
            ProviderError::Unavailable("drop".into()),
        ))),
        Caps::default(),
    );
    let sink = Arc::new(Rec::default());
    r.start("hello".into(), sink.clone()).unwrap();
    sink.wait_done().await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(sink.text(), "Par");
    assert_eq!(sink.dones(), vec![StopReason::Error]);
    assert_eq!(orch.snapshot().phase, AssistantPhase::Error);
}

#[tokio::test]
async fn second_request_while_busy_is_rejected() {
    let (r, _, _) = runner(Some(Fake::new(Script::HangAfterText)), Caps::default());
    let a = Arc::new(Rec::default());
    r.start("first".into(), a.clone()).unwrap();
    let e = r
        .start("second".into(), Arc::new(Rec::default()))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Internal);
    assert!(e.message.contains("busy"));
}

#[test]
fn history_trimming_keeps_budget_and_starts_on_a_user_turn() {
    let big = "x".repeat(HISTORY_CHAR_BUDGET / 2 + 1);
    let mut h = vec![
        Message {
            role: Role::User,
            content: big.clone(),
        },
        Message {
            role: Role::Assistant,
            content: big.clone(),
        },
        Message {
            role: Role::User,
            content: "latest".into(),
        },
    ];
    trim_history(&mut h);
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].content, "latest");
}

#[tokio::test]
async fn cancel_command_reports_the_reason_once_and_a_second_cancel_is_silent() {
    let (r, orch, _) = runner(Some(Fake::new(Script::HangAfterText)), Caps::default());
    let a = Arc::new(Rec::default());
    let id = r.start("long answer please".into(), a.clone()).unwrap();
    for _ in 0..100 {
        if a.text() == "Hi" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let s = Rec::default();
    assert!(r.cancel(&id, InterruptReason::Hotkey, &s));
    // A second cancel for the same turn must not interrupt again.
    let _ = r.cancel(&id, InterruptReason::Hotkey, &s);
    a.wait_done().await;
    let i = lock(&s.interrupts).clone();
    assert_eq!(i.len(), 1, "only the first cancel interrupts");
    assert_eq!(
        (i[0].request_id.as_str(), i[0].reason),
        (id.as_str(), InterruptReason::Hotkey)
    );
    assert_eq!(orch.snapshot().phase, AssistantPhase::Idle);
}
