//! The pure parts of the Spotify sign-in: PKCE, the authorize address, and reading the
//! redirect. No network and no I/O here, so all of it is tested on every system.
//! Authorization Code with PKCE (RFC 7636): there is no client secret anywhere.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::Url;
use sha2::{Digest, Sha256};

/// What JARVIS asks Spotify for: control playback, see what is playing, and list the person's
/// playlists (their names and addresses, to find "my workout playlist"). Nothing that reads the
/// email address, the saved songs, the listening history, or what is inside a playlist.
pub const SCOPES: &str = "user-modify-playback-state user-read-playback-state playlist-read-private playlist-read-collaborative";

/// A one-time pair for one sign-in attempt.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

fn random_b64(bytes: usize) -> Result<String, getrandom::Error> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf)?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// The S256 challenge for a verifier.
pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

impl Pkce {
    pub fn new() -> Result<Self, getrandom::Error> {
        // 64 random bytes give an 86 character verifier (RFC 7636 allows 43 to 128).
        let verifier = random_b64(64)?;
        Ok(Self {
            challenge: challenge_for(&verifier),
            verifier,
            state: random_b64(24)?,
        })
    }
}

/// The address the person's browser opens. Built with a URL encoder: nothing is pasted in.
pub fn authorize_url(
    base: &str,
    client_id: &str,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
) -> Option<Url> {
    let mut url = Url::parse(base).ok()?;
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("code_challenge_method", "S256")
        .append_pair("code_challenge", challenge)
        .append_pair("state", state)
        .append_pair("scope", SCOPES);
    Some(url)
}

/// What the redirect to our loopback address carried.
#[derive(Debug, PartialEq, Eq)]
pub enum Callback {
    Code(String),
    /// The person pressed "Cancel" on Spotify's page, or Spotify refused.
    Denied,
    /// Not our redirect (a favicon request, a probe, a wrong state): keep waiting.
    Ignore,
}

/// Reads the first line of the HTTP request the browser sent, for example
/// `GET /callback?code=abc&state=xyz HTTP/1.1`. The state must match, or it is ignored:
/// a page that did not start this sign-in cannot finish it.
pub fn parse_callback(request_line: &str, expected_state: &str) -> Callback {
    let mut parts = request_line.split_whitespace();
    if parts.next() != Some("GET") {
        return Callback::Ignore;
    }
    let Some(target) = parts.next() else {
        return Callback::Ignore;
    };
    let Ok(url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Callback::Ignore;
    };
    if url.path() != "/callback" {
        return Callback::Ignore;
    }
    let (mut code, mut state, mut error) = (None, None, false);
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            "error" => error = true,
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        return Callback::Ignore;
    }
    if error {
        return Callback::Denied;
    }
    match code {
        Some(c) if !c.is_empty() && c.len() <= 2048 => Callback::Code(c),
        _ => Callback::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_matches_the_rfc_7636_example() {
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn a_fresh_pair_is_long_url_safe_and_never_repeats() {
        let a = Pkce::new().unwrap();
        let b = Pkce::new().unwrap();
        assert!(
            (43..=128).contains(&a.verifier.len()),
            "{}",
            a.verifier.len()
        );
        for s in [&a.verifier, &a.challenge, &a.state] {
            assert!(
                s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            );
        }
        assert_eq!(a.challenge, challenge_for(&a.verifier));
        assert_ne!(a.verifier, b.verifier);
        assert_ne!(a.state, b.state);
    }

    #[test]
    fn the_authorize_url_carries_exactly_what_it_should_and_is_encoded() {
        let u = authorize_url(
            "https://accounts.spotify.com/authorize",
            "0123456789abcdef0123456789abcdef",
            "http://127.0.0.1:51234/callback",
            "chal",
            "st",
        )
        .unwrap();
        assert_eq!(u.host_str(), Some("accounts.spotify.com"));
        let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:51234/callback");
        assert_eq!(q["scope"], SCOPES);
        assert_eq!(q.len(), 7);
        // The redirect address is percent-encoded in the raw string.
        assert!(
            u.as_str()
                .contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A51234%2Fcallback")
        );
        // Exactly these four; nothing that reads email, saved songs or listening history, and
        // nothing that can change a playlist.
        let mut asked: Vec<&str> = SCOPES.split(' ').collect();
        asked.sort_unstable();
        assert_eq!(
            asked,
            [
                "playlist-read-collaborative",
                "playlist-read-private",
                "user-modify-playback-state",
                "user-read-playback-state"
            ]
        );
        assert!(!SCOPES.contains("modify-private") && !SCOPES.contains("library"));
    }

    #[test]
    fn the_redirect_is_read_only_with_the_right_state() {
        let ok = "GET /callback?code=AQB123&state=abc HTTP/1.1";
        assert_eq!(parse_callback(ok, "abc"), Callback::Code("AQB123".into()));
        assert_eq!(parse_callback(ok, "other"), Callback::Ignore);
        assert_eq!(
            parse_callback(
                "GET /callback?error=access_denied&state=abc HTTP/1.1",
                "abc"
            ),
            Callback::Denied
        );
        // A refusal with the wrong state is not trusted either.
        assert_eq!(
            parse_callback(
                "GET /callback?error=access_denied&state=zzz HTTP/1.1",
                "abc"
            ),
            Callback::Ignore
        );
        for junk in [
            "",
            "GET /favicon.ico HTTP/1.1",
            "POST /callback?code=x&state=abc HTTP/1.1",
            "GET /callback?state=abc HTTP/1.1",
            "GET /callback?code=&state=abc HTTP/1.1",
            "GET //evil.example/callback?code=x&state=abc HTTP/1.1",
            "GET",
        ] {
            assert_eq!(parse_callback(junk, "abc"), Callback::Ignore, "{junk:?}");
        }
    }
}
