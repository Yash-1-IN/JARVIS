//! The Spotify client: browser sign-in on a loopback address, the refresh token in the OS
//! keychain, and "search, then play" on the person's own Spotify device. Every request has a
//! timeout and every wait obeys a `CancellationToken`. Response bodies are never logged.

use super::pkce::{self, Callback, Pkce};
use crate::platform::keychain::{ApiKeyVault, SecretString};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(180);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
/// All of "search, play, check" together; the `music.play` tool allows a little longer.
pub const PLAY_DEADLINE: Duration = Duration::from_secs(20);
/// How many times (one second apart) to look at the player to see a song start.
const NORMAL_POLLS: u32 = 4;
/// The same right after the Spotify app was started, which is slower to begin.
const FRESH_POLLS: u32 = 8;
/// Least time worth spending on the final "play" after the Spotify app was started.
const MIN_PLAY_AFTER_START: Duration = Duration::from_secs(4);
/// The search alone.
const FIND_DEADLINE: Duration = Duration::from_secs(10);
/// Longest search words we send. A spoken request is short.
const MAX_QUERY_CHARS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotifyError {
    /// No Client ID saved.
    NotConfigured,
    NotConnected,
    Offline,
    /// The browser sign-in was not finished in time.
    TimedOut,
    /// The person pressed Cancel on Spotify's page.
    Denied,
    Cancelled,
    /// Spotify answered that this needs Premium (or the app is not allowed).
    Forbidden,
    /// No Spotify app or device is open to play on.
    NoDevice,
    /// The search found nothing.
    NoResults,
    /// Another program is using the sign-in's loopback port.
    PortBusy,
    /// Spotify accepted the song but nothing started playing.
    NotPlaying,
    /// Anything else. Details are logged without the response body.
    Failed,
}

/// The track chosen for a spoken request. The words come from Spotify: untrusted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Played {
    pub title: String,
    pub artist: String,
    /// The Spotify device that is playing it, when Spotify says.
    pub device: Option<String>,
}

/// What a search chose: the address and the words to show. Words are untrusted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub uri: String,
    pub title: String,
    pub artist: String,
    /// The track's album (`spotify:album:` and 22 letters or digits), used to play the track inside
    /// its album when playing the track alone does not start.
    pub album_uri: Option<String>,
}

#[derive(Clone)]
pub struct Endpoints {
    pub accounts: String,
    pub api: String,
    /// The loopback port of the redirect address. Spotify's dashboard only accepts a
    /// redirect address with a port, and the request must match it exactly, so the app uses
    /// one fixed port. 0 (tests only) lets the system choose.
    pub redirect_port: u16,
    /// How long to wait before asking Spotify whether the song really started (0 in tests).
    pub settle_ms: u64,
}

/// The redirect address the person registers in their Spotify app.
pub const REDIRECT_PORT: u16 = 43597;

impl Endpoints {
    pub fn real() -> Self {
        Self {
            accounts: "https://accounts.spotify.com".into(),
            api: "https://api.spotify.com".into(),
            redirect_port: REDIRECT_PORT,
            settle_ms: 1000,
        }
    }
}

/// Opens an address in the person's browser. Platform code supplies the real one.
pub type OpenUrl = Arc<dyn Fn(&str) -> bool + Send + Sync>;

struct Access {
    token: SecretString,
    expires: Instant,
}

pub struct Spotify {
    http: reqwest::Client,
    ep: Endpoints,
    vault: Arc<ApiKeyVault>,
    client_id: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    open: OpenUrl,
    access: tokio::sync::Mutex<Option<Access>>,
    signing_in: Mutex<Option<CancellationToken>>,
    name: Mutex<Option<String>>,
    /// The steps of the last "play" and the HTTP status each got (no bodies, no tokens), so a
    /// failure can say what Spotify answered.
    trace: Mutex<Vec<String>>,
    /// Bumped by every Disconnect. A sign-in or refresh that started before it must not store its
    /// token afterwards, or a Disconnect pressed while one was in flight would be undone.
    epoch: AtomicU64,
    /// Held while a token is written or cleared, so the epoch check and the write are one step.
    store_gate: Mutex<()>,
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: String,
    expires_in: u64,
    refresh_token: Option<String>,
}

enum Stored {
    Yes,
    /// Disconnect was pressed since this sign-in began.
    Disconnected,
    Failed,
}

#[derive(Deserialize)]
struct Me {
    display_name: Option<String>,
}

/// `"get devices 200 ×3"` is `("get devices 200", 3)`; a line with no count is one occurrence.
fn split_repeat(s: &str) -> (&str, u32) {
    match s.rsplit_once(" ×") {
        Some((base, n)) => match n.parse::<u32>() {
            Ok(n) => (base, n),
            Err(_) => (s, 1),
        },
        None => (s, 1),
    }
}

fn clip(s: &str, n: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(n).collect()
}

/// The search words, cleaned: no control characters, not too long, not empty.
pub fn clean_query(q: &str) -> Option<String> {
    let q = clip(q, MAX_QUERY_CHARS);
    let q = q.trim();
    (!q.is_empty()).then(|| q.to_owned())
}

/// The searches to try, best first. "X by Y" is searched as a track and an artist, which
/// finds the right song far more often than plain words; the plain words come second.
pub fn search_queries(words: &str) -> Vec<String> {
    let plain = words.replace('"', "");
    let mut out = Vec::new();
    if let Some((track, artist)) = plain.rsplit_once(" by ") {
        let (track, artist) = (track.trim(), artist.trim());
        if !track.is_empty() && !artist.is_empty() {
            out.push(format!("track:\"{track}\" artist:\"{artist}\""));
        }
    }
    out.push(plain);
    out
}

/// `prefix` and then exactly 22 letters or digits, nothing else.
fn valid_id(uri: &str, prefix: &str) -> bool {
    uri.strip_prefix(prefix)
        .is_some_and(|id| id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric()))
}

fn form_body(pairs: &[(&str, &str)]) -> String {
    let mut u = Url::parse("http://x/").expect("static url");
    u.query_pairs_mut().extend_pairs(pairs.iter().copied());
    u.query().unwrap_or_default().to_owned()
}

fn net(e: &reqwest::Error) -> SpotifyError {
    tracing::warn!(
        timeout = e.is_timeout(),
        connect = e.is_connect(),
        "spotify request failed"
    );
    if e.is_timeout() || e.is_connect() {
        SpotifyError::Offline
    } else {
        SpotifyError::Failed
    }
}

impl Spotify {
    pub fn new(
        ep: Endpoints,
        vault: Arc<ApiKeyVault>,
        client_id: Arc<dyn Fn() -> Option<String> + Send + Sync>,
        open: OpenUrl,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("http client");
        Self {
            http,
            ep,
            vault,
            client_id,
            open,
            access: tokio::sync::Mutex::new(None),
            signing_in: Mutex::new(None),
            name: Mutex::new(None),
            trace: Mutex::new(Vec::new()),
            epoch: AtomicU64::new(0),
            store_gate: Mutex::new(()),
        }
    }

    pub fn note(&self, step: String) {
        let mut t = self.trace.lock().unwrap_or_else(|e| e.into_inner());
        // The same step again (a wait that polls) is counted, not repeated, so a long wait cannot
        // push the lines that matter out of the short trace.
        if let Some(last) = t.last_mut() {
            let (base, n) = split_repeat(last);
            if base == step {
                *last = format!("{base} ×{}", n + 1);
                return;
            }
        }
        if t.len() < 20 {
            t.push(step);
        }
    }

    /// Records what the player said, in place of the plain "get player 200" line the request itself
    /// just recorded, so a run of identical answers collapses to one line with a count.
    fn note_state(&self, step: String) {
        {
            let mut t = self.trace.lock().unwrap_or_else(|e| e.into_inner());
            if t.last().is_some_and(|l| l.starts_with("get player 200")) {
                t.pop();
            }
        }
        self.note(step);
    }

    /// What the last "play" did, for example `search 200 · play 204 · player 204`.
    pub fn last_trace(&self) -> String {
        self.trace
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .join(" · ")
    }

    pub fn connected(&self) -> bool {
        matches!(self.vault.get(), Ok(Some(_)))
    }

    pub fn display_name(&self) -> Option<String> {
        self.name.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Forgets the token, cancels a sign-in in progress, keeps the Client ID.
    pub fn disconnect(&self) {
        if let Some(c) = self
            .signing_in
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            c.cancel();
        }
        {
            let _gate = self.store_gate.lock().unwrap_or_else(|e| e.into_inner());
            self.epoch.fetch_add(1, Ordering::SeqCst);
            if self.vault.clear().is_err() {
                tracing::warn!("could not delete the Spotify token");
            }
        }
        *self.name.lock().unwrap_or_else(|e| e.into_inner()) = None;
        // A running refresh may still hold the lock; the next call finds the vault empty.
        if let Ok(mut a) = self.access.try_lock() {
            *a = None;
        }
    }

