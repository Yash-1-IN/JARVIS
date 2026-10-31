//! Structured JSON logs with daily rotation and a redaction layer.
//! Nothing here sends data off the machine (ADR-017).

use regex::Regex;
use std::io::{self, Write};
use std::path::Path;
use std::sync::OnceLock;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

const MASK: &str = "***REDACTED***";

fn patterns() -> &'static [Regex; 2] {
    static P: OnceLock<[Regex; 2]> = OnceLock::new();
    P.get_or_init(|| {
        [
            // Provider-style keys, e.g. sk-ant-api03-...
            Regex::new(r"sk-[A-Za-z0-9_\-]{8,}").expect("valid regex"),
            // key=value / "key":"value" for common secret names
            Regex::new(
                r#"(?i)((?:api[_-]?key|token|secret|password|authorization)["']?\s*[:=]\s*["']?)(?:Bearer\s+)?[^\s"',}]+"#,
            )
            .expect("valid regex"),
        ]
    })
}

/// Masks secrets in a log line.
pub fn redact(input: &str) -> String {
    let p = patterns();
    let step = p[0].replace_all(input, MASK);
    p[1].replace_all(&step, |c: &regex::Captures| format!("{}{}", &c[1], MASK))
        .into_owned()
}

/// Wraps a writer so every buffer is redacted before it reaches the sink.
/// The fmt layer writes one complete event per call, so a secret is never split.
pub struct RedactingWriter<W: Write>(W);

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        self.0.write_all(redact(&text).as_bytes())?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[derive(Clone)]
pub struct RedactingMakeWriter<M>(M);

impl<'a, M> MakeWriter<'a> for RedactingMakeWriter<M>
where
    M: MakeWriter<'a>,
{
    type Writer = RedactingWriter<M::Writer>;
    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter(self.0.make_writer())
    }
}

/// Starts logging to `dir` (daily rotation, 14 files kept). Keep the guard alive.
pub fn init(dir: &Path) -> Result<WorkerGuard, Box<dyn std::error::Error>> {
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("jarvis")
        .filename_suffix("log")
        .max_log_files(14)
        .build(dir)?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(RedactingMakeWriter(writer))
        .try_init()
        .map_err(|e| e.to_string())?;
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Buf(Arc<Mutex<Vec<u8>>>);
    impl Write for Buf {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl<'a> MakeWriter<'a> for Buf {
        type Writer = Buf;
        fn make_writer(&'a self) -> Buf {
            self.clone()
        }
    }

    #[test]
    fn masks_fake_api_key_in_emitted_log() {
        let buf = Buf::default();
        let sub = tracing_subscriber::fmt()
            .json()
            .with_writer(RedactingMakeWriter(buf.clone()))
            .finish();
        tracing::subscriber::with_default(sub, || {
            tracing::info!("calling provider with sk-ant-api03-FAKEKEY1234567890abcdef");
            tracing::info!(api_key = "hunter2hunter2", "config loaded");
        });
        let out = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(!out.contains("FAKEKEY1234567890"), "{out}");
        assert!(!out.contains("hunter2"), "{out}");
        assert!(out.contains(MASK));
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        assert_eq!(redact("wake word detected"), "wake word detected");
    }
}
