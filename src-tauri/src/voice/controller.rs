//! Push-to-talk, start to finish (M3 step 4): begin a voice turn, capture until speech ends or the
//! key is released, turn the audio into text, and hand the text to the turn loop. Every step has a
//! cancel and a time limit, nothing here runs inference on a Tokio worker, and the audio stays in
//! RAM and is dropped when the request is over.
//!
//! The orchestrator still owns the only state machine: this module asks it to move the phase
//! (`listening`, `transcribing`, then the turn loop's `thinking`) and publishes what it returns.

use super::endpoint::{Decision, EndpointConfig, Endpointer};
use super::transcribe::{Transcriber, VoiceError};
use crate::audio::capture::{AudioError, Backend, Capture, CaptureStatus, RETRY_EVERY};
use crate::ipc::types::{ErrorCode, JarvisError, MicState, Transcript};
use crate::orchestrator::turn::{TurnRunner, TurnSink};
use crate::orchestrator::{Orchestrator, TransitionError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Where a voice turn reports to: everything a typed turn reports, plus what was heard.
pub trait VoiceSink: TurnSink {
    /// The `transcript.final` event: the text the turn will use.
    fn transcript_final(&self, t: Transcript);
}

#[derive(Clone, Copy)]
pub struct VoiceConfig {
    pub endpoint: EndpointConfig,
    /// How long speech recognition may take before the request is given up (spec §10: 1.2 s is the
    /// budget for a 5 s utterance; this is the point where something is clearly wrong).
    pub transcribe_timeout: Duration,
    /// How often the capture engine retries a microphone it could not open.
    pub retry: Duration,
    /// Give up if no audio at all arrives this long after the start: a microphone that hangs while
    /// it opens (macOS asking for permission) must not leave a hands-free start listening forever.
    pub no_audio_timeout: Duration,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            endpoint: EndpointConfig::default(),
            transcribe_timeout: Duration::from_secs(10),
            retry: RETRY_EVERY,
            no_audio_timeout: Duration::from_secs(20),
        }
    }
}

type BackendFactory = dyn Fn() -> Box<dyn Backend> + Send + Sync;

pub struct VoiceController {
    orch: Arc<Orchestrator>,
    runner: Arc<TurnRunner>,
    sink: Arc<dyn VoiceSink>,
    backend: Box<BackendFactory>,
    /// The chosen microphone name, read when a capture starts (`null` means the system default).
    device: Box<dyn Fn() -> Option<String> + Send + Sync>,
    transcriber: Arc<Mutex<Box<dyn Transcriber>>>,
    cfg: VoiceConfig,
    active: Mutex<Option<Active>>,
}

struct Active {
    id: String,
    /// Set by the key release: end the capture and use what was said.
    stop: Arc<AtomicBool>,
    /// Set by a cancel: drop everything.
    #[cfg_attr(not(test), allow(dead_code))] // read by `cancel`, which the wake word will call
    cancel: CancellationToken,
}

enum Msg {
    Audio(Vec<f32>),
    Status(CaptureStatus),
}

enum Outcome {
    Audio(Vec<f32>),
    NoSpeech,
    /// Not one chunk of audio arrived in time: the microphone never started.
    NoAudio,
    Cancelled,
    Lost(AudioError),
}