    /// The browser sign-in. Returns when it ends, times out or is cancelled.
    pub async fn connect(&self) -> Result<(), SpotifyError> {
        let client_id = (self.client_id)().ok_or(SpotifyError::NotConfigured)?;
        let cancel = CancellationToken::new();
        {
            let mut slot = self.signing_in.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(old) = slot.replace(cancel.clone()) {
                old.cancel(); // a second Connect replaces the first
            }
        }
        // A finished token stays in the slot: cancelling it later is harmless.
        self.sign_in(&client_id, &cancel).await
    }

    async fn sign_in(
        &self,
        client_id: &str,
        cancel: &CancellationToken,
    ) -> Result<(), SpotifyError> {
        let epoch = self.epoch.load(Ordering::SeqCst);
        let pkce = Pkce::new().map_err(|_| SpotifyError::Failed)?;
        // A second Connect cancels the first, whose listener may not be dropped yet: try twice.
        let mut bound = TcpListener::bind(("127.0.0.1", self.ep.redirect_port)).await;
        if bound.is_err() {
            tokio::time::sleep(Duration::from_millis(400)).await;
            bound = TcpListener::bind(("127.0.0.1", self.ep.redirect_port)).await;
        }
        let listener = bound.map_err(|e| {
            tracing::warn!(error = %e, port = self.ep.redirect_port, "cannot listen for the Spotify sign-in");
            SpotifyError::PortBusy
        })?;
        let port = listener
            .local_addr()
            .map_err(|_| SpotifyError::Failed)?
            .port();
        let redirect = format!("http://127.0.0.1:{port}/callback");
        let url = pkce::authorize_url(
            &format!("{}/authorize", self.ep.accounts),
            client_id,
            &redirect,
            &pkce.challenge,
            &pkce.state,
        )
        .ok_or(SpotifyError::Failed)?;
        if !(self.open)(url.as_str()) {
            return Err(SpotifyError::Failed);
        }
        let code = await_code(&listener, &pkce.state, cancel).await?;
        let reply = self
            .token_request(&form_body(&[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", &redirect),
                ("client_id", client_id),
                ("code_verifier", &pkce.verifier),
            ]))
            .await?;
        let refresh = reply.refresh_token.as_deref().ok_or(SpotifyError::Failed)?;
        let secret = SecretString::parse(refresh).map_err(|_| SpotifyError::Failed)?;
        // A Disconnect pressed while the code was being exchanged wins: nothing is stored.
        match self.store_refresh(epoch, &secret) {
            Stored::Yes => {}
            Stored::Disconnected => return Err(SpotifyError::Cancelled),
            Stored::Failed => return Err(SpotifyError::Failed),
        }
        self.keep_access(reply).await;
        // The name is only for the "Connected as ..." line; a failure here is not fatal.
        if let Ok(me) = self.get_json::<Me>("/v1/me", &[]).await {
            *self.name.lock().unwrap_or_else(|e| e.into_inner()) =
                me.display_name.map(|n| clip(&n, 60));
        }
        Ok(())
    }

    /// Saves a refresh token unless Disconnect was pressed since `epoch` was read.
    fn store_refresh(&self, epoch: u64, secret: &SecretString) -> Stored {
        let _gate = self.store_gate.lock().unwrap_or_else(|e| e.into_inner());
        if self.epoch.load(Ordering::SeqCst) != epoch {
            return Stored::Disconnected;
        }
        match self.vault.set(secret) {
            Ok(_) => Stored::Yes,
            Err(_) => Stored::Failed,
        }
    }

    /// The refresh token from the OS keychain, read on a blocking thread with a time limit.
    /// macOS can ask the person for permission the first time a rebuilt app reads an item;
    /// that question must not stall an async worker, and a dialog nobody answers must end.
    async fn read_refresh_token(&self) -> Result<SecretString, SpotifyError> {
        let vault = self.vault.clone();
        let read = tokio::task::spawn_blocking(move || vault.get());
        match tokio::time::timeout(Duration::from_secs(12), read).await {
            Ok(Ok(Ok(Some(t)))) => Ok(t),
            Ok(Ok(Ok(None))) => Err(SpotifyError::NotConnected),
            Ok(_) => {
                self.note("keychain read failed".into());
                Err(SpotifyError::Failed)
            }
            Err(_) => {
                self.note("keychain read timed out (is a macOS permission question open?)".into());
                Err(SpotifyError::Failed)
            }
        }
    }

