//! Per-OS adapters (spec §4.1). Platform-specific code lives only here.

pub mod apps;
// Used by tools::files once the move/rename tools switch to it (#22 review).
#[allow(dead_code)]
pub mod fs;
pub mod keychain;
