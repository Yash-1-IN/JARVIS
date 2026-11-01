//! Tests run against an in-process fake server; no real API key or network is used.

use super::*;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Clone)]
enum Reply {
    Full {
        status: u16,
        body: String,
    },
    /// Send headers (and optional partial body) and then hang.
    Hang {
        partial: String,
    },
}

fn sse(events: &[(&str, &str)]) -> String {
    events
        .iter()
        .map(|(e, d)| format!("event: {e}\ndata: {d}\n\n"))
        .collect()
}

fn text_stream() -> String {
    sse(&[
        (
            "message_start",
            r#"{"message":{"usage":{"input_tokens":11}}}"#,
        ),
        (
            "content_block_start",
            r#"{"index":0,"content_block":{"type":"text","text":""}}"#,
        ),
        ("ping", "{}"),
        (
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"text_delta","text":"Good "}}"#,
        ),
        (
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"text_delta","text":"evening."}}"#,
        ),
        ("content_block_stop", r#"{"index":0}"#),
        (
            "message_delta",
            r#"{"delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":4}}"#,
        ),
        ("message_stop", "{}"),
    ])
}

/// Serves `replies` in order, one per connection. Returns the base URL and captured requests.
async fn serve(replies: Vec<Reply>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    tokio::spawn(async move {
        for reply in replies {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let n = sock.read(&mut tmp).await.unwrap();
                buf.extend_from_slice(&tmp[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                if let Some(h) = text.find("\r\n\r\n") {
                    let len = text[..h]
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length: ")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if buf.len() >= h + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            seen2
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf).to_string());
            match reply {
                Reply::Full { status, body } => {
                    let head = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    sock.write_all(head.as_bytes()).await.unwrap();
                    sock.write_all(body.as_bytes()).await.unwrap();
                    let _ = sock.shutdown().await;
                }
                Reply::Hang { partial } => {
                    sock.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n").await.unwrap();
                    if !partial.is_empty() {
                        let chunk = format!("{:x}\r\n{}\r\n", partial.len(), partial);
                        sock.write_all(chunk.as_bytes()).await.unwrap();
                    }
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
            }
        }
    });
    (url, seen)
}

fn provider(url: String) -> AnthropicProvider {
    let mut cfg = AnthropicConfig::new("sk-ant-FAKEKEY-for-tests".into());
    cfg.base_url = url;
    cfg.backoff_base = Duration::ZERO;
    cfg.first_token_timeout = Duration::from_millis(300);
    AnthropicProvider::new(cfg).unwrap()
}

fn req() -> TurnRequest {
    TurnRequest {
        model: "test-model".into(),
        system: "You are JARVIS.".into(),
        messages: vec![Message {
            role: Role::User,
            content: "hello".into(),
        }],
        tools: vec![],
        max_output_tokens: 256,
        effort: Effort::Low,
    }
}

async fn collect(p: &AnthropicProvider) -> Result<Vec<TurnEvent>, ProviderError> {
    let mut s = p.stream_turn(req(), CancellationToken::new()).await?;
    let mut out = vec![];
    while let Some(item) = s.next().await {
        out.push(item?);
    }
    Ok(out)
}

#[tokio::test]
async fn streams_text_deltas_and_usage() {
    let (url, seen) = serve(vec![Reply::Full {
        status: 200,
        body: text_stream(),
    }])
    .await;
    let evs = collect(&provider(url)).await.unwrap();
    assert_eq!(evs[0], TurnEvent::TextDelta("Good ".into()));
    assert_eq!(evs[1], TurnEvent::TextDelta("evening.".into()));
    assert_eq!(
        evs[2],
        TurnEvent::Done {
            stop: StopReason::EndTurn,
            usage: Usage {
                input_tokens: 11,
                output_tokens: 4
            }
        }
    );
    let sent = seen.lock().unwrap()[0].to_lowercase();
    assert!(sent.contains("x-api-key: sk-ant-fakekey-for-tests"));
    assert!(sent.contains("anthropic-version: 2023-06-01"));
    assert!(sent.contains("\"stream\":true") && sent.contains("\"model\":\"test-model\""));
    assert!(
        sent.contains("\"output_config\":{\"effort\":\"low\"}"),
        "{sent}"
    );
}

#[tokio::test]
async fn assembles_tool_call_from_partial_json() {
    let body = sse(&[
        (
            "message_start",
            r#"{"message":{"usage":{"input_tokens":5}}}"#,
        ),
        (
            "content_block_start",
            r#"{"index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"files.search"}}"#,
        ),
        (
            "content_block_delta",
            r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"{\"pattern\":"}}"#,
        ),
        (
            "content_block_delta",
            r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"\"tax\"}"}}"#,
        ),
        ("content_block_stop", r#"{"index":1}"#),
        (
            "message_delta",
            r#"{"delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":9}}"#,
        ),
        ("message_stop", "{}"),
    ]);
    let (url, _) = serve(vec![Reply::Full { status: 200, body }]).await;
    let evs = collect(&provider(url)).await.unwrap();
    assert_eq!(
        evs[0],
        TurnEvent::ToolCall {
            id: "toolu_1".into(),
            name: "files.search".into(),
            args: json!({"pattern":"tax"})
        }
    );
    assert!(matches!(
        evs[1],
        TurnEvent::Done {
            stop: StopReason::ToolUse,
            ..
        }
    ));
}