fn err(code: ErrorCode, message: &str, retryable: bool, id: Option<&str>) -> JarvisError {
    JarvisError {
        code,
        message: message.into(),
        retryable,
        request_id: id.map(str::to_owned),
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the person is told when the microphone cannot be used.
fn audio_error(e: &AudioError, id: &str) -> JarvisError {
    match e {
        AudioError::PermissionDenied => err(
            ErrorCode::MicPermissionDenied,
            "JARVIS is not allowed to use the microphone.",
            false,
            Some(id),
        ),
        AudioError::NoDevice => err(
            ErrorCode::DeviceLost,
            "No microphone was found.",
            true,
            Some(id),
        ),
        AudioError::DeviceLost => err(
            ErrorCode::DeviceLost,
            "The microphone was lost.",
            true,
            Some(id),
        ),
        AudioError::Failed(_) => err(
            ErrorCode::DeviceLost,
            "The microphone could not be opened.",
            true,
            Some(id),
        ),
    }
}

impl VoiceController {
    pub fn new(
        orch: Arc<Orchestrator>,
        runner: Arc<TurnRunner>,
        sink: Arc<dyn VoiceSink>,
        backend: Box<BackendFactory>,
        device: Box<dyn Fn() -> Option<String> + Send + Sync>,
        transcriber: Box<dyn Transcriber>,
        cfg: VoiceConfig,
    ) -> Arc<Self> {
        Arc::new(Self {
            orch,
            runner,
            sink,
            backend,
            device,
            transcriber: Arc::new(Mutex::new(transcriber)),
            cfg,
            active: Mutex::new(None),
        })
    }

    /// Push to talk, key down. A start while a capture is already running does nothing and says so
    /// with `Ok` (the contract). Must be called inside the Tokio runtime: the reply is streamed
    /// there once speech has been turned into text.
    pub fn start_capture(self: &Arc<Self>) -> Result<(), JarvisError> {
        let mut active = lock(&self.active);
        if active.is_some() {
            return Ok(());
        }
        let handle = tokio::runtime::Handle::try_current().map_err(|_| {
            err(
                ErrorCode::Internal,
                "Voice input is not ready yet.",
                true,
                None,
            )
        })?;
        let (id, snap) = self.orch.begin_listening().map_err(|e| match e {
            TransitionError::MicNotAvailable => err(
                ErrorCode::Internal,
                "The microphone is muted or not available.",
                false,
                None,
            ),
            _ => err(
                ErrorCode::Internal,
                "JARVIS is busy with another request.",
                false,
                None,
            ),
        })?;
        self.sink.snapshot(snap);
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = CancellationToken::new();
        *active = Some(Active {
            id: id.clone(),
            stop: stop.clone(),
            cancel: cancel.clone(),
        });
        drop(active);
        let me = Arc::clone(self);
        let rid = id.clone();
        let spawned = std::thread::Builder::new()
            .name("jarvis-voice".into())
            .spawn(move || {
                // A panic in a driver or model must not take the app down, and must not leave the
                // assistant stuck in `listening`.
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    me.session(&rid, &stop, &cancel, &handle);
                }));
                if r.is_err() {
                    tracing::error!("the voice session panicked");
                    me.fail(
                        &rid,
                        err(
                            ErrorCode::Internal,
                            "Voice input stopped unexpectedly.",
                            true,
                            Some(&rid),
                        ),
                    );
                }
                me.clear_active(&rid);
            });
        if spawned.is_err() {
            self.clear_active(&id);
            let e = err(
                ErrorCode::Internal,
                "Could not start voice input.",
                true,
                Some(&id),
            );
            self.fail(&id, e.clone());
            return Err(e);
        }
        Ok(())
    }

    /// Push to talk, key up: end the capture and use what was said. Safe when nothing is capturing.
    pub fn stop_capture(&self) {
        if let Some(a) = lock(&self.active).as_ref() {
            a.stop.store(true, Ordering::Release);
        }
    }

    /// Cancel: drop the audio and return to idle at once. Safe when nothing is capturing. The window
    /// does not need it (Esc and mute end a capture through the orchestrator, see `session`); the
    /// wake word, which has no key, will.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn cancel(&self) {
        let Some((id, token)) = lock(&self.active)
            .as_ref()
            .map(|a| (a.id.clone(), a.cancel.clone()))
        else {
            return;
        };
        token.cancel();
        self.end_cancelled(&id);
    }

    fn clear_active(&self, id: &str) {
        let mut a = lock(&self.active);
        if a.as_ref().is_some_and(|x| x.id == id) {
            *a = None;
        }
    }

    fn fail(&self, id: &str, e: JarvisError) {
        if let Ok(snap) = self.orch.fail(id, e) {
            self.sink.snapshot(snap);
        }
    }

    fn end_cancelled(&self, id: &str) {
        if let Ok(snap) = self.orch.cancel(id) {
            self.sink.snapshot(snap);
        }
    }

    /// Runs on the voice thread: capture, then recognise, then hand over.
    fn session(
        &self,
        id: &str,
        stop: &AtomicBool,
        cancel: &CancellationToken,
        handle: &tokio::runtime::Handle,
    ) {
        let _in_runtime = handle.enter();
        let (tx, rx) = mpsc::channel::<Msg>();
        let tx_status = tx.clone();
        let capture = Capture::start(
            (self.backend)(),
            (self.device)(),
            self.cfg.retry,
            move |c| {
                let _ = tx.send(Msg::Audio(c.to_vec()));
            },
            move |s| {
                let _ = tx_status.send(Msg::Status(s.clone()));
            },
        );
        let mut endpoint = Endpointer::new(self.cfg.endpoint);
        // The capture ends at once when the person mutes (the button, the hotkey, a typed or a
        // spoken "mute") or when the request is cancelled from outside (Esc, "stop"): the
        // orchestrator's snapshot is the single place that is looked at, so the microphone can
        // never keep recording for a request that is over.
        let muted = || {
            let n = self.orch.snapshot();
            n.mic == MicState::Muted || n.active_request_id.as_deref() != Some(id)
        };
        // The device is open: only now is the microphone `capturing`, which is the cue to speak.
        let mut on_open = || {
            if let Ok(snap) = self.orch.mic_open(id) {
                self.sink.snapshot(snap);
            }
        };
        let outcome = collect(
            &rx,
            &mut endpoint,
            stop,
            cancel,
            &muted,
            self.cfg.no_audio_timeout,
            &mut on_open,
        );
        capture.stop();
        drop(capture);
        match outcome {
            Outcome::Cancelled => self.end_cancelled(id),
            Outcome::Lost(e) => self.fail(id, audio_error(&e, id)),
            Outcome::NoSpeech => self.no_speech(id),
            Outcome::NoAudio => self.fail(
                id,
                err(
                    ErrorCode::DeviceLost,
                    "The microphone did not start. Check that JARVIS is allowed to use it.",
                    true,
                    Some(id),
                ),
            ),
            Outcome::Audio(audio) => self.recognise(id, audio, cancel),
        }
    }

    fn no_speech(&self, id: &str) {
        self.fail(
            id,
            err(
                ErrorCode::Internal,
                "I did not hear anything.",
                true,
                Some(id),
            ),
        );
    }

    fn recognise(&self, id: &str, audio: Vec<f32>, cancel: &CancellationToken) {
        match self
            .orch
            .transition(id, crate::ipc::types::AssistantPhase::Transcribing)
        {
            Ok(snap) => self.sink.snapshot(snap),
            Err(_) => return, // cancelled in the meantime: the orchestrator already moved on
        }
        let began = std::time::Instant::now();
        let text = self.transcribe_bounded(audio);
        match &text {
            Ok(t) => tracing::info!(
                ms = began.elapsed().as_millis() as u64,
                words = t.split_whitespace().count(),
                "voice: recognised"
            ),
            Err(_) => tracing::info!(
                ms = began.elapsed().as_millis() as u64,
                "voice: recognition gave no text"
            ),
        }
        if cancel.is_cancelled() {
            return; // `cancel` already returned the assistant to idle
        }
        match text {
            Ok(t) if t.trim().is_empty() => self.no_speech(id),
            Ok(t) => {
                let t = t.trim().to_owned();
                self.sink.transcript_final(Transcript {
                    request_id: id.to_owned(),
                    text: t.clone(),
                });
                let sink: Arc<dyn TurnSink> = self.sink.clone();
                if let Err(e) = self.runner.continue_voice(id, t, sink) {
                    self.fail(id, e);
                }
            }
            Err(VoiceError::ModelMissing) => self.fail(
                id,
                err(
                    ErrorCode::ModelMissing,
                    "The speech models are not installed yet.",
                    false,
                    Some(id),
                ),
            ),
            Err(VoiceError::TimedOut) => self.fail(
                id,
                err(
                    ErrorCode::Internal,
                    "Speech recognition took too long.",
                    true,
                    Some(id),
                ),
            ),
            Err(VoiceError::Failed(why)) => {
                tracing::warn!(error = %why, "speech recognition failed");
                self.fail(
                    id,
                    err(
                        ErrorCode::Internal,
                        "Speech recognition failed.",
                        true,
                        Some(id),
                    ),
                );
            }
        }
    }

    /// Recognition on its own thread with a time limit. A call that overruns is abandoned (its
    /// result is dropped), so one slow request cannot hold the assistant in `transcribing`.
    fn transcribe_bounded(&self, audio: Vec<f32>) -> Result<String, VoiceError> {
        let t = Arc::clone(&self.transcriber);
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("jarvis-stt".into())
            .spawn(move || {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    lock(&t).transcribe(&audio)
                }))
                .unwrap_or_else(|_| Err(VoiceError::Failed("the recogniser panicked".into())));
                let _ = tx.send(r);
            })
            .map_err(|e| VoiceError::Failed(e.to_string()))?;
        match rx.recv_timeout(self.cfg.transcribe_timeout) {
            Ok(r) => r,
            Err(_) => Err(VoiceError::TimedOut),
        }
    }
}

