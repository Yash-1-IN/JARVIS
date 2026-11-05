use super::*;
use crate::audio::capture::{AudioError, Opened};
use crate::ipc::types::MicState;
use rtrb::RingBuffer;
use std::sync::atomic::AtomicU64;

type Feed = Arc<Mutex<Option<rtrb::Producer<f32>>>>;

struct Mic {
    script: Vec<f32>,
    opened: Arc<AtomicU64>,
    feed: Feed,
}
impl Backend for Mic {
    fn open(&mut self, _device: Option<&str>) -> Result<Opened, AudioError> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        let (mut p, ring) = RingBuffer::new(16_000 * 30);
        for s in &self.script {
            p.push(*s).unwrap();
        }
        *self.feed.lock().unwrap() = Some(p);
        Ok(Opened {
            guard: Box::new(()),
            rate: 16_000,
            device_name: "Fake mic".into(),
            ring,
            lost: Arc::new(AtomicBool::new(false)),
            overruns: Arc::new(AtomicU64::new(0)),
        })
    }
}

/// One score per 20 ms: 0.9 for loud sound, otherwise 0.
struct Loud {
    resets: Arc<AtomicU64>,
}
impl Detector for Loud {
    fn reset(&mut self) {
        self.resets.fetch_add(1, Ordering::SeqCst);
    }
    fn feed(&mut self, s: &[f32]) -> Result<Vec<f32>, WakeError> {
        Ok(s.chunks(320)
            .map(|c| {
                if c.iter().any(|v| v.abs() > 0.5) {
                    0.9
                } else {
                    0.0
                }
            })
            .collect())
    }
}

#[derive(Default)]
struct Calls {
    leads: Mutex<Vec<usize>>,
    rx: Mutex<Option<Receiver<Msg>>>,
}
impl Handover for Calls {
    fn start(&self, lead: Vec<f32>, rx: Receiver<Msg>) -> Result<(), JarvisError> {
        self.leads.lock().unwrap().push(lead.len());
        *self.rx.lock().unwrap() = Some(rx);
        Ok(())
    }
}

struct Rig {
    orch: Arc<Orchestrator>,
    calls: Arc<Calls>,
    opened: Arc<AtomicU64>,
    wanted: Arc<AtomicBool>,
    published: Arc<Mutex<Vec<MicState>>>,
    feed: Feed,
    standby: Standby,
}

fn phrase_script() -> Vec<f32> {
    let mut a = vec![0.01; 3_200];
    a.extend(std::iter::repeat_n(0.8, 6_400));
    a.extend(vec![0.01; 6_400]);
    a
}

fn rig(orch: Arc<Orchestrator>, wanted: bool, script: Vec<f32>) -> Rig {
    let calls = Arc::new(Calls::default());
    let opened = Arc::new(AtomicU64::new(0));
    let wanted = Arc::new(AtomicBool::new(wanted));
    let published = Arc::new(Mutex::new(Vec::new()));
    let feed: Feed = Arc::default();
    let f2 = feed.clone();
    let (o2, w2, p2) = (opened.clone(), wanted.clone(), published.clone());
    let standby = Standby::spawn(Deps {
        orch: orch.clone(),
        handover: calls.clone(),
        backend: Box::new(move || {
            Box::new(Mic {
                script: script.clone(),
                opened: o2.clone(),
                feed: f2.clone(),
            })
        }),
        device: Box::new(|| None),
        wanted: Box::new(move || w2.load(Ordering::SeqCst)),
        load: Box::new(|| {
            Ok(Box::new(Loud {
                resets: Arc::new(AtomicU64::new(0)),
            }) as Box<dyn Detector>)
        }),
        publish: Box::new(move |s| p2.lock().unwrap().push(s.mic)),
        retry: Duration::from_millis(50),
    });
    Rig {
        orch,
        calls,
        opened,
        wanted,
        published,
        feed,
        standby,
    }
}

fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !ok() {
        assert!(Instant::now() < end, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_phrase_starts_a_turn_with_the_audio_just_before_it() {
    let r = rig(Arc::new(Orchestrator::default()), true, phrase_script());
    wait_for("a hand-over", || !r.calls.leads.lock().unwrap().is_empty());
    let leads = r.calls.leads.lock().unwrap().clone();
    assert_eq!(leads.len(), 1, "one phrase, one turn");
    assert!(
        leads[0] > 0 && leads[0] <= LEAD_SAMPLES,
        "lead: {}",
        leads[0]
    );
    // The rest of the sound goes to the turn.
    let rx = r.calls.rx.lock().unwrap().take().unwrap();
    {
        let mut g = r.feed.lock().unwrap();
        for _ in 0..3_200 {
            g.as_mut().unwrap().push(0.02).unwrap();
        }
    }
    let got = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(got, Msg::Audio(_)));
    r.standby.shutdown();
}

#[test]
fn it_reports_standby_while_listening_and_off_when_switched_off() {
    let r = rig(Arc::new(Orchestrator::default()), true, vec![0.01; 3_200]);
    wait_for("standby", || r.orch.snapshot().mic == MicState::Standby);
    assert!(r.published.lock().unwrap().contains(&MicState::Standby));
    r.wanted.store(false, Ordering::SeqCst);
    wait_for("off", || r.orch.snapshot().mic == MicState::Off);
    r.standby.shutdown();
}

#[test]
fn nothing_is_opened_while_it_is_not_wanted() {
    let r = rig(Arc::new(Orchestrator::default()), false, phrase_script());
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(r.opened.load(Ordering::SeqCst), 0);
    assert_eq!(r.orch.snapshot().mic, MicState::Off);
    r.wanted.store(true, Ordering::SeqCst);
    wait_for("opened once switched on", || {
        r.opened.load(Ordering::SeqCst) == 1
    });
    r.standby.shutdown();
}

#[test]
fn a_muted_microphone_is_not_opened() {
    let orch = Arc::new(Orchestrator::default());
    orch.set_mute(true);
    let r = rig(orch, true, phrase_script());
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(r.opened.load(Ordering::SeqCst), 0);
    assert!(r.calls.leads.lock().unwrap().is_empty());
    r.standby.shutdown();
}

#[test]
fn muting_closes_the_microphone_and_unmuting_listens_again() {
    let r = rig(Arc::new(Orchestrator::default()), true, vec![0.01; 3_200]);
    wait_for("standby", || r.orch.snapshot().mic == MicState::Standby);
    r.orch.set_mute(true);
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(r.orch.snapshot().mic, MicState::Muted);
    r.orch.set_mute(false);
    wait_for("listening again", || {
        r.orch.snapshot().mic == MicState::Standby
    });
    assert_eq!(r.opened.load(Ordering::SeqCst), 2);
    r.standby.shutdown();
}

#[test]
fn while_a_request_is_running_the_phrase_is_ignored() {
    let orch = Arc::new(Orchestrator::default());
    let (_id, _) = orch.begin_turn().unwrap(); // thinking
    let r = rig(orch, true, phrase_script());
    wait_for("standby", || r.orch.snapshot().mic == MicState::Standby);
    std::thread::sleep(Duration::from_millis(600));
    assert!(
        r.calls.leads.lock().unwrap().is_empty(),
        "JARVIS must not hear itself"
    );
    r.standby.shutdown();
}

#[test]
fn a_detector_that_fails_to_load_leaves_the_microphone_closed() {
    let orch = Arc::new(Orchestrator::default());
    let opened = Arc::new(AtomicU64::new(0));
    let o2 = opened.clone();
    let s = Standby::spawn(Deps {
        orch: orch.clone(),
        handover: Arc::new(Calls::default()),
        backend: Box::new(move || {
            Box::new(Mic {
                script: vec![],
                opened: o2.clone(),
                feed: Feed::default(),
            })
        }),
        device: Box::new(|| None),
        wanted: Box::new(|| true),
        load: Box::new(|| Err(WakeError::Model("broken".into()))),
        publish: Box::new(|_| {}),
        retry: Duration::from_millis(50),
    });
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(opened.load(Ordering::SeqCst), 0);
    assert_eq!(orch.snapshot().mic, MicState::Off);
    s.shutdown();
}
