//! The capture engine: owns the microphone stream, drains the ring, resamples to 16 kHz, keeps the
//! pre-roll, and regains the device when it goes away (spec §6.7: retry every 2 s, recover without a
//! restart). The device is behind [`Backend`], so every rule here is tested with a fake.

use super::dsp::{PreRoll, Resampler, STT_RATE};
use rtrb::Consumer;
use std::any::Any;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// How long to wait before opening the microphone again after it was lost or could not be opened.
pub const RETRY_EVERY: Duration = Duration::from_secs(2);
/// How often the voice thread looks at the ring: 20 ms of audio at a time.
const POLL: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioError {
    #[error("no microphone was found")]
    NoDevice,
    /// The operating system refused access (macOS and Windows privacy settings).
    #[error("the operating system did not allow microphone access")]
    PermissionDenied,
    /// The device was there and went away (unplugged, switched, driver reset).
    #[error("the microphone was lost")]
    DeviceLost,
    #[error("the microphone could not be opened: {0}")]
    Failed(String),
}

/// A microphone stream that is running. Dropping it closes the stream, on the thread that opened it.
pub struct Opened {
    /// Keeps the stream alive. May be `!Send` (cpal's is not on every OS), so it is created, held
    /// and dropped on the capture thread only.
    pub guard: Box<dyn Any>,
    /// The device's own sample rate.
    pub rate: u32,
    pub device_name: String,
    /// Mono samples at the device rate, written by the audio callback.
    pub ring: Consumer<f32>,
    /// Set by the stream's error callback when the device goes away.
    pub lost: Arc<AtomicBool>,
    /// Samples the callback had to drop because the ring was full (the voice thread fell behind).
    pub overruns: Arc<AtomicU64>,
}

/// Opens the microphone. `device` is a name from the device list, or `None` for the system default.
pub trait Backend: Send + 'static {
    fn open(&mut self, device: Option<&str>) -> Result<Opened, AudioError>;
}