    async fn token_request(&self, body: &str) -> Result<TokenReply, SpotifyError> {
        let r = self
            .http
            .post(format!("{}/api/token", self.ep.accounts))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body.to_owned())
            .send()
            .await
            .map_err(|e| net(&e))?;
        self.note(format!("token {}", r.status().as_u16()));
        if !r.status().is_success() {
            tracing::warn!(
                status = r.status().as_u16(),
                "spotify token request refused"
            );
            return Err(if r.status() == StatusCode::BAD_REQUEST {
                SpotifyError::NotConnected // a revoked or expired refresh token
            } else {
                SpotifyError::Failed
            });
        }
        r.json().await.map_err(|_| SpotifyError::Failed)
    }

    async fn keep_access(&self, reply: TokenReply) {
        if let Ok(token) = SecretString::parse(&reply.access_token) {
            let life = Duration::from_secs(reply.expires_in.saturating_sub(60));
            *self.access.lock().await = Some(Access {
                token,
                expires: Instant::now() + life,
            });
        }
    }

    /// A valid access token, refreshed from the keychain's refresh token when needed.
    async fn token(&self, force_refresh: bool) -> Result<SecretString, SpotifyError> {
        let mut slot = self.access.lock().await;
        if !force_refresh && let Some(a) = slot.as_ref().filter(|a| a.expires > Instant::now()) {
            return Ok(a.token.clone());
        }
        let client_id = (self.client_id)().ok_or(SpotifyError::NotConfigured)?;
        let epoch = self.epoch.load(Ordering::SeqCst);
        let refresh = self.read_refresh_token().await?;
        let reply = self
            .token_request(&form_body(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.expose()),
                ("client_id", &client_id),
            ]))
            .await?;
        // The refresh took a while; if Disconnect was pressed meanwhile, drop the result instead of
        // putting a token back.
        {
            let _gate = self.store_gate.lock().unwrap_or_else(|e| e.into_inner());
            if self.epoch.load(Ordering::SeqCst) != epoch {
                return Err(SpotifyError::NotConnected);
            }
            if let Some(new) = reply
                .refresh_token
                .as_deref()
                .and_then(|r| SecretString::parse(r).ok())
                && self.vault.set(&new).is_err()
            {
                // Spotify rotated the token but it could not be saved: the next refresh may fail.
                tracing::warn!("could not save the rotated Spotify refresh token");
            }
        }
        let token = SecretString::parse(&reply.access_token).map_err(|_| SpotifyError::Failed)?;
        let life = Duration::from_secs(reply.expires_in.saturating_sub(60));
        *slot = Some(Access {
            token: token.clone(),
            expires: Instant::now() + life,
        });
        Ok(token)
    }

    /// One authorised request; a 401 refreshes the token once and tries again.
    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<String>,
    ) -> Result<(StatusCode, String), SpotifyError> {
        for attempt in 0..2 {
            let token = self.token(attempt == 1).await?;
            let mut url =
                Url::parse(&format!("{}{path}", self.ep.api)).map_err(|_| SpotifyError::Failed)?;
            if !query.is_empty() {
                url.query_pairs_mut().extend_pairs(query.iter().copied());
            }
            let mut req = self
                .http
                .request(method.clone(), url)
                .bearer_auth(token.expose());
            if let Some(b) = &body {
                req = req
                    .header("Content-Type", "application/json")
                    .body(b.clone());
            }
            let r = req.send().await.map_err(|e| net(&e))?;
            let status = r.status();
            if status == StatusCode::UNAUTHORIZED && attempt == 0 {
                continue;
            }
            self.note(format!(
                "{} {} {}",
                method.as_str().to_lowercase(),
                path.trim_start_matches("/v1/").replace("me/", ""),
                status.as_u16()
            ));
            let text = r.text().await.map_err(|_| SpotifyError::Failed)?;
            return Ok((status, text));
        }
        Err(SpotifyError::NotConnected)
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        q: &[(&str, &str)],
    ) -> Result<T, SpotifyError> {
        let (s, text) = self.call(reqwest::Method::GET, path, q, None).await?;
        if !s.is_success() {
            tracing::warn!(status = s.as_u16(), path, "spotify GET refused");
            return Err(map_status(s));
        }
        serde_json::from_str(&text).map_err(|_| SpotifyError::Failed)
    }

    /// "Play <words>": the top matching track, on the person's active (or first) device.
    #[cfg(test)]
    pub async fn play(
        &self,
        words: &str,
        cancel: &CancellationToken,
    ) -> Result<Played, SpotifyError> {
        let found = self.find(words, cancel).await?;
        self.play_found(&found, cancel).await
    }

    /// Searches for the words and remembers the best match. Starts a new trace.
    pub async fn find(
        &self,
        words: &str,
        cancel: &CancellationToken,
    ) -> Result<Found, SpotifyError> {
        self.trace.lock().unwrap_or_else(|e| e.into_inner()).clear();
        match tokio::time::timeout(FIND_DEADLINE, self.find_steps(words, cancel)).await {
            Ok(r) => r,
            Err(_) => {
                self.note(format!(
                    "search gave up after {} s",
                    FIND_DEADLINE.as_secs()
                ));
                Err(SpotifyError::Failed)
            }
        }
    }

    /// Plays a track found by `find` through Spotify's servers on the person's devices. Our
    /// own deadline is shorter than the tool's, so a slow Spotify ends here with the trace of
    /// what it answered, instead of the tool timing out with nothing to show.
    #[cfg(test)]
    pub async fn play_found(
        &self,
        found: &Found,
        cancel: &CancellationToken,
    ) -> Result<Played, SpotifyError> {
        self.play_found_within(found, cancel, PLAY_DEADLINE, NORMAL_POLLS)
            .await
    }

    async fn play_found_within(
        &self,
        found: &Found,
        cancel: &CancellationToken,
        deadline: Duration,
        polls: u32,
    ) -> Result<Played, SpotifyError> {
        match tokio::time::timeout(deadline, self.play_found_steps(found, cancel, polls)).await {
            Ok(r) => r,
            Err(_) => {
                self.note(format!("gave up after {} s", deadline.as_secs()));
                Err(SpotifyError::Failed)
            }
        }
    }

    /// Like `play_found`, but when Spotify lists no device to play on (the Spotify app on this
    /// computer is closed), it starts that app with `start_app`, waits for it to show up as a
    /// device, and asks again. All of it fits inside `budget`, so the tool's own time limit never
    /// cuts it off with nothing to show. `start_app` runs on a blocking thread and answers whether
    /// the app was started (it must refuse when the app is not installed).
    pub async fn play_found_or_start_app(
        &self,
        found: &Found,
        cancel: &CancellationToken,
        start_app: Arc<dyn Fn() -> bool + Send + Sync>,
        budget: Duration,
    ) -> Result<Played, SpotifyError> {
        let began = Instant::now();
        let first = self
            .play_found_within(found, cancel, budget.min(PLAY_DEADLINE), NORMAL_POLLS)
            .await;
        // Only "nothing to play on" is worth starting the app for. A device that took the song and
        // stayed silent is already running, a refusal and a sign-in problem are not about the app,
        // and no network will not be fixed by it.
        if first != Err(SpotifyError::NoDevice) {
            return first;
        }
        if cancel.is_cancelled() {
            return Err(SpotifyError::Cancelled);
        }
        let started = tokio::task::spawn_blocking(move || start_app())
            .await
            .unwrap_or(false);
        if !started {
            self.note("could not start the Spotify app".into());
            return first;
        }
        self.note("started the Spotify app".into());
        // The app needs a few seconds before Spotify lists it: wait, then look for it.
        let settle = Duration::from_millis(self.ep.settle_ms.saturating_mul(3));
        tokio::select! {
            _ = cancel.cancelled() => return Err(SpotifyError::Cancelled),
            _ = tokio::time::sleep(settle) => {}
        }
        let left = budget.saturating_sub(began.elapsed());
        let min_after = self.min_play_after_start();
        if left < min_after {
            self.note("no time left after starting the Spotify app".into());
            return first;
        }
        if !self
            .wait_for_device(left.saturating_sub(min_after), cancel)
            .await
        {
            if cancel.is_cancelled() {
                return Err(SpotifyError::Cancelled);
            }
            return Err(SpotifyError::NoDevice);
        }
        // A freshly started app registers as a device before it is ready to play: give it a few
        // seconds more, and watch the player for longer.
        let ready = Duration::from_millis(self.ep.settle_ms.saturating_mul(5));
        tokio::select! {
            _ = cancel.cancelled() => return Err(SpotifyError::Cancelled),
            _ = tokio::time::sleep(ready) => {}
        }
        let left = budget.saturating_sub(began.elapsed());
        self.play_found_within(found, cancel, left.max(min_after), FRESH_POLLS)
            .await
    }

    /// The least time worth giving the last "play" (a short one in tests, where nothing settles).
    fn min_play_after_start(&self) -> Duration {
        if self.ep.settle_ms == 0 {
            Duration::from_millis(300)
        } else {
            MIN_PLAY_AFTER_START
        }
    }

    /// Waits until Spotify lists a device we can play on, up to `within`. True if one showed up.
    async fn wait_for_device(&self, within: Duration, cancel: &CancellationToken) -> bool {
        let end = Instant::now() + within;
        let poll = Duration::from_millis(self.ep.settle_ms.max(10));
        loop {
            if cancel.is_cancelled() {
                return false;
            }
            if let Ok(devs) = self.get_json::<Devices>("/v1/me/player/devices", &[]).await
                && devs
                    .devices
                    .iter()
                    .any(|d| d.id.is_some() && !d.is_restricted)
            {
                self.note("a device appeared".into());
                return true;
            }
            if Instant::now() + poll >= end {
                self.note(format!("no device appeared within {} s", within.as_secs()));
                return false;
            }
            tokio::select! {
                _ = cancel.cancelled() => return false,
                _ = tokio::time::sleep(poll) => {}
            }
        }
    }

    async fn find_steps(
        &self,
        words: &str,
        cancel: &CancellationToken,
    ) -> Result<Found, SpotifyError> {
        let q = clean_query(words).ok_or(SpotifyError::NoResults)?;
        if cancel.is_cancelled() {
            return Err(SpotifyError::Cancelled);
        }
        let mut hit = None;
        for query in search_queries(&q) {
            let items = self.search_tracks(&query).await?;
            // The first result that can be played where this person is. The Spotify app clears
            // its player when it is asked for a track it cannot play (reported on Windows).
            if let Some(t) = items.into_iter().find(|t| t.is_playable != Some(false)) {
                hit = Some(t);
                break;
            }
        }
        let track = hit.ok_or(SpotifyError::NoResults)?;
        if !track.uri.starts_with("spotify:track:") || track.uri.len() > 64 {
            return Err(SpotifyError::Failed);
        }
        // A track address is public; recording it lets a failure be checked against what the
        // Spotify app shows for the same song.
        self.note(format!(
            "track {} playable={}",
            track.uri,
            match track.is_playable {
                Some(true) => "yes",
                Some(false) => "no",
                None => "unknown",
            }
        ));
        let album_uri = track
            .album
            .map(|a| a.uri)
            .filter(|u| valid_id(u, "spotify:album:"));
        let found = Found {
            album_uri,
            uri: track.uri,
            title: clip(&track.name, 120),
            artist: clip(
                track.artists.first().map(|a| a.name.as_str()).unwrap_or(""),
                120,
            ),
        };
        Ok(found)
    }

    /// Searches for tracks. (A search for the account's country, `market=from_token`, is refused
    /// with 403 for apps in Spotify's development mode, so the plain search is the one used.)
    async fn search_tracks(&self, query: &str) -> Result<Vec<Track>, SpotifyError> {
        let s: Search = self
            .get_json(
                "/v1/search",
                &[("q", query), ("type", "track"), ("limit", "5")],
            )
            .await?;
        Ok(s.tracks.items)
    }

    async fn play_found_steps(
        &self,
        found: &Found,
        cancel: &CancellationToken,
        polls: u32,
    ) -> Result<Played, SpotifyError> {
        if cancel.is_cancelled() {
            return Err(SpotifyError::Cancelled);
        }
        // The ways to ask, best first. Playing the track inside its album is how Spotify's own apps
        // start a song, and it is the one that works on Windows (the app clears its player when
        // asked for the track alone); the track alone is the fallback, and the only way when the
        // album is not known.
        let alone = serde_json::json!({ "uris": [found.uri] }).to_string();
        let (body, fallback) = match &found.album_uri {
            Some(album) => (
                serde_json::json!({
                    "context_uri": album,
                    "offset": { "uri": found.uri }
                })
                .to_string(),
                Some(alone),
            ),
            None => (alone, None),
        };
        let (mut s, _) = self
            .call(
                reqwest::Method::PUT,
                "/v1/me/player/play",
                &[],
                Some(body.clone()),
            )
            .await?;
        if s == StatusCode::NOT_FOUND {
            // No device is active. Wake the first one the account lists, if any.
            let devs: Devices = self.get_json("/v1/me/player/devices", &[]).await?;
            let id = devs
                .devices
                .into_iter()
                .find_map(|d| d.id)
                .ok_or(SpotifyError::NoDevice)?;
            (s, _) = self
                .call(
                    reqwest::Method::PUT,
                    "/v1/me/player/play",
                    &[("device_id", &id)],
                    Some(body.clone()),
                )
                .await?;
        }
        if !s.is_success() {
            tracing::warn!(status = s.as_u16(), "spotify play refused");
            return Err(if s == StatusCode::NOT_FOUND {
                SpotifyError::NoDevice
            } else {
                map_status(s)
            });
        }
        let device = match self.confirm_playing(cancel, polls).await {
            Err(SpotifyError::NotPlaying) => {
                // Spotify took the request but nothing started: the device may be asleep.
                // Move playback to it explicitly, then ask for the song again.
                match self.wake_device_and_retry(&body, cancel, polls).await {
                    // Still nothing: ask the other way.
                    Err(SpotifyError::NotPlaying) if fallback.is_some() => {
                        self.play_other_way(fallback.as_deref().unwrap_or_default(), cancel, polls)
                            .await?
                    }
                    other => other?,
                }
            }
            other => other?,
        };
        Ok(Played {
            title: found.title.clone(),
            artist: found.artist.clone(),
            device,
        })
    }

    /// Asks once more with `body` (the other way of asking) on the best device, and watches.
    async fn play_other_way(
        &self,
        body: &str,
        cancel: &CancellationToken,
        polls: u32,
    ) -> Result<Option<String>, SpotifyError> {
        let devs: Devices = self.get_json("/v1/me/player/devices", &[]).await?;
        let id = devs
            .devices
            .iter()
            .filter(|d| !d.is_restricted)
            .max_by_key(|d| d.is_active)
            .and_then(|d| d.id.clone())
            .ok_or(SpotifyError::NoDevice)?;
        self.note("trying the other way of asking".into());
        let (s, _) = self
            .call(
                reqwest::Method::PUT,
                "/v1/me/player/play",
                &[("device_id", &id)],
                Some(body.to_owned()),
            )
            .await?;
        if !s.is_success() {
            return Err(map_status(s));
        }
        self.confirm_playing(cancel, polls).await
    }

    async fn wake_device_and_retry(
        &self,
        body: &str,
        cancel: &CancellationToken,
        polls: u32,
    ) -> Result<Option<String>, SpotifyError> {
        let devs: Devices = self.get_json("/v1/me/player/devices", &[]).await?;
        self.note(format!(
            "devices {}",
            devs.devices
                .iter()
                .map(|d| format!(
                    "{}:{}{}",
                    clip(&d.kind, 12),
                    if d.is_active { "active" } else { "idle" },
                    if d.is_restricted { "+restricted" } else { "" }
                ))
                .collect::<Vec<_>>()
                .join(",")
        ));
        let id = devs
            .devices
            .iter()
            .filter(|d| !d.is_restricted)
            .max_by_key(|d| d.is_active)
            .and_then(|d| d.id.clone())
            .ok_or(SpotifyError::NoDevice)?;
        let transfer = serde_json::json!({ "device_ids": [id], "play": true }).to_string();
        let _ = self
            .call(reqwest::Method::PUT, "/v1/me/player", &[], Some(transfer))
            .await?;
        tokio::time::sleep(Duration::from_millis(self.ep.settle_ms.saturating_mul(2))).await;
        let (s, _) = self
            .call(
                reqwest::Method::PUT,
                "/v1/me/player/play",
                &[("device_id", &id)],
                Some(body.to_owned()),
            )
            .await?;
        if !s.is_success() {
            return Err(map_status(s));
        }
        self.confirm_playing(cancel, polls).await
    }

    /// Asks Spotify whether something is really playing, up to `polls` times, one second apart. A 204 or
    /// `is_playing: false` means no. Any other answer is "cannot tell", which is not a
    /// failure (the play request itself was accepted).
    async fn confirm_playing(
        &self,
        cancel: &CancellationToken,
        polls: u32,
    ) -> Result<Option<String>, SpotifyError> {
        let last = polls.saturating_sub(1);
        for attempt in 0..polls {
            if self.ep.settle_ms > 0 {
                tokio::select! {
                    _ = cancel.cancelled() => return Err(SpotifyError::Cancelled),
                    _ = tokio::time::sleep(Duration::from_millis(self.ep.settle_ms)) => {}
                }
            }
            let (s, text) = self
                .call(reqwest::Method::GET, "/v1/me/player", &[], None)
                .await?;
            if s == StatusCode::NO_CONTENT {
                if attempt == last {
                    return Err(SpotifyError::NotPlaying);
                }
                continue;
            }
            if !s.is_success() {
                return Ok(None);
            }
            let Ok(state) = serde_json::from_str::<PlayerState>(&text) else {
                return Ok(None);
            };
            // What the player says, as yes and no only: never the track, the device's name or any
            // text. Repeated identical lines collapse in the trace.
            self.note_state(format!(
                "player playing={} track={} progress={}s device={}",
                state.is_playing,
                state.item.is_some(),
                state.progress_ms.unwrap_or(0) / 1000,
                match &state.device {
                    Some(d) if d.is_active => "active",
                    Some(_) => "idle",
                    None => "none",
                }
            ));
            if state.is_playing {
                return Ok(state.device.map(|d| clip(&d.name, 60)));
            }
            if attempt == last {
                return Err(SpotifyError::NotPlaying);
            }
        }
        Err(SpotifyError::NotPlaying)
    }
}