/// One line about how a capture ended: facts only, never audio and never text. It tells apart the
/// usual causes of a garbled sentence: cut short by a pause or by the key, too quiet, or too long.
fn log_capture_end(why: &str, audio: &[f32]) {
    let peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    tracing::info!(
        why,
        seconds = format!("{:.1}", audio.len() as f32 / 16_000.0),
        peak = format!("{peak:.2}"),
        "voice: capture ended"
    );
}

/// Collects audio until speech ends, the key is released, the capture is cancelled or the
/// microphone is lost. **A mute or a cancel always wins**: if one is in force at the moment a result
/// is handed over, the audio is dropped. Without this, a mute that lands a few milliseconds before
/// the window's "key released" (which follows every mute) could be overtaken by it inside one
/// 20 ms pass of the loop, and what was said would be sent after the person muted.
fn collect(
    rx: &Receiver<Msg>,
    endpoint: &mut Endpointer,
    stop: &AtomicBool,
    cancel: &CancellationToken,
    muted: &dyn Fn() -> bool,
    no_audio_timeout: Duration,
    on_open: &mut dyn FnMut(),
) -> Outcome {
    let outcome = collect_until_done(rx, endpoint, stop, cancel, muted, no_audio_timeout, on_open);
    match outcome {
        Outcome::Audio(_) if cancel.is_cancelled() || muted() => Outcome::Cancelled,
        other => other,
    }
}