impl Backend for Box<dyn Backend> {
    fn open(&mut self, device: Option<&str>) -> Result<Opened, AudioError> {
        (**self).open(device)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureStatus {
    Starting,
    Running {
        device: String,
        rate: u32,
    },
    /// Not capturing; the engine is retrying. The orchestrator turns this into `mic: unavailable`
    /// and, by the error, `MIC_PERMISSION_DENIED` or `DEVICE_LOST`.
    Unavailable(AudioError),
    Stopped,
}

/// A running capture. Dropping it, or calling [`Capture::stop`], cancels it without waiting.
pub struct Capture {
    status: Arc<Mutex<CaptureStatus>>,
    pre_roll: Arc<Mutex<PreRoll>>,
    cancel: CancellationToken,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    /// Starts the capture thread. `on_chunk` gets 16 kHz mono audio about every 20 ms, on that
    /// thread; `on_status` is called on each change. Neither may block for long.
    pub fn start(
        mut backend: impl Backend,
        device: Option<String>,
        retry: Duration,
        mut on_chunk: impl FnMut(&[f32]) + Send + 'static,
        mut on_status: impl FnMut(&CaptureStatus) + Send + 'static,
    ) -> Self {
        let status = Arc::new(Mutex::new(CaptureStatus::Starting));
        let pre_roll = Arc::new(Mutex::new(PreRoll::new()));
        let cancel = CancellationToken::new();
        let (st, pr, token) = (status.clone(), pre_roll.clone(), cancel.clone());
        let thread = std::thread::Builder::new()
            .name("jarvis-capture".into())
            .spawn(move || {
                let mut set = |s: CaptureStatus| {
                    on_status(&s);
                    if let Ok(mut g) = st.lock() {
                        *g = s;
                    }
                };
                // An audio failure must never take the app down (Cargo.toml: panic = "unwind").
                let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(
                        &mut backend,
                        device.as_deref(),
                        retry,
                        &token,
                        &pr,
                        &mut on_chunk,
                        &mut set,
                    );
                }));
                if run.is_err() {
                    tracing::error!("the capture thread panicked; capture stopped");
                }
                set(CaptureStatus::Stopped);
            })
            .ok();
        if thread.is_none() {
            tracing::error!("could not start the capture thread");
        }
        Self {
            status,
            pre_roll,
            cancel,
            thread,
        }
    }

    pub fn status(&self) -> CaptureStatus {
        self.status
            .lock()
            .map(|g| g.clone())
            .unwrap_or(CaptureStatus::Stopped)
    }

    /// The last two seconds of 16 kHz audio, oldest first. A copy; nothing is written anywhere.
    pub fn pre_roll(&self) -> Vec<f32> {
        self.pre_roll
            .lock()
            .map(|g| g.snapshot())
            .unwrap_or_default()
    }

    /// Asks the thread to stop. Returns at once: a device that is stuck opening (for example while
    /// macOS shows its permission question) cannot be interrupted, and must not hold up the app.
    pub fn stop(&self) {
        self.cancel.cancel();
    }

    /// Waits for the thread to end, up to `timeout`. Returns whether it ended.
    pub fn join(&mut self, timeout: Duration) -> bool {
        let Some(handle) = self.thread.take() else {
            return true;
        };
        let end = Instant::now() + timeout;
        while !handle.is_finished() {
            if Instant::now() >= end {
                self.thread = Some(handle);
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        handle.join().is_ok()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Sleeps in short slices so a cancel is noticed quickly. Returns `true` if cancelled.
fn sleep_unless_cancelled(token: &CancellationToken, total: Duration) -> bool {
    let end = Instant::now() + total;
    while Instant::now() < end {
        if token.is_cancelled() {
            return true;
        }
        std::thread::sleep(POLL.min(end.saturating_duration_since(Instant::now())));
    }
    token.is_cancelled()
}

fn run(
    backend: &mut impl Backend,
    device: Option<&str>,
    retry: Duration,
    token: &CancellationToken,
    pre_roll: &Mutex<PreRoll>,
    on_chunk: &mut impl FnMut(&[f32]),
    set: &mut impl FnMut(CaptureStatus),
) {
    let mut scratch: Vec<f32> = Vec::new();
    let mut out: Vec<f32> = Vec::new();
    while !token.is_cancelled() {
        set(CaptureStatus::Starting);
        let mut open = match backend.open(device) {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!(error = %e, "microphone not available, will retry");
                set(CaptureStatus::Unavailable(e));
                if sleep_unless_cancelled(token, retry) {
                    return;
                }
                continue;
            }
        };
        tracing::info!(device = %open.device_name, rate = open.rate, "microphone opened");
        set(CaptureStatus::Running {
            device: open.device_name.clone(),
            rate: open.rate,
        });
        // A new stream is a break in the audio: never splice it onto the old pre-roll.
        if let Ok(mut p) = pre_roll.lock() {
            p.clear();
        }
        let mut resampler = Resampler::new(open.rate, STT_RATE);
        let mut reported_overruns = 0u64;
        loop {
            // Read the flag before draining, so what the device wrote before it vanished is kept.
            let lost = open.lost.load(Ordering::Acquire);
            drain(&mut open.ring, &mut scratch);
            if !scratch.is_empty() {
                out.clear();
                resampler.process(&scratch, &mut out);
                if !out.is_empty() {
                    if let Ok(mut p) = pre_roll.lock() {
                        p.push(&out);
                    }
                    on_chunk(&out);
                }
            }
            let overruns = open.overruns.load(Ordering::Relaxed);
            if overruns != reported_overruns {
                tracing::warn!(dropped = overruns - reported_overruns, "audio ring overran");
                reported_overruns = overruns;
            }
            if token.is_cancelled() {
                return; // `open` drops here and closes the stream
            }
            if lost {
                break;
            }
            std::thread::sleep(POLL);
        }
        drop(open); // close the dead stream before opening a new one
        tracing::warn!("microphone lost, will retry");
        set(CaptureStatus::Unavailable(AudioError::DeviceLost));
        if sleep_unless_cancelled(token, retry) {
            return;
        }
    }
}

/// Moves everything currently in the ring into `scratch` (replacing its contents).
fn drain(ring: &mut Consumer<f32>, scratch: &mut Vec<f32>) {
    scratch.clear();
    let n = ring.slots();
    if n == 0 {
        return;
    }
    if let Ok(chunk) = ring.read_chunk(n) {
        let (a, b) = chunk.as_slices();
        scratch.extend_from_slice(a);
        scratch.extend_from_slice(b);
        chunk.commit_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::{Producer, RingBuffer};
    use std::collections::VecDeque;

    /// One fake open stream: the test writes into `producer` the way the audio callback would.
    struct FakeStream {
        producer: Producer<f32>,
        lost: Arc<AtomicBool>,
        closed: Arc<AtomicBool>,
    }

    struct CloseFlag(Arc<AtomicBool>);
    impl Drop for CloseFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[derive(Clone, Default)]
    struct Fake {
        /// Errors to return from the next opens, front first; empty means open succeeds.
        failures: Arc<Mutex<VecDeque<AudioError>>>,
        streams: Arc<Mutex<Vec<FakeStream>>>,
        asked: Arc<Mutex<Vec<Option<String>>>>,
    }

    impl Backend for Fake {
        fn open(&mut self, device: Option<&str>) -> Result<Opened, AudioError> {
            self.asked.lock().unwrap().push(device.map(String::from));
            if let Some(e) = self.failures.lock().unwrap().pop_front() {
                return Err(e);
            }
            let (producer, ring) = RingBuffer::new(96_000);
            let lost = Arc::new(AtomicBool::new(false));
            let closed = Arc::new(AtomicBool::new(false));
            self.streams.lock().unwrap().push(FakeStream {
                producer,
                lost: lost.clone(),
                closed: closed.clone(),
            });
            Ok(Opened {
                guard: Box::new(CloseFlag(closed)),
                rate: 48_000,
                device_name: "Fake mic".into(),
                ring,
                lost,
                overruns: Arc::new(AtomicU64::new(0)),
            })
        }
    }

    fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while !ok() {
            assert!(Instant::now() < end, "timed out waiting for: {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    type Log = Arc<Mutex<Vec<CaptureStatus>>>;

    fn start(fake: &Fake, device: Option<&str>) -> (Capture, Arc<Mutex<Vec<f32>>>, Log) {
        let got = Arc::new(Mutex::new(Vec::new()));
        let log: Log = Arc::default();
        let (g, l) = (got.clone(), log.clone());
        let cap = Capture::start(
            fake.clone(),
            device.map(String::from),
            Duration::from_millis(40),
            move |c| g.lock().unwrap().extend_from_slice(c),
            move |s| l.lock().unwrap().push(s.clone()),
        );
        (cap, got, log)
    }

    #[test]
    fn audio_is_resampled_to_16k_and_kept_in_the_pre_roll() {
        let fake = Fake::default();
        let (mut cap, got, _log) = start(&fake, Some("Fake mic"));
        wait_for("running", || {
            matches!(cap.status(), CaptureStatus::Running { .. })
        });
        assert_eq!(
            cap.status(),
            CaptureStatus::Running {
                device: "Fake mic".into(),
                rate: 48_000
            }
        );
        assert_eq!(fake.asked.lock().unwrap()[0].as_deref(), Some("Fake mic"));
        {
            let mut streams = fake.streams.lock().unwrap();
            for _ in 0..4_800 {
                streams[0].producer.push(0.25).unwrap();
            }
        }
        // 4800 samples at 48 kHz are 100 ms, which is 1600 samples at 16 kHz.
        wait_for("1600 samples", || got.lock().unwrap().len() >= 1_599);
        let n = got.lock().unwrap().len();
        assert!((1_598..=1_601).contains(&n), "{n}");
        assert!(
            got.lock().unwrap()[2..]
                .iter()
                .all(|v| (v - 0.25).abs() < 1e-5)
        );
        assert_eq!(cap.pre_roll().len(), n);
        cap.stop();
        assert!(cap.join(Duration::from_secs(2)));
    }

    #[test]
    fn an_unavailable_microphone_is_retried_until_it_appears() {
        let fake = Fake::default();
        fake.failures
            .lock()
            .unwrap()
            .extend([AudioError::PermissionDenied, AudioError::NoDevice]);
        let (mut cap, _got, log) = start(&fake, None);
        wait_for("running after two failures", || {
            matches!(cap.status(), CaptureStatus::Running { .. })
        });
        let seen = log.lock().unwrap().clone();
        assert!(seen.contains(&CaptureStatus::Unavailable(AudioError::PermissionDenied)));
        assert!(seen.contains(&CaptureStatus::Unavailable(AudioError::NoDevice)));
        assert_eq!(fake.asked.lock().unwrap().len(), 3);
        assert!(fake.asked.lock().unwrap().iter().all(|d| d.is_none()));
        cap.stop();
        assert!(cap.join(Duration::from_secs(2)));
    }

    #[test]
    fn a_lost_device_closes_the_stream_and_the_capture_recovers_without_a_restart() {
        let fake = Fake::default();
        let (mut cap, got, log) = start(&fake, None);
        wait_for("first stream", || fake.streams.lock().unwrap().len() == 1);
        {
            let mut s = fake.streams.lock().unwrap();
            for _ in 0..960 {
                s[0].producer.push(0.5).unwrap();
            }
        }
        wait_for("first audio", || !got.lock().unwrap().is_empty());
        // The device goes away. The dead stream is closed, then a new one is opened.
        let (first_closed, first_lost) = {
            let s = fake.streams.lock().unwrap();
            (s[0].closed.clone(), s[0].lost.clone())
        };
        first_lost.store(true, Ordering::Release);
        wait_for("second stream", || fake.streams.lock().unwrap().len() == 2);
        assert!(
            first_closed.load(Ordering::SeqCst),
            "the dead stream was closed"
        );
        assert!(
            log.lock()
                .unwrap()
                .contains(&CaptureStatus::Unavailable(AudioError::DeviceLost))
        );
        wait_for("running again", || {
            matches!(cap.status(), CaptureStatus::Running { .. })
        });
        // Audio flows again, and the pre-roll started fresh instead of splicing old audio on.
        let before = got.lock().unwrap().len();
        {
            let mut s = fake.streams.lock().unwrap();
            for _ in 0..960 {
                s[1].producer.push(0.5).unwrap();
            }
        }
        wait_for("audio from the new stream", || {
            got.lock().unwrap().len() > before
        });
        assert!(cap.pre_roll().len() <= got.lock().unwrap().len() - before + 1);
        cap.stop();
        assert!(cap.join(Duration::from_secs(2)));
    }

    #[test]
    fn what_was_written_just_before_the_device_vanished_is_not_thrown_away() {
        let fake = Fake::default();
        let (mut cap, got, _log) = start(&fake, None);
        wait_for("stream", || fake.streams.lock().unwrap().len() == 1);
        {
            let mut s = fake.streams.lock().unwrap();
            for _ in 0..3_000 {
                s[0].producer.push(0.5).unwrap();
            }
            s[0].lost.store(true, Ordering::Release);
        }
        wait_for("the 1000 samples", || got.lock().unwrap().len() >= 990);
        cap.stop();
        assert!(cap.join(Duration::from_secs(2)));
    }

    #[test]
    fn stopping_ends_the_thread_quickly_and_closes_the_stream() {
        let fake = Fake::default();
        let (mut cap, _got, log) = start(&fake, None);
        wait_for("stream", || fake.streams.lock().unwrap().len() == 1);
        let closed = fake.streams.lock().unwrap()[0].closed.clone();
        let t = Instant::now();
        cap.stop();
        assert!(cap.join(Duration::from_secs(2)));
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "{:?}",
            t.elapsed()
        );
        assert!(closed.load(Ordering::SeqCst));
        assert_eq!(cap.status(), CaptureStatus::Stopped);
        assert_eq!(log.lock().unwrap().last(), Some(&CaptureStatus::Stopped));
    }

    #[test]
    fn stopping_while_the_microphone_is_unavailable_does_not_wait_for_the_retry() {
        let fake = Fake::default();
        fake.failures
            .lock()
            .unwrap()
            .extend(std::iter::repeat_n(AudioError::NoDevice, 100));
        let mut cap = Capture::start(
            fake.clone(),
            None,
            Duration::from_secs(30), // a long retry: stop must not sit through it
            |_| {},
            |_| {},
        );
        wait_for("first attempt", || !fake.asked.lock().unwrap().is_empty());
        let t = Instant::now();
        cap.stop();
        assert!(cap.join(Duration::from_secs(2)));
        assert!(t.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn a_panic_in_the_backend_stops_the_capture_without_taking_the_app_down() {
        struct Boom;
        impl Backend for Boom {
            fn open(&mut self, _: Option<&str>) -> Result<Opened, AudioError> {
                panic!("driver bug");
            }
        }
        let mut cap = Capture::start(Boom, None, Duration::from_millis(10), |_| {}, |_| {});
        assert!(cap.join(Duration::from_secs(2)));
        assert_eq!(cap.status(), CaptureStatus::Stopped);
    }
}