#[tokio::test]
async fn bad_key_is_auth_error_without_retry() {
    let (url, seen) = serve(vec![Reply::Full {
        status: 401,
        body: "{}".into(),
    }])
    .await;
    assert_eq!(
        collect(&provider(url)).await.unwrap_err(),
        ProviderError::Auth
    );
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn rate_limit_then_success_retries() {
    let (url, seen) = serve(vec![
        Reply::Full {
            status: 429,
            body: "{}".into(),
        },
        Reply::Full {
            status: 200,
            body: text_stream(),
        },
    ])
    .await;
    assert!(collect(&provider(url)).await.is_ok());
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn persistent_500_gives_up_after_two_retries() {
    let r = Reply::Full {
        status: 500,
        body: "{}".into(),
    };
    let (url, seen) = serve(vec![r.clone(), r.clone(), r.clone(), r]).await;
    assert!(matches!(
        collect(&provider(url)).await.unwrap_err(),
        ProviderError::Unavailable(_)
    ));
    assert_eq!(seen.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn unreachable_server_is_offline() {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    drop(l);
    assert_eq!(
        collect(&provider(url)).await.unwrap_err(),
        ProviderError::Offline
    );
}

#[tokio::test]
async fn no_first_token_times_out() {
    let (url, _) = serve(vec![Reply::Hang {
        partial: String::new(),
    }])
    .await;
    let err = collect(&provider(url)).await.unwrap_err();
    assert!(matches!(err, ProviderError::Unavailable(m) if m.contains("first token")));
}

#[tokio::test]
async fn stall_after_message_start_still_hits_first_token_timeout() {
    // message_start and ping arrive at once, then nothing: must not wait for the 60 s turn limit.
    let partial = sse(&[
        (
            "message_start",
            r#"{"message":{"usage":{"input_tokens":3}}}"#,
        ),
        ("ping", "{}"),
    ]);
    let (url, _) = serve(vec![Reply::Hang { partial }]).await;
    let start = std::time::Instant::now();
    let err = collect(&provider(url)).await.unwrap_err();
    assert!(matches!(err, ProviderError::Unavailable(m) if m.contains("first token")));
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn cancel_stops_stream_within_300ms() {
    let partial = sse(&[(
        "content_block_delta",
        r#"{"index":0,"delta":{"type":"text_delta","text":"Hi"}}"#,
    )]);
    let (url, _) = serve(vec![Reply::Hang { partial }]).await;
    let p = provider(url);
    let cancel = CancellationToken::new();
    let mut s = p.stream_turn(req(), cancel.clone()).await.unwrap();
    assert_eq!(
        s.next().await.unwrap().unwrap(),
        TurnEvent::TextDelta("Hi".into())
    );
    let start = std::time::Instant::now();
    cancel.cancel();
    assert_eq!(
        s.next().await.unwrap().unwrap_err(),
        ProviderError::Cancelled
    );
    assert!(start.elapsed() < Duration::from_millis(300));
}

#[tokio::test]
async fn mid_stream_error_event_is_surfaced() {
    let body = sse(&[
        (
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"text_delta","text":"a"}}"#,
        ),
        (
            "error",
            r#"{"error":{"type":"overloaded_error","message":"x"}}"#,
        ),
    ]);
    let (url, _) = serve(vec![Reply::Full { status: 200, body }]).await;
    assert!(matches!(
        collect(&provider(url)).await.unwrap_err(),
        ProviderError::Unavailable(_)
    ));
}

#[test]
fn config_debug_hides_api_key() {
    let c = AnthropicConfig::new("sk-ant-SECRET123456".into());
    assert!(!format!("{c:?}").contains("SECRET"));
}

#[tokio::test]
async fn key_check_accepts_a_good_key_and_sends_no_body_tokens() {
    let (url, seen) = serve(vec![Reply::Full {
        status: 200,
        body: "{}".into(),
    }])
    .await;
    assert_eq!(
        check_key(&url, "sk-ant-FAKEKEY-for-tests", Duration::from_secs(2)).await,
        Ok(())
    );
    let sent = seen.lock().unwrap()[0].to_lowercase();
    assert!(sent.starts_with("get /v1/models"), "{sent}");
    assert!(sent.contains("x-api-key: sk-ant-fakekey-for-tests"));
}

#[tokio::test]
async fn key_check_maps_rejection_offline_and_timeout() {
    let (url, _) = serve(vec![Reply::Full {
        status: 401,
        body: "{}".into(),
    }])
    .await;
    assert_eq!(
        check_key(&url, "bad", Duration::from_secs(2)).await,
        Err(ProviderError::Auth)
    );
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead = format!("http://{}", l.local_addr().unwrap());
    drop(l);
    // Generous timeout: Windows takes about 2 s to report a refused connection, and a
    // short limit would turn this into "timed out" instead of "offline".
    assert_eq!(
        check_key(&dead, "k", Duration::from_secs(15)).await,
        Err(ProviderError::Offline)
    );
    // A server that accepts the connection and never answers.
    let silent = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", silent.local_addr().unwrap());
    tokio::spawn(async move {
        let _held = silent.accept().await;
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let r = check_key(&url, "k", Duration::from_millis(200)).await;
    assert!(matches!(r, Err(ProviderError::Unavailable(_))), "{r:?}");
}

#[tokio::test]
async fn key_with_invalid_header_characters_is_rejected_without_a_request() {
    assert_eq!(
        check_key("http://127.0.0.1:1", "bad\nkey", Duration::from_secs(1)).await,
        Err(ProviderError::Auth)
    );
}

#[tokio::test]
async fn refusal_and_max_tokens_stop_reasons_are_mapped() {
    for (reason, expected) in [
        ("refusal", StopReason::Refusal),
        ("max_tokens", StopReason::MaxTokens),
    ] {
        let body = sse(&[
            (
                "message_start",
                r#"{"message":{"usage":{"input_tokens":1}}}"#,
            ),
            (
                "message_delta",
                &format!(
                    r#"{{"delta":{{"stop_reason":"{reason}"}},"usage":{{"output_tokens":7}}}}"#
                ),
            ),
            ("message_stop", "{}"),
        ]);
        let (url, _) = serve(vec![Reply::Full { status: 200, body }]).await;
        let evs = collect(&provider(url)).await.unwrap();
        assert!(
            matches!(evs.last(), Some(TurnEvent::Done { stop, .. }) if *stop == expected),
            "{reason}: {evs:?}"
        );
    }
}