fn map_status(s: StatusCode) -> SpotifyError {
    match s {
        StatusCode::UNAUTHORIZED => SpotifyError::NotConnected,
        StatusCode::FORBIDDEN => SpotifyError::Forbidden,
        StatusCode::NOT_FOUND => SpotifyError::NoDevice,
        _ => SpotifyError::Failed,
    }
}

#[derive(Deserialize)]
struct Search {
    tracks: Items,
}
#[derive(Deserialize)]
struct Items {
    items: Vec<Track>,
}
#[derive(Deserialize)]
struct Track {
    uri: String,
    name: String,
    artists: Vec<Artist>,
    /// False means Spotify says it cannot be played for this account.
    #[serde(default)]
    is_playable: Option<bool>,
    #[serde(default)]
    album: Option<Album>,
}
#[derive(Deserialize)]
struct Album {
    uri: String,
}
#[derive(Deserialize)]
struct Artist {
    name: String,
}
#[derive(Deserialize)]
struct PlayerState {
    is_playing: bool,
    device: Option<PlayerDevice>,
    /// Whether Spotify has a current track at all (the track itself is never read or logged).
    #[serde(default)]
    item: Option<serde::de::IgnoredAny>,
    #[serde(default)]
    progress_ms: Option<u64>,
}
#[derive(Deserialize)]
struct PlayerDevice {
    name: String,
    #[serde(default)]
    is_active: bool,
}
#[derive(Deserialize)]
struct Devices {
    devices: Vec<Device>,
}
#[derive(Deserialize)]
struct Device {
    id: Option<String>,
    /// "Computer", "Smartphone", "Speaker": what kind of device, never its name.
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    is_active: bool,
    #[serde(default)]
    is_restricted: bool,
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>JARVIS</title><body style=\"font-family:system-ui;text-align:center;margin-top:20vh\"><h1>You can close this tab.</h1><p>Return to JARVIS.</p>";

/// Waits for the browser's redirect on the loopback listener. Junk requests are answered and
/// skipped; only a request with the right state ends the wait.
async fn await_code(
    listener: &TcpListener,
    state: &str,
    cancel: &CancellationToken,
) -> Result<String, SpotifyError> {
    let deadline = tokio::time::sleep(SIGN_IN_TIMEOUT);
    tokio::pin!(deadline);
    loop {
        let (mut sock, _) = tokio::select! {
            _ = cancel.cancelled() => return Err(SpotifyError::Cancelled),
            _ = &mut deadline => return Err(SpotifyError::TimedOut),
            r = listener.accept() => r.map_err(|_| SpotifyError::Failed)?,
        };
        let mut buf = vec![0u8; 4096];
        let n = match tokio::time::timeout(Duration::from_secs(5), sock.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            _ => continue,
        };
        let head = String::from_utf8_lossy(&buf[..n]);
        let line = head.lines().next().unwrap_or("");
        let parsed = pkce::parse_callback(line, state);
        let (status, body) = match parsed {
            Callback::Ignore => ("404 Not Found", ""),
            _ => ("200 OK", DONE_PAGE),
        };
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
            body.len()
        );
        let _ = sock.write_all(reply.as_bytes()).await;
        let _ = sock.shutdown().await;
        match parsed {
            Callback::Code(c) => return Ok(c),
            Callback::Denied => return Err(SpotifyError::Denied),
            Callback::Ignore => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::types::KeyStorage;
    use crate::platform::keychain::SessionMemory;

    const ID: &str = "0123456789abcdef0123456789abcdef";

    #[derive(Clone, Debug)]
    struct Req {
        method: String,
        target: String,
        body: String,
        auth: bool,
    }
    type Answer = Arc<dyn Fn(&Req) -> (u16, String) + Send + Sync>;

    /// A tiny local HTTP server that records what it is asked and answers from a closure.
    async fn fake(answer: Answer) -> (String, Arc<Mutex<Vec<Req>>>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let log = Arc::new(Mutex::new(Vec::new()));
        let seen = log.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let (answer, seen) = (answer.clone(), seen.clone());
                tokio::spawn(async move {
                    let mut data = Vec::new();
                    let mut buf = [0u8; 4096];
                    let (head, mut body) = loop {
                        let n = s.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        data.extend_from_slice(&buf[..n]);
                        if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                            break (
                                String::from_utf8_lossy(&data[..i]).to_string(),
                                data[i + 4..].to_vec(),
                            );
                        }
                    };
                    let want: usize = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse().ok())
                        })
                        .unwrap_or(0);
                    while body.len() < want {
                        let n = s.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        body.extend_from_slice(&buf[..n]);
                    }
                    let mut first = head.lines().next().unwrap_or("").split_whitespace();
                    let req = Req {
                        method: first.next().unwrap_or("").into(),
                        target: first.next().unwrap_or("").into(),
                        body: String::from_utf8_lossy(&body).into(),
                        auth: head.to_ascii_lowercase().contains("authorization: bearer "),
                    };
                    seen.lock().unwrap().push(req.clone());
                    let (code, text) = answer(&req);
                    let reply = format!(
                        "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                        text.len()
                    );
                    let _ = s.write_all(reply.as_bytes()).await;
                    let _ = s.shutdown().await;
                });
            }
        });
        (base, log)
    }

    fn client(base: &str, open: OpenUrl) -> (Spotify, Arc<ApiKeyVault>) {
        let vault = Arc::new(ApiKeyVault::with_backend(
            Box::<SessionMemory>::default(),
            KeyStorage::SessionOnly,
        ));
        let ep = Endpoints {
            accounts: base.into(),
            api: base.into(),
            redirect_port: 0,
            settle_ms: 0,
        };
        let s = Spotify::new(ep, vault.clone(), Arc::new(|| Some(ID.to_string())), open);
        (s, vault)
    }

    fn no_browser() -> OpenUrl {
        Arc::new(|_| false)
    }

    fn tokens() -> String {
        r#"{"access_token":"acc-0123456789","expires_in":3600,"refresh_token":"ref-0123456789abcdef"}"#.into()
    }

    /// A "browser" that follows the authorize address to its redirect with the given code.
    fn browser(code: &'static str, wrong_state_first: bool) -> OpenUrl {
        Arc::new(move |url| {
            let u = Url::parse(url).unwrap();
            let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
            let redirect = q["redirect_uri"].clone();
            let state = q["state"].clone();
            tokio::spawn(async move {
                let http = reqwest::Client::new();
                if wrong_state_first {
                    let _ = http
                        .get(format!("{redirect}?code=evil&state=nope"))
                        .send()
                        .await;
                }
                let _ = http
                    .get(format!("{redirect}?code={code}&state={state}"))
                    .send()
                    .await;
            });
            true
        })
    }

    #[tokio::test]
    async fn signing_in_stores_only_the_refresh_token_and_sends_the_verifier() {
        let (base, log) = fake(Arc::new(|r| match (r.method.as_str(), r.target.as_str()) {
            ("POST", "/api/token") => (200, tokens()),
            ("GET", "/v1/me") => (200, r#"{"display_name":"Om"}"#.into()),
            _ => (404, "{}".into()),
        }))
        .await;
        let (sp, vault) = client(&base, browser("CODE123", true));
        assert!(!sp.connected());
        sp.connect().await.unwrap();
        assert!(sp.connected());
        assert_eq!(sp.display_name().as_deref(), Some("Om"));
        assert_eq!(
            vault.get().unwrap().unwrap().expose(),
            "ref-0123456789abcdef"
        );
        let reqs = log.lock().unwrap().clone();
        let tok = reqs.iter().find(|r| r.target == "/api/token").unwrap();
        // The forged redirect (wrong state) was ignored; only the real code was exchanged.
        assert!(
            tok.body.contains("code=CODE123") && !tok.body.contains("evil"),
            "{}",
            tok.body
        );
        assert!(tok.body.contains("grant_type=authorization_code"));
        assert!(tok.body.contains(&format!("client_id={ID}")));
        assert!(!tok.body.contains("client_secret"));
        let verifier = tok
            .body
            .split('&')
            .find_map(|p| p.strip_prefix("code_verifier="))
            .unwrap();
        assert!(verifier.len() >= 43, "PKCE verifier sent");
        assert!(reqs.iter().find(|r| r.target == "/v1/me").unwrap().auth);
    }

    #[tokio::test]
    async fn pressing_cancel_on_spotifys_page_is_a_clear_outcome() {
        let (base, _) = fake(Arc::new(|_| (404, "{}".into()))).await;
        let open: OpenUrl = Arc::new(|url| {
            let q: std::collections::HashMap<_, _> = Url::parse(url)
                .unwrap()
                .query_pairs()
                .into_owned()
                .collect();
            let (r, s) = (q["redirect_uri"].clone(), q["state"].clone());
            tokio::spawn(async move {
                let _ = reqwest::get(format!("{r}?error=access_denied&state={s}")).await;
            });
            true
        });
        let (sp, _) = client(&base, open);
        assert_eq!(sp.connect().await, Err(SpotifyError::Denied));
        assert!(!sp.connected());
    }

    #[tokio::test]
    async fn a_second_command_can_cancel_a_sign_in_that_is_waiting() {
        let (base, _) = fake(Arc::new(|_| (404, "{}".into()))).await;
        let (sp, _) = client(&base, Arc::new(|_| true)); // the browser never comes back
        let sp = Arc::new(sp);
        let waiting = tokio::spawn({
            let sp = sp.clone();
            async move { sp.connect().await }
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        sp.disconnect();
        assert_eq!(waiting.await.unwrap(), Err(SpotifyError::Cancelled));
    }

    #[tokio::test]
    async fn without_a_client_id_or_a_browser_it_says_why() {
        let (base, _) = fake(Arc::new(|_| (404, "{}".into()))).await;
        let (sp, _) = client(&base, no_browser());
        assert_eq!(sp.connect().await, Err(SpotifyError::Failed));
        let vault = Arc::new(ApiKeyVault::with_backend(
            Box::<SessionMemory>::default(),
            KeyStorage::SessionOnly,
        ));
        let none = Spotify::new(
            Endpoints {
                accounts: base.clone(),
                api: base,
                redirect_port: 0,
                settle_ms: 0,
            },
            vault,
            Arc::new(|| None),
            no_browser(),
        );
        assert_eq!(none.connect().await, Err(SpotifyError::NotConfigured));
    }

    const SEARCH_HIT: &str = r#"{"tracks":{"items":[{"uri":"spotify:track:4u7EnebtmKWzUH433cf5Qv","name":"Bohemian Rhapsody","artists":[{"name":"Queen"}]}]}}"#;

    async fn connected(answer: Answer) -> (Spotify, Arc<Mutex<Vec<Req>>>) {
        let (base, log) = fake(answer).await;
        let (sp, vault) = client(&base, no_browser());
        vault
            .set(&SecretString::parse("ref-0123456789abcdef").unwrap())
            .unwrap();
        (sp, log)
    }

    #[tokio::test]
    async fn play_searches_then_plays_the_top_track_with_only_its_uri() {
        let (sp, log) = connected(Arc::new(|r| {
            match (r.method.as_str(), r.target.split('?').next().unwrap()) {
                ("POST", "/api/token") => (
                    200,
                    r#"{"access_token":"acc-0123456789","expires_in":3600}"#.into(),
                ),
                ("GET", "/v1/search") => (200, SEARCH_HIT.into()),
                ("PUT", "/v1/me/player/play") => (204, String::new()),
                _ => (404, "{}".into()),
            }
        }))
        .await;
        let p = sp
            .play("bohemian rhapsody", &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            (p.title.as_str(), p.artist.as_str()),
            ("Bohemian Rhapsody", "Queen")
        );
        let reqs = log.lock().unwrap().clone();
        let search = reqs
            .iter()
            .find(|r| r.target.starts_with("/v1/search"))
            .unwrap();
        assert!(search.target.contains("q=bohemian+rhapsody") && search.target.contains("limit=5"));
        let play = reqs.iter().find(|r| r.method == "PUT").unwrap();
        assert_eq!(
            play.body,
            r#"{"uris":["spotify:track:4u7EnebtmKWzUH433cf5Qv"]}"#
        );
        assert!(play.auth && !play.target.contains("device_id"));
    }

    #[tokio::test]
    async fn with_no_active_device_it_wakes_the_first_one_and_with_none_it_says_so() {
        for devices in [
            r#"{"devices":[{"id":"dev123","name":"Mac"}]}"#,
            r#"{"devices":[]}"#,
        ] {
            let plays = Arc::new(Mutex::new(0));
            let p2 = plays.clone();
            let (sp, log) = connected(Arc::new(move |r| {
                match (r.method.as_str(), r.target.split('?').next().unwrap()) {
                    ("POST", "/api/token") => (
                        200,
                        r#"{"access_token":"a-0123456789","expires_in":3600}"#.into(),
                    ),
                    ("GET", "/v1/search") => (200, SEARCH_HIT.into()),
                    ("GET", "/v1/me/player/devices") => (200, devices.into()),
                    ("PUT", _) => {
                        let mut n = p2.lock().unwrap();
                        *n += 1;
                        if r.target.contains("device_id=dev123") {
                            (204, String::new())
                        } else {
                            (404, "{}".into())
                        }
                    }
                    _ => (404, "{}".into()),
                }
            }))
            .await;
            let out = sp.play("queen", &CancellationToken::new()).await;
            if devices.contains("dev123") {
                assert!(out.is_ok(), "{out:?}");
                assert!(
                    log.lock()
                        .unwrap()
                        .iter()
                        .any(|r| r.target.contains("device_id=dev123"))
                );
            } else {
                assert_eq!(out, Err(SpotifyError::NoDevice));
            }
        }
    }

    #[tokio::test]
    async fn a_401_refreshes_once_and_a_403_is_reported_as_forbidden() {
        let hits = Arc::new(Mutex::new(0));
        let h = hits.clone();
        let (sp, _) = connected(Arc::new(move |r| {
            match (r.method.as_str(), r.target.split('?').next().unwrap()) {
                ("POST", "/api/token") => (
                    200,
                    r#"{"access_token":"a-0123456789","expires_in":3600}"#.into(),
                ),
                ("GET", "/v1/search") => {
                    let mut n = h.lock().unwrap();
                    *n += 1;
                    if *n == 1 {
                        (401, "{}".into())
                    } else {
                        (200, SEARCH_HIT.into())
                    }
                }
                ("PUT", _) => (403, r#"{"error":{"reason":"PREMIUM_REQUIRED"}}"#.into()),
                _ => (404, "{}".into()),
            }
        }))
        .await;
        assert_eq!(
            sp.play("x", &CancellationToken::new()).await,
            Err(SpotifyError::Forbidden)
        );
        assert_eq!(*hits.lock().unwrap(), 2, "searched again after the refresh");
    }

    #[tokio::test]
    async fn no_results_not_connected_and_hostile_words_are_handled() {
        let (sp, log) = connected(Arc::new(|r| match r.target.split('?').next().unwrap() {
            "/api/token" => (
                200,
                r#"{"access_token":"a-0123456789","expires_in":3600}"#.into(),
            ),
            "/v1/search" => (200, r#"{"tracks":{"items":[]}}"#.into()),
            _ => (404, "{}".into()),
        }))
        .await;
        assert_eq!(
            sp.play("zzzz", &CancellationToken::new()).await,
            Err(SpotifyError::NoResults)
        );
        assert_eq!(
            sp.play("   \n ", &CancellationToken::new()).await,
            Err(SpotifyError::NoResults)
        );
        let c = CancellationToken::new();
        c.cancel();
        assert_eq!(sp.play("x", &c).await, Err(SpotifyError::Cancelled));
        // Control characters are dropped and the length is capped before anything is sent.
        let long = format!("a\u{0}b\n{}", "x".repeat(500));
        let q = clean_query(&long).unwrap();
        assert!(q.chars().count() <= MAX_QUERY_CHARS && !q.contains('\n') && !q.contains('\u{0}'));
        let _ = log;
        let (base, _) = fake(Arc::new(|_| (404, "{}".into()))).await;
        let (idle, _) = client(&base, no_browser());
        assert_eq!(
            idle.play("x", &CancellationToken::new()).await,
            Err(SpotifyError::NotConnected)
        );
    }

    #[test]
    fn the_real_redirect_port_is_the_one_the_docs_and_the_screen_tell_people_to_register() {
        assert_eq!(Endpoints::real().redirect_port, 43597);
    }

    #[tokio::test]
    async fn a_busy_port_is_its_own_error() {
        let held = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = held.local_addr().unwrap().port();
        let (base, _) = fake(Arc::new(|_| (404, "{}".into()))).await;
        let (mut sp, _) = client(&base, Arc::new(|_| true));
        sp.ep.redirect_port = port;
        assert_eq!(sp.connect().await, Err(SpotifyError::PortBusy));
    }

    #[tokio::test]
    async fn disconnect_forgets_the_token_but_not_the_client_id() {
        let (sp, _) = connected(Arc::new(|_| (404, "{}".into()))).await;
        assert!(sp.connected());
        sp.disconnect();
        assert!(!sp.connected());
        sp.disconnect(); // safe twice
    }

    #[test]
    fn x_by_y_is_searched_as_track_and_artist_then_as_plain_words() {
        assert_eq!(
            search_queries("i like that by bazzi"),
            [
                "track:\"i like that\" artist:\"bazzi\"",
                "i like that by bazzi"
            ]
        );
        // The last " by " splits, so a title containing "by" still works.
        assert_eq!(
            search_queries("stand by me by ben e king")[0],
            "track:\"stand by me\" artist:\"ben e king\""
        );
        assert_eq!(search_queries("bohemian rhapsody"), ["bohemian rhapsody"]);
        // Quotes in the words cannot break out of the filter.
        let q = search_queries("a\" artist:\"evil by b");
        // The words' own quotes are removed, so only the filter's four remain and the
        // injected text stays inside the track value.
        assert_eq!(q[0].matches('"').count(), 4, "{q:?}");
        assert!(q[0].starts_with("track:\"a artist:evil\""), "{q:?}");
    }

    fn playing_answer(state: &'static str, searches: Arc<Mutex<Vec<String>>>) -> Answer {
        Arc::new(
            move |r| {
                match (r.method.as_str(), r.target.split('?').next().unwrap()) {
                ("POST", "/api/token") => (
                    200,
                    r#"{"access_token":"a-0123456789","expires_in":3600}"#.into(),
                ),
                ("GET", "/v1/search") => {
                    searches.lock().unwrap().push(r.target.clone());
                    if r.target.contains("track%3A") {
                        (200, r#"{"tracks":{"items":[]}}"#.into())
                    } else {
                        (200, SEARCH_HIT.into())
                    }
                }
                ("PUT", "/v1/me/player/play" | "/v1/me/player") => (204, String::new()),
                ("GET", "/v1/me/player/devices") => (
                    200,
                    r#"{"devices":[{"id":"d1","name":"Mac","is_active":false,"is_restricted":false}]}"#
                        .into(),
                ),
                ("GET", "/v1/me/player") => match state {
                    "playing" => (
                        200,
                        r#"{"is_playing":true,"device":{"name":"MacBook Air"}}"#.into(),
                    ),
                    "paused" => (
                        200,
                        r#"{"is_playing":false,"device":{"name":"MacBook Air"}}"#.into(),
                    ),
                    _ => (204, String::new()),
                },
                _ => (404, "{}".into()),
            }
            },
        )
    }

    #[tokio::test]
    async fn it_checks_that_the_song_really_started_and_names_the_device() {
        let searches = Arc::new(Mutex::new(vec![]));
        let (sp, _) = connected(playing_answer("playing", searches.clone())).await;
        let p = sp
            .play("queen by queen", &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(p.device.as_deref(), Some("MacBook Air"));
        // The track-and-artist search found nothing, so the plain words were tried next.
        assert_eq!(searches.lock().unwrap().len(), 2);
        for state in ["paused", "none"] {
            let (sp, _) = connected(playing_answer(state, Arc::new(Mutex::new(vec![])))).await;
            assert_eq!(
                sp.play("queen", &CancellationToken::new()).await,
                Err(SpotifyError::NotPlaying),
                "{state}"
            );
            // The trace says what Spotify answered, including the explicit device transfer.
            let t = sp.last_trace();
            assert!(
                t.contains("search 200") && t.contains("player/play 204"),
                "{t}"
            );
            assert!(t.contains(":idle") && t.contains("player 204"), "{t}");
            assert!(
                !t.contains("acc-") && !t.contains("ref-"),
                "no token in the trace"
            );
        }
    }

    struct Locked;
    impl crate::platform::keychain::SecretBackend for Locked {
        fn get(&self) -> Result<Option<SecretString>, crate::platform::keychain::BackendError> {
            Err(crate::platform::keychain::BackendError::Other(
                "denied".into(),
            ))
        }
        fn set(&self, _: &SecretString) -> Result<(), crate::platform::keychain::BackendError> {
            Ok(())
        }
        fn delete(&self) -> Result<(), crate::platform::keychain::BackendError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_keychain_that_refuses_is_traced_not_blamed_on_spotify() {
        let (base, _) = fake(Arc::new(|_| (404, "{}".into()))).await;
        let vault = Arc::new(ApiKeyVault::with_backend(
            Box::new(Locked),
            KeyStorage::Keychain,
        ));
        let sp = Spotify::new(
            Endpoints {
                accounts: base.clone(),
                api: base,
                redirect_port: 0,
                settle_ms: 0,
            },
            vault,
            Arc::new(|| Some(ID.to_string())),
            no_browser(),
        );
        assert_eq!(
            sp.play("queen", &CancellationToken::new()).await,
            Err(SpotifyError::Failed)
        );
        assert!(
            sp.last_trace().contains("keychain read failed"),
            "{}",
            sp.last_trace()
        );
    }

    // ---- Windows: start the Spotify app when nothing is open ----

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// A Spotify account whose app is closed until `started` is set, then lists one device.
    fn closed_app(started: Arc<AtomicBool>, shows_up: bool) -> Answer {
        Arc::new(move |r| {
            let open = started.load(Ordering::SeqCst) && shows_up;
            match (r.method.as_str(), r.target.split('?').next().unwrap()) {
                ("POST", "/api/token") => (
                    200,
                    r#"{"access_token":"a-0123456789","expires_in":3600}"#.into(),
                ),
                ("GET", "/v1/search") => (200, SEARCH_HIT.into()),
                ("GET", "/v1/me/player/devices") if open => {
                    (200, r#"{"devices":[{"id":"pc1","name":"My PC"}]}"#.into())
                }
                ("GET", "/v1/me/player/devices") => (200, r#"{"devices":[]}"#.into()),
                ("PUT", _) if open && r.target.contains("device_id=pc1") => (204, String::new()),
                ("PUT", _) => (404, "{}".into()),
                ("GET", "/v1/me/player") if open => (
                    200,
                    r#"{"is_playing":true,"device":{"name":"My PC"}}"#.into(),
                ),
                _ => (404, "{}".into()),
            }
        })
    }

    fn starter(
        flag: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
        ok: bool,
    ) -> Arc<dyn Fn() -> bool + Send + Sync> {
        Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            if ok {
                flag.store(true, Ordering::SeqCst);
            }
            ok
        })
    }

    fn found() -> Found {
        Found {
            uri: "spotify:track:4u7EnebtmKWzUH433cf5Qv".into(),
            title: "Bohemian Rhapsody".into(),
            artist: "Queen".into(),
            album_uri: Some("spotify:album:6i6folBtxKV28WX3msQ4FE".into()),
        }
    }

    #[tokio::test]
    async fn with_no_device_it_starts_the_app_waits_for_it_and_plays() {
        let started = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let (sp, log) = connected(closed_app(started.clone(), true)).await;
        let p = sp
            .play_found_or_start_app(
                &found(),
                &CancellationToken::new(),
                starter(started, calls.clone(), true),
                Duration::from_secs(8),
            )
            .await
            .unwrap();
        assert_eq!(
            (p.title.as_str(), p.device.as_deref()),
            ("Bohemian Rhapsody", Some("My PC"))
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "the app was started once");
        // It played on the device that appeared, with only the track's address.
        let plays: Vec<_> = log
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.method == "PUT" && r.target.contains("device_id=pc1"))
            .cloned()
            .collect();
        assert!(!plays.is_empty());
        assert!(
            plays[0]
                .body
                .contains("spotify:track:4u7EnebtmKWzUH433cf5Qv")
        );
        assert!(sp.last_trace().contains("started the Spotify app"));
        assert!(sp.last_trace().contains("a device appeared"));
    }

    #[tokio::test]
    async fn if_the_app_cannot_be_started_it_says_there_is_no_device_and_asks_nothing_more() {
        let started = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let (sp, log) = connected(closed_app(started.clone(), true)).await;
        let out = sp
            .play_found_or_start_app(
                &found(),
                &CancellationToken::new(),
                starter(started, calls.clone(), false),
                Duration::from_secs(8),
            )
            .await;
        assert_eq!(out, Err(SpotifyError::NoDevice));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(sp.last_trace().contains("could not start the Spotify app"));
        // No waiting loop: the device list was asked for by the first attempt only.
        let lists = log
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.target.starts_with("/v1/me/player/devices"))
            .count();
        assert!(lists <= 1, "{lists}");
    }

    #[tokio::test]
    async fn an_app_that_starts_but_never_shows_up_gives_up_inside_the_budget() {
        let started = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let (sp, _) = connected(closed_app(started.clone(), false)).await;
        let t = Instant::now();
        let out = sp
            .play_found_or_start_app(
                &found(),
                &CancellationToken::new(),
                starter(started, calls, true),
                Duration::from_millis(1500),
            )
            .await;
        assert_eq!(out, Err(SpotifyError::NoDevice));
        assert!(
            t.elapsed() < Duration::from_millis(2500),
            "{:?}",
            t.elapsed()
        );
        assert!(sp.last_trace().contains("no device appeared"));
    }

    #[tokio::test]
    async fn a_cancel_while_waiting_for_the_app_stops_at_once() {
        let started = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let (sp, _) = connected(closed_app(started.clone(), false)).await;
        let cancel = CancellationToken::new();
        let c = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            c.cancel();
        });
        let t = Instant::now();
        let out = sp
            .play_found_or_start_app(
                &found(),
                &cancel,
                starter(started, calls, true),
                Duration::from_secs(20),
            )
            .await;
        assert_eq!(out, Err(SpotifyError::Cancelled));
        assert!(t.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn problems_that_are_not_about_a_missing_app_never_start_it() {
        // Premium needed (403), and not signed in (401 and no refresh): the app is left alone.
        for (code, want) in [(403u16, SpotifyError::Forbidden)] {
            let started = Arc::new(AtomicBool::new(false));
            let calls = Arc::new(AtomicUsize::new(0));
            let (sp, _) = connected(Arc::new(move |r| {
                match (r.method.as_str(), r.target.split('?').next().unwrap()) {
                    ("POST", "/api/token") => (
                        200,
                        r#"{"access_token":"a-0123456789","expires_in":3600}"#.into(),
                    ),
                    ("PUT", _) => (code, "{}".into()),
                    _ => (404, "{}".into()),
                }
            }))
            .await;
            let out = sp
                .play_found_or_start_app(
                    &found(),
                    &CancellationToken::new(),
                    starter(started, calls.clone(), true),
                    Duration::from_secs(8),
                )
                .await;
            assert_eq!(out, Err(want));
            assert_eq!(
                calls.load(Ordering::SeqCst),
                0,
                "the app must not be started"
            );
        }
    }

    #[tokio::test]
    async fn when_a_device_is_already_there_the_app_is_not_started() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let (sp, _) = connected(Arc::new(|r| {
            match (r.method.as_str(), r.target.split('?').next().unwrap()) {
                ("POST", "/api/token") => (
                    200,
                    r#"{"access_token":"a-0123456789","expires_in":3600}"#.into(),
                ),
                ("PUT", "/v1/me/player/play") => (204, String::new()),
                ("GET", "/v1/me/player") => (
                    200,
                    r#"{"is_playing":true,"device":{"name":"My PC"}}"#.into(),
                ),
                _ => (404, "{}".into()),
            }
        }))
        .await;
        let p = sp
            .play_found_or_start_app(
                &found(),
                &CancellationToken::new(),
                Arc::new(move || {
                    c.fetch_add(1, Ordering::SeqCst);
                    true
                }),
                Duration::from_secs(8),
            )
            .await
            .unwrap();
        assert_eq!(p.device.as_deref(), Some("My PC"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_long_wait_collapses_its_polls_so_the_summary_line_survives() {
        let (sp, _) = connected(Arc::new(|_| (404, "{}".into()))).await;
        for _ in 0..50 {
            sp.note("get player/devices 200".into());
        }
        sp.note("no device appeared within 12 s".into());
        let t = sp.last_trace();
        assert!(t.contains("get player/devices 200 ×50"), "{t}");
        assert!(t.ends_with("no device appeared within 12 s"), "{t}");
        // Different steps are still kept in order, up to the cap.
        sp.note("a".into());
        sp.note("a".into());
        assert!(sp.last_trace().ends_with("a ×2"));
    }

    #[tokio::test]
    async fn a_device_that_takes_the_song_and_stays_silent_does_not_get_the_app_started() {
        // Spotify says 204 to every play, a device is listed, but the player never plays.
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let (sp, _) = connected(Arc::new(|r| {
            match (r.method.as_str(), r.target.split('?').next().unwrap()) {
                ("POST", "/api/token") => (
                    200,
                    r#"{"access_token":"a-0123456789","expires_in":3600}"#.into(),
                ),
                ("PUT", _) => (204, String::new()),
                ("GET", "/v1/me/player/devices") => (
                    200,
                    r#"{"devices":[{"id":"pc1","type":"Computer","is_active":true}]}"#.into(),
                ),
                ("GET", "/v1/me/player") => (
                    200,
                    r#"{"is_playing":false,"device":{"name":"PC","is_active":true},"progress_ms":0}"#
                        .into(),
                ),
                _ => (404, "{}".into()),
            }
        }))
        .await;
        let out = sp
            .play_found_or_start_app(
                &found(),
                &CancellationToken::new(),
                Arc::new(move || {
                    c.fetch_add(1, Ordering::SeqCst);
                    true
                }),
                Duration::from_secs(8),
            )
            .await;
        assert_eq!(out, Err(SpotifyError::NotPlaying));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a running app is not started again"
        );
        // The trace says what the player reported, in yes and no only, and what kind of device.
        let t = sp.last_trace();
        assert!(
            t.contains("player playing=false track=false progress=0s device=active"),
            "{t}"
        );
        assert!(t.contains("Computer:active"), "{t}");
        assert!(
            !t.contains("PC"),
            "a device's name never goes into the trace: {t}"
        );
    }

    /// A Disconnect pressed while a token refresh is in flight must stay a Disconnect: the refresh
    /// that was already on its way must not put the (possibly rotated) token back in the keychain.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_disconnect_during_a_refresh_is_not_undone_by_it() {
        let (base, _) = fake(Arc::new(|r| {
            if r.target.starts_with("/api/token") {
                // The refresh is slow, and Spotify rotates the refresh token.
                std::thread::sleep(Duration::from_millis(400));
                return (
                    200,
                    r#"{"access_token":"acc-new-0123456789","expires_in":3600,"refresh_token":"ref-rotated-0123456789"}"#.into(),
                );
            }
            (404, "{}".into())
        }))
        .await;
        let (sp, vault) = client(&base, no_browser());
        vault
            .set(&SecretString::parse("ref-0123456789abcdef").unwrap())
            .unwrap();
        let sp = Arc::new(sp);
        let refreshing = {
            let sp = sp.clone();
            tokio::spawn(async move { sp.token(true).await.is_ok() })
        };
        tokio::time::sleep(Duration::from_millis(150)).await; // the refresh is now in flight
        sp.disconnect();
        assert!(
            vault.get().unwrap().is_none(),
            "disconnect clears the keychain"
        );
        let _ = refreshing.await;
        assert!(
            vault.get().unwrap().is_none(),
            "the in-flight refresh put the token back after Disconnect"
        );
        assert!(!sp.connected());
    }

    #[tokio::test]
    async fn the_search_skips_a_track_that_cannot_be_played_and_keeps_the_album() {
        let hits = r#"{"tracks":{"items":[
            {"uri":"spotify:track:AAAAAAAAAAAAAAAAAAAAAA","name":"Blocked","artists":[{"name":"X"}],"is_playable":false},
            {"uri":"spotify:track:4u7EnebtmKWzUH433cf5Qv","name":"Thunderstruck","artists":[{"name":"AC/DC"}],"is_playable":true,
             "album":{"uri":"spotify:album:6i6folBtxKV28WX3msQ4FE"}},
            {"uri":"spotify:track:BBBBBBBBBBBBBBBBBBBBBB","name":"Other","artists":[],"album":{"uri":"not an album address"}}]}}"#;
        let (sp, _) = connected(Arc::new(move |r| {
            match (r.method.as_str(), r.target.split('?').next().unwrap()) {
                ("POST", "/api/token") => (
                    200,
                    r#"{"access_token":"a-0123456789","expires_in":3600}"#.into(),
                ),
                ("GET", "/v1/search") => (200, hits.into()),
                _ => (404, "{}".into()),
            }
        }))
        .await;
        let f = sp
            .find("thunderstruck", &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(f.uri, "spotify:track:4u7EnebtmKWzUH433cf5Qv");
        assert_eq!(
            f.album_uri.as_deref(),
            Some("spotify:album:6i6folBtxKV28WX3msQ4FE")
        );
        assert!(
            sp.last_trace()
                .contains("track spotify:track:4u7EnebtmKWzUH433cf5Qv playable=yes")
        );
    }

    #[test]
    fn only_well_formed_album_addresses_are_kept() {
        assert!(valid_id(
            "spotify:album:6i6folBtxKV28WX3msQ4FE",
            "spotify:album:"
        ));
        for bad in [
            "",
            "spotify:album:",
            "spotify:album:6i6folBtxKV28WX3msQ4F",
            "spotify:track:6i6folBtxKV28WX3msQ4FE",
            "spotify:album:6i6folBtxKV28WX3msQ4F\"}",
            "https://open.spotify.com/album/6i6folBtxKV28WX3msQ4FE",
        ] {
            assert!(!valid_id(bad, "spotify:album:"), "{bad:?}");
        }
    }

    /// A Spotify app that plays only when asked the given way (`needle` is in the request body).
    fn picky(needle: &'static str, plays: Arc<Mutex<Vec<String>>>) -> Answer {
        let started = Arc::new(AtomicBool::new(false));
        Arc::new(move |r| {
            match (r.method.as_str(), r.target.split('?').next().unwrap()) {
                ("POST", "/api/token") => (
                    200,
                    r#"{"access_token":"a-0123456789","expires_in":3600}"#.into(),
                ),
                ("GET", "/v1/me/player/devices") => (
                    200,
                    r#"{"devices":[{"id":"pc1","type":"Computer","is_active":true}]}"#.into(),
                ),
                ("PUT", "/v1/me/player/play") => {
                    plays.lock().unwrap().push(r.body.clone());
                    if r.body.contains(needle) {
                        started.store(true, Ordering::SeqCst);
                    }
                    (204, String::new())
                }
                ("PUT", _) => (204, String::new()),
                ("GET", "/v1/me/player") if started.load(Ordering::SeqCst) => (
                    200,
                    r#"{"is_playing":true,"device":{"name":"My PC","is_active":true},"progress_ms":900}"#.into(),
                ),
                ("GET", "/v1/me/player") => (
                    200,
                    r#"{"is_playing":false,"device":{"name":"My PC","is_active":true},"progress_ms":0}"#.into(),
                ),
                _ => (404, "{}".into()),
            }
        })
    }

    #[tokio::test]
    async fn the_track_is_played_inside_its_album_first_and_at_once() {
        let plays = Arc::new(Mutex::new(Vec::new()));
        let (sp, _) = connected(picky("context_uri", plays.clone())).await;
        let p = sp
            .play_found(&found(), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(p.device.as_deref(), Some("My PC"));
        let plays = plays.lock().unwrap().clone();
        assert_eq!(plays.len(), 1, "one request was enough: {plays:?}");
        assert!(plays[0].contains("spotify:album:6i6folBtxKV28WX3msQ4FE"));
        assert!(plays[0].contains(r#""offset":{"uri":"spotify:track:4u7EnebtmKWzUH433cf5Qv"}"#));
        assert!(!plays[0].contains("\"uris\""));
    }

    #[tokio::test]
    async fn if_the_album_way_never_starts_the_track_alone_is_tried() {
        let plays = Arc::new(Mutex::new(Vec::new()));
        let (sp, _) = connected(picky("\"uris\"", plays.clone())).await;
        let p = sp
            .play_found(&found(), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(p.device.as_deref(), Some("My PC"));
        let plays = plays.lock().unwrap().clone();
        assert!(plays.first().unwrap().contains("context_uri"));
        assert!(plays.last().unwrap().contains("\"uris\""));
        assert!(sp.last_trace().contains("trying the other way of asking"));
    }

    #[tokio::test]
    async fn without_an_album_address_only_the_track_alone_is_asked_for() {
        let (sp, log) = connected(Arc::new(|r| {
            match (r.method.as_str(), r.target.split('?').next().unwrap()) {
                ("POST", "/api/token") => (
                    200,
                    r#"{"access_token":"a-0123456789","expires_in":3600}"#.into(),
                ),
                ("GET", "/v1/me/player/devices") => (
                    200,
                    r#"{"devices":[{"id":"pc1","type":"Computer","is_active":true}]}"#.into(),
                ),
                ("PUT", _) => (204, String::new()),
                ("GET", "/v1/me/player") => (
                    200,
                    r#"{"is_playing":false,"device":{"name":"PC","is_active":true}}"#.into(),
                ),
                _ => (404, "{}".into()),
            }
        }))
        .await;
        let mut f = found();
        f.album_uri = None;
        assert_eq!(
            sp.play_found(&f, &CancellationToken::new()).await,
            Err(SpotifyError::NotPlaying)
        );
        assert!(
            !log.lock()
                .unwrap()
                .iter()
                .any(|r| r.body.contains("context_uri"))
        );
    }

    #[tokio::test]
    async fn only_unplayable_results_count_as_no_results() {
        let (sp, _) = connected(Arc::new(|r| {
            match (r.method.as_str(), r.target.split('?').next().unwrap()) {
                ("POST", "/api/token") => (
                    200,
                    r#"{"access_token":"a-0123456789","expires_in":3600}"#.into(),
                ),
                ("GET", "/v1/search") => (
                    200,
                    r#"{"tracks":{"items":[{"uri":"spotify:track:AAAAAAAAAAAAAAAAAAAAAA","name":"B","artists":[],"is_playable":false}]}}"#.into(),
                ),
                _ => (404, "{}".into()),
            }
        }))
        .await;
        assert_eq!(
            sp.find("anything", &CancellationToken::new()).await,
            Err(SpotifyError::NoResults)
        );
    }
}
