//! Standby listening (M3, D-29 step 3): keeps the microphone open while the wake word is on, runs
//! the detector on what it hears, and hands the stream to a voice turn when the phrase is said.
//!
//! What it does not do: it never keeps or writes audio (the detector sees each chunk once; the
//! capture's two-second ring stays in RAM), it does not listen while the microphone is muted, and
//! it does not listen while JARVIS is busy with a request (thinking, acting or speaking), so its
//! own voice cannot start another turn. The orchestrator stays the only state machine: this module
//! only reports `standby` through [`Orchestrator::set_standby`] and starts turns through the same
//! voice controller as push to talk.

use super::oww::{OwwDetector, WakeError};
use super::trigger::Trigger;
use crate::audio::capture::{Backend, Capture, CaptureStatus};
use crate::ipc::types::{JarvisError, MicState, Snapshot};
use crate::orchestrator::Orchestrator;
use crate::voice::controller::Msg;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Audio kept from just before the detector fired (16 kHz samples, about 160 ms): the detector
/// needs two high scores in a row, so a person who goes on talking at once has already started.
const LEAD_SAMPLES: usize = 2_560;
/// How often the thread looks at the settings and the microphone when no audio arrives.
const POLL: Duration = Duration::from_millis(250);
/// After a detector that failed to load, wait this long before trying again.
const LOAD_RETRY: Duration = Duration::from_secs(30);

/// The detector, as the engine uses it (so tests need no models).
pub trait Detector: Send {
    fn reset(&mut self);
    fn feed(&mut self, samples: &[f32]) -> Result<Vec<f32>, WakeError>;
}

impl Detector for OwwDetector {
    fn reset(&mut self) {
        OwwDetector::reset(self);
    }
    fn feed(&mut self, samples: &[f32]) -> Result<Vec<f32>, WakeError> {
        OwwDetector::feed(self, samples)
    }
}

/// Starts a voice turn from audio that is already flowing.
pub trait Handover: Send + Sync {
    fn start(&self, lead: Vec<f32>, rx: Receiver<Msg>) -> Result<(), JarvisError>;
}

pub struct Deps {
    pub orch: Arc<Orchestrator>,
    pub handover: Arc<dyn Handover>,
    pub backend: Box<dyn Fn() -> Box<dyn Backend> + Send + Sync>,
    pub device: Box<dyn Fn() -> Option<String> + Send + Sync>,
    /// Whether the person switched the wake word on and everything it needs is installed.
    pub wanted: Box<dyn Fn() -> bool + Send + Sync>,
    /// Builds the detector. Called on the standby thread, again after a failure.
    pub load: Box<dyn Fn() -> Result<Box<dyn Detector>, WakeError> + Send + Sync>,
    /// Sends a changed snapshot to the window.
    pub publish: Box<dyn Fn(Snapshot) + Send + Sync>,
    pub retry: Duration,
}

