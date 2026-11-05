//! Per-OS adapters (spec §4.1). Platform-specific code lives only here.

pub mod apps;
pub mod browser;
pub mod dialog;
// Opens the wake word's ONNX Runtime library the safe way on Windows (wake/oww.rs, D-29).
pub mod dll;
// Used by tools::files once the move/rename tools switch to it (#22 review).
#[allow(dead_code)]
pub mod fs;
pub mod keychain;
// Called by the apps.launch and system.status tools (Dev A's next M4 PR).
#[allow(dead_code)]
pub mod launch;
// Used by the music.control and music.now_playing tools through tools::adapters.
pub mod media;
#[allow(dead_code)]
pub mod status;
