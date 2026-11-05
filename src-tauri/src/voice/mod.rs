//! The voice pipeline's logic (M3 step 4, `docs/m3-plan.md`): finding where speech ends, turning
//! it into text, and driving the turn. Audio comes from `audio::capture`; the speech model plugs
//! in behind [`transcribe::Transcriber`], so everything here is tested with fakes, and no model is
//! needed to build or run it. Nothing in this module writes audio anywhere.

pub mod controller;
pub mod endpoint;
pub mod models;
pub mod sherpa;
pub mod transcribe;