fn collect_until_done(
    rx: &Receiver<Msg>,
    endpoint: &mut Endpointer,
    stop: &AtomicBool,
    cancel: &CancellationToken,
    muted: &dyn Fn() -> bool,
    no_audio_timeout: Duration,
    on_open: &mut dyn FnMut(),
) -> Outcome {
    let mut audio: Vec<f32> = Vec::new();
    let started = std::time::Instant::now();
    loop {
        if cancel.is_cancelled() || muted() {
            return Outcome::Cancelled;
        }
        if audio.is_empty() && started.elapsed() >= no_audio_timeout {
            return Outcome::NoAudio;
        }
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Msg::Audio(c)) => {
                if audio.is_empty() {
                    tracing::info!(
                        ms = started.elapsed().as_millis() as u64,
                        "voice: first sound arrived"
                    );
                }
                audio.extend_from_slice(&c);
                match endpoint.push(&c) {
                    Decision::Continue => {}
                    Decision::SpeechEnded => {
                        log_capture_end("a pause after speech", &audio);
                        return Outcome::Audio(audio);
                    }
                    Decision::TooLong => {
                        log_capture_end("the 15 s limit", &audio);
                        return Outcome::Audio(audio);
                    }
                    Decision::NoSpeech => {
                        log_capture_end("no speech heard", &audio);
                        return Outcome::NoSpeech;
                    }
                }
            }
            Ok(Msg::Status(CaptureStatus::Unavailable(e))) => return Outcome::Lost(e),
            Ok(Msg::Status(CaptureStatus::Running { .. })) => {
                tracing::info!(
                    ms = started.elapsed().as_millis() as u64,
                    "voice: microphone open"
                );
                on_open();
            }
            Ok(Msg::Status(_)) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Outcome::Lost(AudioError::Failed("capture ended".into()));
            }
        }
        if stop.load(Ordering::Acquire) {
            // The key was released: use what was said, including audio already queued.
            while let Ok(m) = rx.try_recv() {
                if let Msg::Audio(c) = m {
                    audio.extend_from_slice(&c);
                    endpoint.push(&c);
                }
            }
            return if endpoint.has_speech() {
                log_capture_end("the key was released", &audio);
                Outcome::Audio(audio)
            } else {
                log_capture_end("the key was released, no speech heard", &audio);
                Outcome::NoSpeech
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::capture::Opened;
    use crate::intelligence::spend::{Caps, Day, Price, SpendMeter};
    use crate::ipc::types::{AssistantPhase, MicState, Snapshot, TurnStreamEvent};
    use crate::voice::transcribe::MissingModels;
    use rtrb::RingBuffer;
    use std::sync::atomic::AtomicU64;
    use std::time::Instant;

    const NOW: u64 = 1_793_491_200;

    #[derive(Default)]
    struct Rec {
        snaps: Mutex<Vec<Snapshot>>,
        events: Mutex<Vec<TurnStreamEvent>>,
        heard: Mutex<Vec<Transcript>>,
    }
    impl TurnSink for Rec {
        fn event(&self, e: TurnStreamEvent) {
            lock(&self.events).push(e);
        }
        fn snapshot(&self, s: Snapshot) {
            lock(&self.snaps).push(s);
        }
    }
    impl VoiceSink for Rec {
        fn transcript_final(&self, t: Transcript) {
            lock(&self.heard).push(t);
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
    }

    fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while !ok() {
            assert!(Instant::now() < end, "timed out waiting for: {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn tone(ms: usize, amp: f32) -> Vec<f32> {
        (0..ms * 16)
            .map(|i| amp * (i as f32 * 220.0 * std::f32::consts::TAU / 16_000.0).sin())
            .collect()
    }

    /// Background, one spoken phrase, then quiet: the shape of a real request.
    fn phrase() -> Vec<f32> {
        let mut a = tone(300, 0.002);
        a.extend(tone(900, 0.3));
        a.extend(tone(1_000, 0.002));
        a
    }

    /// A fake microphone that has `script` waiting in its ring when opened.
    struct Mic {
        script: Vec<f32>,
        lost: bool,
        fail: Option<AudioError>,
        opened: Arc<AtomicU64>,
    }
    impl Backend for Mic {
        fn open(&mut self, _device: Option<&str>) -> Result<Opened, AudioError> {
            self.opened.fetch_add(1, Ordering::SeqCst);
            if let Some(e) = self.fail.clone() {
                return Err(e);
            }
            let (mut p, ring) = RingBuffer::new(16_000 * 30);
            for s in &self.script {
                p.push(*s).unwrap();
            }
            Ok(Opened {
                guard: Box::new(()),
                rate: 16_000,
                device_name: "Fake mic".into(),
                ring,
                lost: Arc::new(AtomicBool::new(self.lost)),
                overruns: Arc::new(AtomicU64::new(0)),
            })
        }
    }

    struct Says(&'static str);
    impl Transcriber for Says {
        fn transcribe(&mut self, audio: &[f32]) -> Result<String, VoiceError> {
            assert!(!audio.is_empty());
            Ok(self.0.to_owned())
        }
    }
    struct Slow;
    impl Transcriber for Slow {
        fn transcribe(&mut self, _: &[f32]) -> Result<String, VoiceError> {
            std::thread::sleep(Duration::from_millis(800));
            Ok("late".into())
        }
    }
    struct Panics;
    impl Transcriber for Panics {
        fn transcribe(&mut self, _: &[f32]) -> Result<String, VoiceError> {
            panic!("model bug");
        }
    }

    struct Setup {
        ctl: Arc<VoiceController>,
        orch: Arc<Orchestrator>,
        sink: Arc<Rec>,
        opened: Arc<AtomicU64>,
    }

    fn setup(
        script: Vec<f32>,
        lost: bool,
        fail: Option<AudioError>,
        t: Box<dyn Transcriber>,
    ) -> Setup {
        let opened = Arc::new(AtomicU64::new(0));
        let o = opened.clone();
        setup_with(
            Box::new(move || {
                Box::new(Mic {
                    script: script.clone(),
                    lost,
                    fail: fail.clone(),
                    opened: o.clone(),
                })
            }),
            opened,
            t,
            Duration::from_secs(20),
        )
    }

    fn setup_with(
        backend: Box<BackendFactory>,
        opened: Arc<AtomicU64>,
        t: Box<dyn Transcriber>,
        no_audio_timeout: Duration,
    ) -> Setup {
        let orch = Arc::new(Orchestrator::default());
        let spend = Arc::new(SpendMeter::new(Caps::default(), Day::from_unix_secs(NOW)));
        let runner = Arc::new(TurnRunner::new(
            orch.clone(),
            spend,
            Box::new(|| None),
            Box::new(|| crate::orchestrator::turn::TurnConfig {
                model: "test".into(),
                price: Price {
                    input_per_mtok: 1,
                    output_per_mtok: 1,
                },
            }),
            Box::new(|| NOW),
        ));
        let sink = Arc::new(Rec::default());
        let ctl = VoiceController::new(
            orch.clone(),
            runner,
            sink.clone(),
            backend,
            Box::new(|| Some("Fake mic".into())),
            t,
            VoiceConfig {
                transcribe_timeout: Duration::from_millis(300),
                retry: Duration::from_millis(20),
                no_audio_timeout,
                ..VoiceConfig::default()
            },
        );
        Setup {
            ctl,
            orch,
            sink,
            opened,
        }
    }

    fn idle(s: &Setup) -> bool {
        let n = s.orch.snapshot();
        n.phase != AssistantPhase::Listening
            && n.phase != AssistantPhase::Transcribing
            && n.phase != AssistantPhase::Thinking
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_spoken_phrase_becomes_a_transcript_and_a_normal_turn() {
        let s = setup(phrase(), false, None, Box::new(Says("what can you do")));
        s.ctl.start_capture().unwrap();
        // The phase went listening at once, the microphone became capturing when the device had
        // opened, then the turn ran and ended.
        assert_eq!(s.sink.snaps_first().phase, AssistantPhase::Listening);
        assert_eq!(s.sink.snaps_first().mic, MicState::Off);
        wait_for("the microphone to report capturing", || {
            lock(&s.sink.snaps)
                .iter()
                .any(|n| n.phase == AssistantPhase::Listening && n.mic == MicState::Capturing)
        });
        wait_for("the turn to end", || {
            !s.sink.text().is_empty() && s.orch.snapshot().phase == AssistantPhase::Idle
        });
        assert_eq!(lock(&s.sink.heard)[0].text, "what can you do");
        let phases: Vec<_> = lock(&s.sink.snaps).iter().map(|n| n.phase).collect();
        let pos = |p| phases.iter().position(|x| *x == p).unwrap();
        assert!(pos(AssistantPhase::Listening) < pos(AssistantPhase::Transcribing));
        assert!(pos(AssistantPhase::Transcribing) < pos(AssistantPhase::Thinking));
        // The microphone is back to what it was, and the request id stayed the same throughout.
        assert_eq!(s.orch.snapshot().mic, MicState::Off);
        let ids: std::collections::HashSet<_> = lock(&s.sink.snaps)
            .iter()
            .filter_map(|n| n.active_request_id.clone())
            .collect();
        assert_eq!(ids.len(), 1);
        assert_eq!(
            lock(&s.sink.heard)[0].request_id,
            *ids.iter().next().unwrap()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn releasing_the_key_uses_what_was_said_so_far() {
        // Speech with no trailing quiet: only the key release can end it.
        let mut a = tone(300, 0.002);
        a.extend(tone(900, 0.3));
        let s = setup(a, false, None, Box::new(Says("what can you do")));
        s.ctl.start_capture().unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(s.orch.snapshot().phase, AssistantPhase::Listening);
        s.ctl.stop_capture();
        wait_for("a transcript", || !lock(&s.sink.heard).is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn releasing_the_key_after_nothing_was_said_says_so_and_nothing_runs() {
        let s = setup(tone(500, 0.002), false, None, Box::new(Says("never")));
        s.ctl.start_capture().unwrap();
        std::thread::sleep(Duration::from_millis(150));
        s.ctl.stop_capture();
        wait_for("an error", || {
            s.orch.snapshot().phase == AssistantPhase::Error
        });
        let n = s.orch.snapshot();
        assert_eq!(n.last_error.unwrap().message, "I did not hear anything.");
        assert_eq!(n.mic, MicState::Off);
        assert!(lock(&s.sink.heard).is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_second_start_while_capturing_does_nothing() {
        let mut a = tone(300, 0.002);
        a.extend(tone(900, 0.3));
        let s = setup(a, false, None, Box::new(Says("what can you do")));
        s.ctl.start_capture().unwrap();
        s.ctl.start_capture().unwrap();
        s.ctl.start_capture().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            s.opened.load(Ordering::SeqCst),
            1,
            "one microphone, opened once"
        );
        s.ctl.cancel();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancel_returns_to_idle_at_once_and_nothing_is_transcribed() {
        let mut a = tone(300, 0.002);
        a.extend(tone(900, 0.3));
        let s = setup(a, false, None, Box::new(Says("never")));
        s.ctl.start_capture().unwrap();
        s.ctl.cancel();
        let n = s.orch.snapshot();
        assert_eq!(
            (n.phase, n.active_request_id, n.mic),
            (AssistantPhase::Idle, None, MicState::Off)
        );
        // Cancelling again, or stopping with nothing running, is safe.
        s.ctl.cancel();
        s.ctl.stop_capture();
        std::thread::sleep(Duration::from_millis(200));
        assert!(lock(&s.sink.heard).is_empty());
        assert_eq!(s.orch.snapshot().phase, AssistantPhase::Idle);
        // And a new capture can start afterwards.
        wait_for("the old session to clear", || lock(&s.ctl.active).is_none());
        s.ctl.start_capture().unwrap();
        s.ctl.cancel();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_muted_microphone_or_a_busy_assistant_refuses_to_start() {
        let s = setup(phrase(), false, None, Box::new(Says("x")));
        s.orch.set_mute(true);
        let e = s.ctl.start_capture().unwrap_err();
        assert_eq!(e.message, "The microphone is muted or not available.");
        s.orch.set_mute(false);
        let (_id, _) = s.orch.begin_turn().unwrap();
        let e = s.ctl.start_capture().unwrap_err();
        assert_eq!(e.message, "JARVIS is busy with another request.");
        assert_eq!(
            s.opened.load(Ordering::SeqCst),
            0,
            "the microphone was never opened"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_missing_or_lost_microphone_is_a_plain_error_and_the_assistant_recovers() {
        for (fail, lost, code, text) in [
            (
                Some(AudioError::NoDevice),
                false,
                ErrorCode::DeviceLost,
                "No microphone was found.",
            ),
            (
                Some(AudioError::PermissionDenied),
                false,
                ErrorCode::MicPermissionDenied,
                "JARVIS is not allowed to use the microphone.",
            ),
            (
                None,
                true,
                ErrorCode::DeviceLost,
                "The microphone was lost.",
            ),
        ] {
            let s = setup(tone(100, 0.002), lost, fail, Box::new(Says("x")));
            s.ctl.start_capture().unwrap();
            wait_for("an error", || {
                s.orch.snapshot().phase == AssistantPhase::Error
            });
            let e = s.orch.snapshot().last_error.unwrap();
            assert_eq!((e.code, e.message.as_str()), (code, text));
            assert_eq!(s.orch.snapshot().mic, MicState::Off);
            // The next attempt is allowed.
            wait_for("the session to clear", || lock(&s.ctl.active).is_none());
            assert!(idle(&s));
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn models_that_are_not_installed_say_so() {
        let s = setup(phrase(), false, None, Box::new(MissingModels));
        s.ctl.start_capture().unwrap();
        wait_for("an error", || {
            s.orch.snapshot().phase == AssistantPhase::Error
        });
        let e = s.orch.snapshot().last_error.unwrap();
        assert_eq!(e.code, ErrorCode::ModelMissing);
        assert!(!e.retryable);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slow_or_crashing_recognition_cannot_hold_the_assistant() {
        let s = setup(phrase(), false, None, Box::new(Slow));
        let t = Instant::now();
        s.ctl.start_capture().unwrap();
        wait_for("an error", || {
            s.orch.snapshot().phase == AssistantPhase::Error
        });
        assert_eq!(
            s.orch.snapshot().last_error.unwrap().message,
            "Speech recognition took too long."
        );
        assert!(t.elapsed() < Duration::from_secs(3));
        let s = setup(phrase(), false, None, Box::new(Panics));
        s.ctl.start_capture().unwrap();
        wait_for("an error", || {
            s.orch.snapshot().phase == AssistantPhase::Error
        });
        assert_eq!(
            s.orch.snapshot().last_error.unwrap().message,
            "Speech recognition failed."
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_spoken_mute_mutes_and_starts_no_turn() {
        let s = setup(phrase(), false, None, Box::new(Says("mute")));
        s.ctl.start_capture().unwrap();
        wait_for("muted", || s.orch.snapshot().mic == MicState::Muted);
        let n = s.orch.snapshot();
        assert_eq!((n.phase, n.active_request_id), (AssistantPhase::Idle, None));
        assert_eq!(s.sink.text(), "Muted.");
    }

    #[test]
    fn outside_the_runtime_a_start_is_refused_with_a_plain_message_and_changes_nothing() {
        let s = setup(phrase(), false, None, Box::new(Says("x")));
        let e = s.ctl.start_capture().unwrap_err();
        assert_eq!(e.message, "Voice input is not ready yet.");
        assert_eq!(s.orch.snapshot().phase, AssistantPhase::Idle);
    }

    impl Rec {
        fn snaps_first(&self) -> Snapshot {
            lock(&self.snaps).first().cloned().expect("a snapshot")
        }
    }

    #[test]
    fn the_audio_error_messages_never_carry_driver_text() {
        let e = audio_error(&AudioError::Failed("secret driver detail".into()), "r");
        assert!(!e.message.contains("secret"));
    }

    /// A microphone whose `open` never returns (macOS waiting for the person to answer its
    /// permission question).
    struct Hangs;
    impl Backend for Hangs {
        fn open(&mut self, _device: Option<&str>) -> Result<Opened, AudioError> {
            std::thread::sleep(Duration::from_secs(30));
            Err(AudioError::NoDevice)
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_microphone_that_never_starts_ends_the_turn_instead_of_listening_forever() {
        let s = setup_with(
            Box::new(|| Box::new(Hangs)),
            Arc::new(AtomicU64::new(0)),
            Box::new(Says("never")),
            Duration::from_millis(300),
        );
        s.ctl.start_capture().unwrap();
        assert_eq!(s.orch.snapshot().phase, AssistantPhase::Listening);
        wait_for("the turn to give up", || {
            s.orch.snapshot().phase == AssistantPhase::Error
        });
        let n = s.orch.snapshot();
        let e = n.last_error.unwrap();
        assert_eq!(e.code, ErrorCode::DeviceLost);
        assert!(e.message.starts_with("The microphone did not start"));
        assert_eq!(n.mic, MicState::Off);
        assert!(lock(&s.sink.heard).is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn muting_during_a_capture_stops_it_at_once_and_nothing_is_transcribed() {
        // Speech with no quiet after it: only a mute or a key release could end this capture.
        let mut a = tone(300, 0.002);
        a.extend(tone(900, 0.3));
        let s = setup(a, false, None, Box::new(Says("never")));
        s.ctl.start_capture().unwrap();
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(s.orch.snapshot().mic, MicState::Capturing);
        // Any route to the orchestrator counts: this is what the button, the hotkey and a typed
        // "mute" all end in.
        s.orch.set_mute(true);
        wait_for("the capture to end", || {
            s.orch.snapshot().phase == AssistantPhase::Idle
        });
        let n = s.orch.snapshot();
        assert_eq!((n.mic, n.active_request_id), (MicState::Muted, None));
        assert!(lock(&s.sink.heard).is_empty(), "nothing was transcribed");
        // Unmuting brings the microphone back to what it was before the capture, not to `Capturing`.
        assert_eq!(s.orch.set_mute(false).mic, MicState::Off);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_cancel_from_outside_the_controller_stops_the_microphone_too() {
        // Esc goes through the turn runner, not through the controller: the capture must still end.
        let mut a = tone(300, 0.002);
        a.extend(tone(900, 0.3));
        let s = setup(a, false, None, Box::new(Says("never")));
        s.ctl.start_capture().unwrap();
        std::thread::sleep(Duration::from_millis(150));
        let id = s.orch.snapshot().active_request_id.unwrap();
        s.orch.cancel(&id).unwrap();
        wait_for("the controller to let go", || lock(&s.ctl.active).is_none());
        assert!(lock(&s.sink.heard).is_empty());
        assert_eq!(s.orch.snapshot().phase, AssistantPhase::Idle);
    }

    /// A microphone that takes a while to open, as a real device can (Windows, a busy driver).
    struct SlowOpen(Mic);
    impl Backend for SlowOpen {
        fn open(&mut self, device: Option<&str>) -> Result<Opened, AudioError> {
            std::thread::sleep(Duration::from_millis(400));
            self.0.open(device)
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_microphone_only_reports_capturing_once_a_slow_device_has_opened() {
        let mut script = tone(300, 0.002);
        script.extend(tone(900, 0.3));
        let opened = Arc::new(AtomicU64::new(0));
        let o = opened.clone();
        let s = setup_with(
            Box::new(move || {
                Box::new(SlowOpen(Mic {
                    script: script.clone(),
                    lost: false,
                    fail: None,
                    opened: o.clone(),
                }))
            }),
            opened,
            Box::new(Says("never")),
            Duration::from_secs(20),
        );
        s.ctl.start_capture().unwrap();
        // Right after the press: listening, but the device is not open, so not capturing.
        let n = s.orch.snapshot();
        assert_eq!((n.phase, n.mic), (AssistantPhase::Listening, MicState::Off));
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(s.orch.snapshot().mic, MicState::Off, "still opening");
        // Then it is open, and only then capturing: the cue to speak.
        wait_for("capturing", || s.orch.snapshot().mic == MicState::Capturing);
        assert_eq!(s.orch.snapshot().phase, AssistantPhase::Listening);
        s.ctl.cancel();
    }

    #[test]
    fn a_mute_that_lands_with_the_key_release_still_discards_what_was_heard() {
        // Speech with no quiet after it; the key release is already flagged.
        let (tx, rx) = mpsc::channel::<Msg>();
        let mut a = tone(300, 0.002);
        a.extend(tone(900, 0.3));
        tx.send(Msg::Audio(a)).unwrap();
        let stop = AtomicBool::new(true);
        let cancel = CancellationToken::new();
        // Not muted at the top of the loop, muted a moment later: the order that went wrong on a
        // real laptop (mute, then the window's release, inside one pass of the loop).
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let muted = || calls.fetch_add(1, Ordering::SeqCst) >= 1;
        let mut ep = Endpointer::new(EndpointConfig::default());
        let out = collect(
            &rx,
            &mut ep,
            &stop,
            &cancel,
            &muted,
            Duration::from_secs(5),
            &mut || {},
        );
        assert!(matches!(out, Outcome::Cancelled));
        // Without a mute the same input is used, as a release should.
        let (tx, rx) = mpsc::channel::<Msg>();
        let mut a = tone(300, 0.002);
        a.extend(tone(900, 0.3));
        tx.send(Msg::Audio(a)).unwrap();
        let mut ep = Endpointer::new(EndpointConfig::default());
        let out = collect(
            &rx,
            &mut ep,
            &stop,
            &cancel,
            &|| false,
            Duration::from_secs(5),
            &mut || {},
        );
        assert!(matches!(out, Outcome::Audio(_)));
    }
}
