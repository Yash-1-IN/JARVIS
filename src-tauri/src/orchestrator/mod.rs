// Turn methods are used by the turn runner added next in M2.
#![allow(dead_code)]

//! The single authoritative assistant state machine (spec §5, ADR-010).
//! Pure and synchronous: it owns the `Snapshot`, enforces legal transitions and
//! bumps `seq` on every change. Publishing to the webview happens in `ipc`.

use crate::ipc::types::{AssistantPhase, JarvisError, MicState, Snapshot};
use std::sync::{Mutex, MutexGuard};

#[derive(Debug, PartialEq, Eq)]
pub enum TransitionError {
    Illegal {
        from: AssistantPhase,
        to: AssistantPhase,
    },
    WrongRequest,
    NoActiveTurn,
}

/// True if `from -> to` is a legal phase change (spec §5.1 and the phase diagram).
pub fn is_legal(from: AssistantPhase, to: AssistantPhase) -> bool {
    use AssistantPhase::*;
    if from == to {
        return false;
    }
    match (from, to) {
        // Any phase may fail, and a cancel/interrupt returns to idle (or listening).
        (_, Error) | (_, Idle) => true,
        // A failed turn is over; the user can start the next one (retry) directly.
        (Error, Listening) | (Error, Thinking) | (Error, Transcribing) => true,
        (Idle, Listening) | (Idle, Thinking) | (Idle, Transcribing) => true,
        (Listening, Transcribing) | (Listening, Thinking) => true,
        (Transcribing, Thinking) => true,
        // Every tool call leaves thinking via acting (low risk) or awaiting_approval.
        (Thinking, Acting) | (Thinking, AwaitingApproval) | (Thinking, Speaking) => true,
        (AwaitingApproval, Acting) | (AwaitingApproval, Thinking) => true,
        // Every result returns to thinking so the model sees it.
        (Acting, Thinking) | (Acting, Speaking) => true,
        // Barge-in by wake word, and follow-up window after a reply.
        (Speaking, Listening) => true,
        _ => false,
    }
}

pub struct Orchestrator {
    inner: Mutex<Inner>,
}

struct Inner {
    snap: Snapshot,
    mic_before_mute: MicState,
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner {
                snap: Snapshot::initial(),
                mic_before_mute: MicState::Off,
            }),
        }
    }
}

impl Orchestrator {
    // A panic elsewhere must not take the whole app down: recover the data.
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> Snapshot {
        self.lock().snap.clone()
    }

    fn bump(inner: &mut Inner) -> Snapshot {
        inner.snap.seq += 1;
        inner.snap.clone()
    }

    /// Starts a user turn from idle (text input). Returns the new request id and snapshot.
    pub fn begin_turn(&self) -> Result<(String, Snapshot), TransitionError> {
        let mut g = self.lock();
        if !is_legal(g.snap.phase, AssistantPhase::Thinking) || g.snap.active_request_id.is_some() {
            return Err(TransitionError::Illegal {
                from: g.snap.phase,
                to: AssistantPhase::Thinking,
            });
        }
        let id = uuid::Uuid::now_v7().to_string();
        g.snap.phase = AssistantPhase::Thinking;
        g.snap.active_request_id = Some(id.clone());
        g.snap.last_error = None;
        Ok((id, Self::bump(&mut g)))
    }

    /// Moves the active turn to `to`. Entering idle ends the turn.
    pub fn transition(
        &self,
        request_id: &str,
        to: AssistantPhase,
    ) -> Result<Snapshot, TransitionError> {
        let mut g = self.lock();
        if g.snap.active_request_id.as_deref() != Some(request_id) {
            return Err(TransitionError::WrongRequest);
        }
        let from = g.snap.phase;
        if !is_legal(from, to) {
            return Err(TransitionError::Illegal { from, to });
        }
        g.snap.phase = to;
        if to == AssistantPhase::Idle {
            g.snap.active_request_id = None;
            g.snap.cloud_in_use = false;
        }
        Ok(Self::bump(&mut g))
    }

    /// `cancelled` is an event, not a state: the phase returns to idle.
    pub fn cancel(&self, request_id: &str) -> Result<Snapshot, TransitionError> {
        let mut g = self.lock();
        if g.snap.active_request_id.as_deref() != Some(request_id) {
            return Err(TransitionError::NoActiveTurn);
        }
        g.snap.phase = AssistantPhase::Idle;
        g.snap.active_request_id = None;
        g.snap.cloud_in_use = false;
        g.snap.pending_approvals.clear();
        Ok(Self::bump(&mut g))
    }

    pub fn fail(&self, request_id: &str, err: JarvisError) -> Result<Snapshot, TransitionError> {
        let mut g = self.lock();
        if g.snap.active_request_id.as_deref() != Some(request_id) {
            return Err(TransitionError::WrongRequest);
        }
        // The turn is over: stop the cloud indicator and free the slot, but keep
        // `last_error.request_id` so the UI can offer a retry.
        g.snap.phase = AssistantPhase::Error;
        g.snap.active_request_id = None;
        g.snap.cloud_in_use = false;
        g.snap.pending_approvals.clear();
        g.snap.last_error = Some(err);
        Ok(Self::bump(&mut g))
    }

