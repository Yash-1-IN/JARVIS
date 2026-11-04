//! Spotify: sign-in (PKCE) and "play X" through the official Web API (issue #68, D-22).
//! Tokens live only here and in the OS keychain; nothing secret reaches the webview.

pub mod client;
pub mod pkce;
