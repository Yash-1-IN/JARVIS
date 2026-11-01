//! Per-OS adapters (spec §4.1). Platform-specific code lives only here.

pub mod apps;
// Used by tools::files once the move/rename tools switch to it (#22 review).
#[allow(dead_code)]
pub mod fs;
pub mod keychain;
// Called by the apps.launch and system.status tools (Dev A's next M4 PR).
#[allow(dead_code)]
pub mod launch;
#[allow(dead_code)]
pub mod status;