    pub fn set_cloud_in_use(&self, on: bool) -> Snapshot {
        let mut g = self.lock();
        g.snap.cloud_in_use = on;
        Self::bump(&mut g)
    }

    /// Mute is a mic axis change and never touches the assistant phase.
    pub fn set_mute(&self, muted: bool) -> Snapshot {
        let mut g = self.lock();
        match (muted, g.snap.mic) {
            (true, MicState::Muted) | (true, MicState::Unavailable) => {}
            (true, current) => {
                g.mic_before_mute = current;
                g.snap.mic = MicState::Muted;
            }
            (false, MicState::Muted) => g.snap.mic = g.mic_before_mute,
            (false, _) => {}
        }
        Self::bump(&mut g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::types::ErrorCode;
    use AssistantPhase::*;

    #[test]
    fn happy_path_text_turn() {
        let o = Orchestrator::default();
        let (id, s) = o.begin_turn().unwrap();
        assert_eq!((s.phase, s.seq), (Thinking, 1));
        o.transition(&id, Acting).unwrap();
        o.transition(&id, Thinking).unwrap();
        o.transition(&id, Speaking).unwrap();
        let s = o.transition(&id, Idle).unwrap();
        assert_eq!(s.active_request_id, None);
        assert_eq!(s.seq, 5);
    }

    #[test]
    fn tool_must_leave_thinking_via_acting_or_approval() {
        assert!(is_legal(Thinking, Acting) && is_legal(Thinking, AwaitingApproval));
        assert!(!is_legal(Idle, Acting));
        assert!(!is_legal(Idle, AwaitingApproval));
        assert!(!is_legal(Speaking, Acting));
        assert!(!is_legal(AwaitingApproval, Speaking));
    }

    #[test]
    fn illegal_transition_is_rejected_and_state_unchanged() {
        let o = Orchestrator::default();
        let (id, _) = o.begin_turn().unwrap();
        let before = o.snapshot();
        assert_eq!(
            o.transition(&id, Listening),
            Err(TransitionError::Illegal {
                from: Thinking,
                to: Listening
            })
        );
        assert_eq!(o.snapshot(), before);
    }

    #[test]
    fn second_turn_rejected_while_one_is_active() {
        let o = Orchestrator::default();
        o.begin_turn().unwrap();
        assert!(o.begin_turn().is_err());
    }

    #[test]
    fn stale_request_id_is_rejected() {
        let o = Orchestrator::default();
        let (id, _) = o.begin_turn().unwrap();
        o.cancel(&id).unwrap();
        assert_eq!(
            o.transition(&id, Speaking),
            Err(TransitionError::WrongRequest)
        );
        assert_eq!(o.cancel(&id), Err(TransitionError::NoActiveTurn));
    }

    #[test]
    fn cancel_returns_to_idle_not_a_cancelled_state() {
        let o = Orchestrator::default();
        let (id, _) = o.begin_turn().unwrap();
        o.set_cloud_in_use(true);
        let s = o.cancel(&id).unwrap();
        assert_eq!(s.phase, Idle);
        assert!(!s.cloud_in_use && s.active_request_id.is_none());
    }

    #[test]
    fn mute_is_independent_of_phase_and_restores_previous_mic() {
        let o = Orchestrator::default();
        let (id, _) = o.begin_turn().unwrap();
        o.transition(&id, Speaking).unwrap();
        let s = o.set_mute(true);
        assert_eq!((s.mic, s.phase), (MicState::Muted, Speaking));
        let s = o.set_mute(false);
        assert_eq!((s.mic, s.phase), (MicState::Off, Speaking));
    }

    #[test]
    fn failure_ends_the_turn_and_a_new_turn_can_start() {
        let o = Orchestrator::default();
        let (id, _) = o.begin_turn().unwrap();
        o.set_cloud_in_use(true);
        let s = o
            .fail(
                &id,
                JarvisError {
                    code: ErrorCode::NetworkOffline,
                    message: "offline".into(),
                    retryable: true,
                    request_id: Some(id.clone()),
                },
            )
            .unwrap();
        assert_eq!(s.phase, Error);
        assert!(!s.cloud_in_use, "cloud indicator must go dark");
        assert_eq!(s.active_request_id, None);
        assert_eq!(
            s.last_error.unwrap().request_id.as_deref(),
            Some(id.as_str())
        );
        let (id2, s2) = o.begin_turn().expect("retry after error");
        assert_ne!(id, id2);
        assert_eq!((s2.phase, s2.last_error), (Thinking, None));
    }

    #[test]
    fn seq_strictly_increases() {
        let o = Orchestrator::default();
        let a = o.set_mute(true).seq;
        let b = o.set_mute(false).seq;
        assert!(b > a);
    }
}
