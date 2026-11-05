//! The wake word (M3, D-29): openWakeWord's pretrained "hey jarvis" through ONNX Runtime.
//!
//! `oww` and `trigger` are the detector: audio in, a score per 80 ms, and a decision when the
//! phrase was said. `standby` (step 3) keeps a microphone open while the wake word is on and hands
//! the audio to a voice turn. Nothing is downloaded here (the model manager does that, D-28). Nothing here records audio: it works on
//! the sound it is handed and keeps no more than about two seconds of derived numbers.
//!
//! The model is licensed CC BY-NC-SA 4.0, for personal non-commercial use by the owner and
//! Developer B only (D-23, D-29); it is never in the installer.

pub mod oww;
pub mod standby;
pub mod trigger;
