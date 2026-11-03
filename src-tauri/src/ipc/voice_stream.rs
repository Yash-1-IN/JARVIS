//! The channel a voice-started turn streams its reply on.
//!
//! A typed turn streams on the `Channel<TurnStreamEvent>` the window hands to `submit_text`. A voice
//! turn (push to talk, later the wake word) starts from Rust, with no such call to carry a channel,
//! so the window attaches one once with `voice_attach`, and voice turns stream there. This holder
//! keeps it, replaces it when the window reloads, and drops it when it closes.

use crate::ipc::types::TurnStreamEvent;
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::ipc::Channel;

/// Sends one event; `false` means the receiver is gone.
type SendFn = dyn Fn(TurnStreamEvent) -> bool + Send + Sync;

#[derive(Default)]
pub struct VoiceStream {
    slot: Mutex<Option<Arc<SendFn>>>,
}

impl VoiceStream {
    fn slot(&self) -> MutexGuard<'_, Option<Arc<SendFn>>> {
        self.slot.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Attaches a sender, replacing any earlier one (a reloaded window attaches again).
    pub fn attach(&self, send: Arc<SendFn>) {
        *self.slot() = Some(send);
    }

    /// Attaches the window's channel.
    pub fn attach_channel(&self, channel: Channel<TurnStreamEvent>) {
        self.attach(Arc::new(move |e| channel.send(e).is_ok()));
    }

    // The voice controller uses the rest from M3 step 4c; until then only `attach` is called.
    #[allow(dead_code)]
    pub fn is_attached(&self) -> bool {
        self.slot().is_some()
    }

    #[allow(dead_code)]
    pub fn detach(&self) {
        *self.slot() = None;
    }

    /// Sends an event to the window. Returns `false` when nothing is attached or the receiver has
    /// gone away (in which case it is detached). The send happens outside the lock, so a slow
    /// window cannot block an attach.
    #[allow(dead_code)]
    pub fn send(&self, event: TurnStreamEvent) -> bool {
        let Some(sender) = self.slot().clone() else {
            return false;
        };
        if sender(event) {
            return true;
        }
        let mut slot = self.slot();
        // Only drop it if it is still the one that failed: a new window may have attached since.
        if slot.as_ref().is_some_and(|s| Arc::ptr_eq(s, &sender)) {
            *slot = None;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::types::StopReason;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn text(s: &str) -> TurnStreamEvent {
        TurnStreamEvent::TextDelta { text: s.into() }
    }

    fn counting() -> (Arc<SendFn>, Arc<AtomicUsize>) {
        let n = Arc::new(AtomicUsize::new(0));
        let c = n.clone();
        (
            Arc::new(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
                true
            }),
            n,
        )
    }

    #[test]
    fn nothing_attached_means_events_have_nowhere_to_go_and_say_so() {
        let v = VoiceStream::default();
        assert!(!v.is_attached());
        assert!(!v.send(text("x")));
    }

    #[test]
    fn an_attached_window_gets_every_event_in_order() {
        let v = VoiceStream::default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        v.attach(Arc::new(move |e| {
            s.lock().unwrap().push(e);
            true
        }));
        assert!(v.is_attached());
        assert!(v.send(text("a")));
        assert!(v.send(TurnStreamEvent::Done {
            stop_reason: StopReason::End
        }));
        let got = seen.lock().unwrap();
        assert!(matches!(&got[0], TurnStreamEvent::TextDelta { text } if text == "a"));
        assert!(matches!(&got[1], TurnStreamEvent::Done { .. }));
    }

    #[test]
    fn attaching_again_replaces_the_old_window() {
        let v = VoiceStream::default();
        let (old, old_n) = counting();
        let (new, new_n) = counting();
        v.attach(old);
        v.send(text("1"));
        v.attach(new);
        v.send(text("2"));
        v.send(text("3"));
        assert_eq!(
            (old_n.load(Ordering::SeqCst), new_n.load(Ordering::SeqCst)),
            (1, 2)
        );
    }

    #[test]
    fn a_closed_window_is_dropped_and_a_later_attach_works() {
        let v = VoiceStream::default();
        v.attach(Arc::new(|_| false)); // the receiver is gone
        assert!(!v.send(text("x")));
        assert!(!v.is_attached(), "a dead channel is not kept");
        let (fresh, n) = counting();
        v.attach(fresh);
        assert!(v.send(text("y")));
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_failed_send_does_not_drop_a_window_that_attached_meanwhile() {
        let v = Arc::new(VoiceStream::default());
        let (fresh, n) = counting();
        let (v2, fresh2) = (v.clone(), fresh.clone());
        let attached = Arc::new(AtomicBool::new(false));
        let a = attached.clone();
        // The old sender fails, but while it was sending the window reloaded and attached anew.
        v.attach(Arc::new(move |_| {
            v2.attach(fresh2.clone());
            a.store(true, Ordering::SeqCst);
            false
        }));
        assert!(!v.send(text("x")));
        assert!(attached.load(Ordering::SeqCst));
        assert!(v.is_attached(), "the new window stays attached");
        assert!(v.send(text("z")));
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn detach_clears_it() {
        let v = VoiceStream::default();
        let (c, _) = counting();
        v.attach(c);
        v.detach();
        assert!(!v.is_attached());
    }
}