pub struct Standby {
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Standby {
    pub fn spawn(deps: Deps) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("jarvis-wake".into())
            .spawn(move || {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Engine::new(deps).run(&flag);
                }));
                if r.is_err() {
                    tracing::error!("the wake word thread panicked; it is off until restart");
                }
            })
            .map_err(|e| tracing::error!(error = %e, "could not start the wake word thread"))
            .ok();
        Self {
            stop,
            thread: Mutex::new(thread),
        }
    }

    /// Stops listening and waits (briefly) for the thread.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
        let handle = self.thread.lock().ok().and_then(|mut g| g.take());
        if let Some(h) = handle {
            let end = Instant::now() + Duration::from_secs(2);
            while !h.is_finished() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

impl Drop for Standby {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// A running microphone and where its audio goes.
struct Listening {
    capture: Capture,
    rx: Receiver<Msg>,
    /// Set while a voice turn has the audio: everything is passed on until the turn drops its end.
    forward: Option<Sender<Msg>>,
    /// The microphone is open and delivering (reported as `standby`).
    open: bool,
    /// The detector holds only audio heard while JARVIS was free; false after a turn or while
    /// busy, and then it is reset before it hears anything again.
    clean: bool,
}

struct Engine {
    d: Deps,
    detector: Option<Box<dyn Detector>>,
    trigger: Trigger,
    next_load: Instant,
    listening: Option<Listening>,
}

impl Engine {
    fn new(d: Deps) -> Self {
        Self {
            d,
            detector: None,
            trigger: Trigger::default(),
            next_load: Instant::now(),
            listening: None,
        }
    }

    fn run(mut self, stop: &AtomicBool) {
        while !stop.load(Ordering::Acquire) {
            self.supervise();
            match self.listening.as_ref().map(|l| l.rx.recv_timeout(POLL)) {
                Some(Ok(msg)) => self.on_msg(msg),
                Some(Err(RecvTimeoutError::Timeout)) => {}
                Some(Err(RecvTimeoutError::Disconnected)) => self.close(),
                None => std::thread::sleep(POLL),
            }
        }
        self.close();
    }

    /// Opens or closes the microphone to match what the person asked for.
    fn supervise(&mut self) {
        let muted = self.d.orch.snapshot().mic == MicState::Muted;
        let want = !muted && (self.d.wanted)();
        match (want, self.listening.is_some()) {
            (false, true) => self.close(),
            (true, false) => self.open(),
            _ => {}
        }
    }

    fn open(&mut self) {
        if self.detector.is_none() {
            if Instant::now() < self.next_load {
                return;
            }
            match (self.d.load)() {
                Ok(d) => self.detector = Some(d),
                Err(e) => {
                    tracing::warn!(error = %e, "wake word: could not load the detector");
                    self.next_load = Instant::now() + LOAD_RETRY;
                    return;
                }
            }
        }
        let (tx, rx) = mpsc::channel::<Msg>();
        let tx2 = tx.clone();
        let capture = Capture::start(
            (self.d.backend)(),
            (self.d.device)(),
            self.d.retry,
            move |c| {
                let _ = tx.send(Msg::Audio(c.to_vec()));
            },
            move |s| {
                let _ = tx2.send(Msg::Status(s.clone()));
            },
        );
        self.reset_detector();
        self.listening = Some(Listening {
            capture,
            rx,
            forward: None,
            open: false,
            clean: true,
        });
    }

    fn close(&mut self) {
        if let Some(l) = self.listening.take() {
            l.capture.stop();
            if l.open {
                self.set_standby(false);
            }
        }
    }

    fn set_standby(&self, on: bool) {
        let before = self.d.orch.snapshot().seq;
        let snap = self.d.orch.set_standby(on);
        if snap.seq != before {
            (self.d.publish)(snap);
        }
    }

    fn reset_detector(&mut self) {
        if let Some(d) = self.detector.as_mut() {
            d.reset();
        }
        self.trigger.reset();
    }

    fn on_msg(&mut self, msg: Msg) {
        // A turn that has the audio gets all of it, statuses included.
        let msg = match self.listening.as_mut() {
            Some(l) => match &l.forward {
                Some(f) => match f.send(msg) {
                    Ok(()) => return,
                    Err(mpsc::SendError(back)) => {
                        // The turn is over (it dropped its end).
                        l.forward = None;
                        l.clean = false;
                        back
                    }
                },
                None => msg,
            },
            None => return,
        };
        match msg {
            Msg::Status(CaptureStatus::Running { .. }) => {
                if let Some(l) = self.listening.as_mut() {
                    l.open = true;
                }
                self.set_standby(true);
            }
            Msg::Status(CaptureStatus::Unavailable(_)) | Msg::Status(CaptureStatus::Stopped) => {
                if let Some(l) = self.listening.as_mut()
                    && l.open
                {
                    l.open = false;
                    self.set_standby(false);
                }
            }
            Msg::Status(_) => {}
            Msg::Audio(chunk) => self.on_audio(chunk),
        }
    }

    fn on_audio(&mut self, chunk: Vec<f32>) {
        // Busy with a request: not listening for the phrase at all (the audio is dropped here and
        // the detector forgets, so what was heard before cannot add up to a trigger later).
        if self.d.orch.snapshot().active_request_id.is_some() {
            if self.listening.as_ref().is_some_and(|l| l.clean) {
                self.reset_detector();
                if let Some(l) = self.listening.as_mut() {
                    l.clean = false;
                }
            }
            return;
        }
        if let Some(l) = self.listening.as_mut() {
            if !l.clean {
                self.trigger.reset();
                if let Some(d) = self.detector.as_mut() {
                    d.reset();
                }
            }
            l.clean = true;
        }
        let Some(detector) = self.detector.as_mut() else {
            return;
        };
        let scores = match detector.feed(&chunk) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "wake word: the detector failed; stopping it");
                self.detector = None;
                self.next_load = Instant::now() + LOAD_RETRY;
                self.close();
                return;
            }
        };
        if scores.into_iter().any(|s| self.trigger.push(s)) {
            self.fire();
        }
    }

    fn fire(&mut self) {
        let Some(l) = self.listening.as_mut() else {
            return;
        };
        let ring = l.capture.pre_roll();
        let lead = ring[ring.len().saturating_sub(LEAD_SAMPLES)..].to_vec();
        let (tx, rx) = mpsc::channel::<Msg>();
        tracing::info!("wake word: heard the phrase");
        match self.d.handover.start(lead, rx) {
            Ok(()) => l.forward = Some(tx),
            Err(e) => {
                tracing::info!(message = %e.message, "wake word: the phrase was heard but no turn started");
                l.clean = false;
            }
        }
    }
}

/// The production hand-over: the voice controller, which needs the Tokio runtime to be entered.
pub struct VoiceHandover {
    pub voice: Arc<crate::voice::controller::VoiceController>,
    pub rt: tokio::runtime::Handle,
}

impl Handover for VoiceHandover {
    fn start(&self, lead: Vec<f32>, rx: Receiver<Msg>) -> Result<(), JarvisError> {
        let _in_runtime = self.rt.enter();
        self.voice.start_from_wake(lead, rx)
    }
}

#[cfg(test)]
mod tests;
