//! The voice pipeline's logic (M3 step 4, `docs/m3-plan.md`): finding where speech ends, turning
//! it into text, and driving the turn. Audio comes from `audio::capture`; the speech models plug
//! in behind [`transcribe::Transcriber`], so everything here is tested with fakes, and no model is
//! needed to build or run it. Nothing in this module writes audio anywhere.

// Not started by a command yet: the app wiring (the push-to-talk command and the channel the
// reply streams on) is the next step, which needs a small contract addition.
#[allow(dead_code)]
pub mod controller;
#[allow(dead_code)]
pub mod endpoint;
#[allow(dead_code)]
pub mod transcribe;
