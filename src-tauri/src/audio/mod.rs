//! Microphone capture for the voice pipeline (M3 step 3, spec §6.2, `docs/m3-plan.md`).
//!
//! The rules, from CLAUDE.md: nothing allocates or locks inside the audio callback, audio stays in
//! RAM and is never written to disk, and every long operation can be cancelled. The callback only
//! writes mono samples into a lock-free ring; everything else (resampling to 16 kHz, the pre-roll,
//! losing and regaining the device) runs on a normal thread. The device itself sits behind the
//! [`Backend`] trait so all of that is tested here with a fake device, no microphone needed.

// Used by the voice pipeline in M3 step 4; the app only lists devices for now.
#[allow(dead_code)]
pub mod capture;
pub mod cpal_backend;
#[allow(dead_code)]
pub mod dsp;
