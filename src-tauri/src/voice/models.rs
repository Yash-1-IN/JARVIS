//! The voice model manager (M3 step 2): downloads the speech models once, checks them, and says
//! whether they are ready.
//!
//! What is trusted, and what is not. The only thing trusted is the **manifest** that ships inside the
//! app (`resources/models.json`): the address of each archive, its size and its SHA-256, and the
//! name, size and SHA-256 of every file that may come out of it. Whatever the network returns is
//! untrusted until it matches those numbers:
//!
//! 1. The archive is downloaded to a `.part` file (resumable with a `Range` request, bounded in
//!    size, with a stall timeout) and its SHA-256 must equal the pinned one, or it is thrown away.
//! 2. Only then it is unpacked, and only into a fresh staging folder. Only regular files that the
//!    manifest lists are written (matched by name, or by their exact path inside the archive, for
//!    example `lib/onnxruntime.dll`); a listed file that is a link or anything but a plain file is
//!    refused; paths with `..`, absolute paths and drive letters are refused; every other entry
//!    (sample recordings, headers, links the archive happens to hold) is skipped, never written.
//!    File names on disk are built from the manifest, never from the archive. Output is
//!    size-bounded. Archives are `.tar.bz2`, `.tgz` or `.zip`; a model that is a single file needs
//!    no unpacking and is only checked.
//! 3. Each extracted file is checked again against its own pinned size and SHA-256.
//! 4. The staging folder replaces the install folder in one rename, and a marker is written.
//!
//! Nothing here records audio, and nothing but the manifest decides where a byte comes from.

use crate::ipc::types::ModelProgress;
use futures::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

/// The default set must stay under this many bytes (spec §10, `docs/m3-plan.md` step 2).
#[cfg(test)]
pub const DEFAULT_SET_LIMIT: u64 = 600 * 1024 * 1024;
/// One file coming out of an archive may not be larger than this, whatever the manifest says.
const MAX_FILE_BYTES: u64 = 400 * 1024 * 1024;
/// Everything coming out of one archive may not be larger than this (guards against a compression bomb).
const MAX_UNPACKED_BYTES: u64 = 700 * 1024 * 1024;
/// A download that sends nothing for this long is given up (the partial file is kept for a resume).
const STALL: Duration = Duration::from_secs(30);
/// One whole download may not take longer than this.
const DOWNLOAD_LIMIT: Duration = Duration::from_secs(60 * 60);
/// A progress event is sent at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);
/// Written into the install folder only after everything was verified.
const MARKER: &str = ".verified";

const MANIFEST: &str = include_str!("../../resources/models.json");

/// Where downloads may come from. Nothing else is ever fetched.
const ALLOWED_SOURCES: &[&str] = &[
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/",
    "https://github.com/microsoft/onnxruntime/releases/download/",
    "https://github.com/dscripka/openWakeWord/releases/download/",
];

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileSpec {
    /// The name on disk, under the model's folder.
    pub name: String,
    /// Where the file is inside the archive, below its top folder (`lib/onnxruntime.dll`). Without
    /// it the file is found by `name` at the top of the archive or one folder down.
    #[serde(default)]
    pub path: Option<String>,
    pub bytes: u64,
    pub sha256: String,
}

/// How what is downloaded is packed.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
pub enum Format {
    #[default]
    #[serde(rename = "tar.bz2")]
    TarBz2,
    #[serde(rename = "tgz")]
    Tgz,
    #[serde(rename = "zip")]
    Zip,
    /// The download is the one file itself.
    #[serde(rename = "file")]
    File,
}

/// The sets of downloads the app knows: what speech needs, and what the wake word needs.
pub const SET_SPEECH: &str = "speech";
pub const SET_WAKE: &str = "wake";
const PLATFORMS: &[&str] = &["windows-x86_64", "macos-aarch64", "linux-x86_64"];

/// The operating system and processor this build runs on, as the manifest spells them.
pub fn current_platform() -> &'static str {
    if cfg!(all(windows, target_arch = "x86_64")) {
        "windows-x86_64"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos-aarch64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "macos-x86_64"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux-x86_64"
    } else {
        "other"
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelSpec {
    pub id: String,
    pub name: String,
    pub purpose: String,
    pub licence: String,
    /// `speech` (downloaded for the speech engine) or `wake` (downloaded when the wake word is
    /// switched on).
    pub set: String,
    /// The system this download is for (`windows-x86_64`, `macos-aarch64`, `linux-x86_64`); without
    /// it, every system.
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub format: Format,
    pub url: String,
    pub archive_bytes: u64,
    pub archive_sha256: String,
    /// Where the files go, under the models folder, with `/` between the parts.
    pub dir: String,
    pub files: Vec<FileSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    version: u32,
    models: Vec<ModelSpec>,
}

/// Lower-case hex of a SHA-256 digest.
fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A bare file name: no separators, no dots-only, no empty, nothing that could climb or hide.
fn is_plain_name(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.len() <= 128
        && !s.starts_with('.')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

impl ModelSpec {
    /// Checks a spec the way the manifest test does. `allow_http` is for tests with a local server.
    fn validate(&self, allow_http: bool) -> Result<(), String> {
        let bad = |what: &str| Err(format!("model {}: {what}", self.id));
        if !is_plain_name(&self.id) {
            return bad("bad id");
        }
        // The only places a download may come from: release pages of the three projects whose
        // files we use, on github.com, over https.
        let https_github = ALLOWED_SOURCES.iter().any(|p| self.url.starts_with(p));
        if !(https_github || allow_http) {
            return bad(
                "the address must be a release of an approved project on github.com over https",
            );
        }
        if self.set != SET_SPEECH && self.set != SET_WAKE {
            return bad("unknown set");
        }
        if let Some(p) = &self.platform
            && !PLATFORMS.contains(&p.as_str())
        {
            return bad("unknown platform");
        }
        if !is_hex64(&self.archive_sha256) || self.archive_bytes == 0 {
            return bad("archive size or checksum is missing");
        }
        if self.dir.is_empty()
            || self.dir.split('/').any(|p| !is_plain_name(p))
            || self.dir.split('/').count() > 3
        {
            return bad("bad install folder");
        }
        if self.files.is_empty() || self.files.len() > 32 {
            return bad("the file list is empty or too long");
        }
        let mut total = 0u64;
        for (i, f) in self.files.iter().enumerate() {
            if !is_plain_name(&f.name) && f.name != "LICENSE" {
                return bad("bad file name");
            }
            if !is_hex64(&f.sha256) || f.bytes == 0 || f.bytes > MAX_FILE_BYTES {
                return bad("file size or checksum is wrong");
            }
            if let Some(path) = &f.path
                && (path.split('/').count() > 4 || path.split('/').any(|p| !is_plain_name(p)))
            {
                return bad("bad path inside the archive");
            }
            if self.files[..i].iter().any(|g| g.name == f.name) {
                return bad("a file is listed twice");
            }
            total += f.bytes;
        }
        if total > MAX_UNPACKED_BYTES {
            return bad("the files are larger than the unpacking limit");
        }
        if self.format == Format::File
            && (self.files.len() != 1
                || self.files[0].bytes != self.archive_bytes
                || self.files[0].sha256 != self.archive_sha256
                || self.files[0].path.is_some())
        {
            return bad("a single-file download must be exactly its one listed file");
        }
        Ok(())
    }

    fn rel_dir(&self) -> PathBuf {
        self.dir.split('/').collect()
    }
}

/// The models this app version knows about.
pub fn manifest() -> &'static [ModelSpec] {
    static M: LazyLock<Vec<ModelSpec>> = LazyLock::new(|| {
        let m: Manifest = serde_json::from_str(MANIFEST).expect("resources/models.json is valid");
        assert_eq!(m.version, 1, "unknown manifest version");
        for s in &m.models {
            s.validate(false).expect("resources/models.json is valid");
        }
        m.models
    });
    &M
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("the download could not be reached")]
    Offline,
    #[error("the server answered {0}")]
    Http(u16),
    #[error("the download stopped answering")]
    Stalled,
    #[error("the download was cancelled")]
    Cancelled,
    #[error("the download did not match the expected checksum")]
    ChecksumMismatch,
    #[error("the download was larger than expected")]
    TooBig,
    #[error("the downloaded archive is not what was expected: {0}")]
    BadArchive(String),
    #[error("a file could not be written: {0}")]
    Io(String),
    #[error("the models are already being downloaded")]
    Busy,
}

fn io(e: impl std::fmt::Display) -> ModelError {
    ModelError::Io(e.to_string())
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether the spec's files are installed and were verified when they were put there. Cheap: it
/// looks at the marker and the file sizes, and does not hash anything.
pub fn is_installed(root: &Path, spec: &ModelSpec) -> bool {
    let dir = root.join(spec.rel_dir());
    match std::fs::read_to_string(dir.join(MARKER)) {
        Ok(m) if m.trim() == spec.archive_sha256 => {}
        _ => return false,
    }
    spec.files.iter().all(|f| {
        std::fs::symlink_metadata(dir.join(&f.name))
            .map(|m| m.is_file() && m.len() == f.bytes)
            .unwrap_or(false)
    })
}

/// The downloads that are for `here`: the ones with no platform, and the ones for exactly this one.
fn for_platform(all: &[ModelSpec], here: &str) -> Vec<ModelSpec> {
    all.iter()
        .filter(|s| s.platform.as_deref().is_none_or(|p| p == here))
        .cloned()
        .collect()
}

fn http_client(allow_http: bool) -> reqwest::Client {
    use reqwest::redirect::Policy;
    reqwest::Client::builder()
        .user_agent("JARVIS")
        .connect_timeout(Duration::from_secs(15))
        // GitHub sends release downloads on to its own asset host; any hop must stay on https.
        .redirect(Policy::custom(move |a| {
            if a.previous().len() >= 5 {
                a.error("too many redirects")
            } else if a.url().scheme() == "https" || allow_http {
                a.follow()
            } else {
                a.error("a redirect to an address that is not https")
            }
        }))
        .build()
        .unwrap_or_default()
}

/// Streams a file through SHA-256. Checks `cancel` between chunks.
fn sha256_file(path: &Path, cancel: &CancellationToken) -> Result<String, ModelError> {
    let mut f = std::fs::File::open(path).map_err(io)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.is_cancelled() {
            return Err(ModelError::Cancelled);
        }
        let n = f.read(&mut buf).map_err(io)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

/// Which listed file (if any) an archive entry is, and the rules every listed file must meet.
struct Picker<'a> {
    spec: &'a ModelSpec,
    seen: Vec<&'a str>,
    total: u64,
}

impl<'a> Picker<'a> {
    fn new(spec: &'a ModelSpec) -> Self {
        Self {
            spec,
            seen: Vec::new(),
            total: 0,
        }
    }

    /// `comps` are the entry's path parts (no `.`), `is_file` says it is a plain file. An entry the
    /// manifest does not list is skipped (`None`); a listed one must be a plain file of the exact
    /// size, listed once, within the total bound.
    fn pick(
        &mut self,
        comps: &[String],
        is_file: bool,
        size: u64,
    ) -> Result<Option<&'a FileSpec>, ModelError> {
        let spec = self.spec;
        let Some(want) = spec.files.iter().find(|f| match &f.path {
            // The top folder, then exactly this path.
            Some(p) => comps.len() >= 2 && comps[1..].join("/") == *p,
            // At the top, or one folder down.
            None => matches!(comps, [n] | [_, n] if *n == f.name),
        }) else {
            return Ok(None);
        };
        if !is_file {
            return Err(ModelError::BadArchive(format!(
                "{} is not a plain file",
                want.name
            )));
        }
        if self.seen.contains(&want.name.as_str()) {
            return Err(ModelError::BadArchive("a file appears twice".into()));
        }
        if size != want.bytes {
            return Err(ModelError::BadArchive(format!(
                "{} has the wrong size",
                want.name
            )));
        }
        self.total += size;
        if self.total > MAX_UNPACKED_BYTES {
            return Err(ModelError::TooBig);
        }
        self.seen.push(&want.name);
        Ok(Some(want))
    }

    /// Every listed file must have turned up.
    fn finish(self) -> Result<(), ModelError> {
        match self
            .spec
            .files
            .iter()
            .find(|f| !self.seen.contains(&f.name.as_str()))
        {
            Some(missing) => Err(ModelError::BadArchive(format!(
                "{} is missing",
                missing.name
            ))),
            None => Ok(()),
        }
    }
}

/// The parts of an archive path. `.` is dropped; `..`, a root and a drive letter are refused.
fn path_parts(path: &Path) -> Result<Vec<String>, ModelError> {
    let mut out = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => out.push(p.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => return Err(ModelError::BadArchive("a path leaves the folder".into())),
        }
    }
    Ok(out)
}

/// Writes exactly `want.bytes` from `src` to `dst` while hashing them, and checks the hash. The
/// name on disk is the manifest's, never the archive's.
fn copy_verified(
    src: &mut dyn Read,
    dst: &Path,
    want: &FileSpec,
    cancel: &CancellationToken,
) -> Result<(), ModelError> {
    let mut f = std::fs::File::create(dst).map_err(io)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut left = want.bytes;
    while left > 0 {
        if cancel.is_cancelled() {
            return Err(ModelError::Cancelled);
        }
        let want_now = left.min(buf.len() as u64) as usize;
        let n = src
            .read(&mut buf[..want_now])
            .map_err(|e| ModelError::BadArchive(e.to_string()))?;
        if n == 0 {
            return Err(ModelError::BadArchive("a file ends early".into()));
        }
        h.update(&buf[..n]);
        f.write_all(&buf[..n]).map_err(io)?;
        left -= n as u64;
    }
    f.flush().map_err(io)?;
    if hex(&h.finalize()) != want.sha256 {
        return Err(ModelError::ChecksumMismatch);
    }
    Ok(())
}

fn bad_archive(e: impl std::fmt::Display) -> ModelError {
    ModelError::BadArchive(e.to_string())
}

/// Unpacks `archive` into the new folder `stage`, keeping only what the spec lists.
fn extract(
    archive: &Path,
    stage: &Path,
    spec: &ModelSpec,
    cancel: &CancellationToken,
) -> Result<(), ModelError> {
    let file = std::fs::File::open(archive).map_err(io)?;
    // A decompressed stream is cut off at the unpacking limit, so a bomb cannot fill the disk.
    let limit = MAX_UNPACKED_BYTES + MAX_FILE_BYTES;
    match spec.format {
        Format::TarBz2 => extract_tar(
            bzip2::read::BzDecoder::new(file).take(limit),
            stage,
            spec,
            cancel,
        ),
        Format::Tgz => extract_tar(
            flate2::read::GzDecoder::new(file).take(limit),
            stage,
            spec,
            cancel,
        ),
        Format::Zip => extract_zip(file, stage, spec, cancel),
        Format::File => {
            let want = &spec.files[0];
            let mut file = file;
            if file.metadata().map_err(io)?.len() != want.bytes {
                return Err(bad_archive("the file has the wrong size"));
            }
            copy_verified(&mut file, &stage.join(&want.name), want, cancel)
        }
    }
}

fn extract_tar<R: Read>(
    reader: R,
    stage: &Path,
    spec: &ModelSpec,
    cancel: &CancellationToken,
) -> Result<(), ModelError> {
    let mut tar = tar::Archive::new(reader);
    let mut picker = Picker::new(spec);
    for entry in tar.entries().map_err(bad_archive)? {
        if cancel.is_cancelled() {
            return Err(ModelError::Cancelled);
        }
        let mut entry = entry.map_err(bad_archive)?;
        let kind = entry.header().entry_type();
        let path = entry.path().map_err(bad_archive)?.into_owned();
        let comps = path_parts(&path)?;
        if kind.is_dir() {
            continue;
        }
        let Some(want) = picker.pick(&comps, kind.is_file(), entry.size())? else {
            continue; // not listed: skipped, whatever it is (a link included): never written
        };
        copy_verified(&mut entry, &stage.join(&want.name), want, cancel)?;
    }
    picker.finish()
}

fn extract_zip(
    file: std::fs::File,
    stage: &Path,
    spec: &ModelSpec,
    cancel: &CancellationToken,
) -> Result<(), ModelError> {
    let mut zip = zip::ZipArchive::new(file).map_err(bad_archive)?;
    let mut picker = Picker::new(spec);
    for i in 0..zip.len() {
        if cancel.is_cancelled() {
            return Err(ModelError::Cancelled);
        }
        let mut entry = zip.by_index(i).map_err(bad_archive)?;
        // `enclosed_name` is `None` for an absolute path or one that climbs out with `..`.
        let Some(path) = entry.enclosed_name() else {
            return Err(bad_archive("a path leaves the folder"));
        };
        let comps = path_parts(&path)?;
        if entry.is_dir() {
            continue;
        }
        let Some(want) = picker.pick(&comps, entry.is_file(), entry.size())? else {
            continue;
        };
        copy_verified(&mut entry, &stage.join(&want.name), want, cancel)?;
    }
    picker.finish()
}

/// Writes the marker into `stage` and moves it into place, replacing an older install.
fn put_in_place(root: &Path, stage: &Path, spec: &ModelSpec) -> Result<(), ModelError> {
    std::fs::write(stage.join(MARKER), &spec.archive_sha256).map_err(io)?;
    let dest = root.join(spec.rel_dir());
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(io)?;
    }
    std::fs::rename(stage, &dest).map_err(io)
}

/// A folder that is removed when it goes out of scope, unless it was moved away first.
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Copies the listed files out of a folder the person chose (an archive they unpacked themselves,
/// for a computer with no internet), checking every one against its pinned size and SHA-256.
/// Links are refused. Blocking.
#[cfg_attr(not(test), allow(dead_code))] // the window's "import from a folder" comes with its UI
pub fn import_from_folder(
    root: &Path,
    spec: &ModelSpec,
    folder: &Path,
    cancel: &CancellationToken,
) -> Result<(), ModelError> {
    let stage = Scratch(
        root.join(".staging")
            .join(format!("import-{}", uuid::Uuid::now_v7())),
    );
    std::fs::create_dir_all(&stage.0).map_err(io)?;
    for f in &spec.files {
        // In a folder the person unpacked, a file has the name it had in the archive.
        let in_folder = f
            .path
            .as_deref()
            .and_then(|p| p.rsplit('/').next())
            .unwrap_or(&f.name);
        let src = folder.join(in_folder);
        let meta = std::fs::symlink_metadata(&src)
            .map_err(|_| ModelError::BadArchive(format!("{} is not in that folder", f.name)))?;
        if !meta.is_file() || meta.len() != f.bytes {
            return Err(ModelError::BadArchive(format!(
                "{} is not the right file",
                f.name
            )));
        }
        let dst = stage.0.join(&f.name);
        std::fs::copy(&src, &dst).map_err(io)?;
        // Hash the copy, not the source, so what was checked is what is kept.
        if sha256_file(&dst, cancel)? != f.sha256 {
            return Err(ModelError::ChecksumMismatch);
        }
    }
    put_in_place(root, &stage.0, spec)?;
    // The folder was moved into place, so there is nothing for `Scratch` to delete.
    std::mem::forget(stage);
    Ok(())
}

/// Owns the models folder and runs one install at a time.
pub struct ModelManager {
    root: PathBuf,
    specs: Vec<ModelSpec>,
    http: reqwest::Client,
    busy: AtomicBool,
    cancel: Mutex<Option<CancellationToken>>,
    stall: Duration,
}

struct BusyGuard<'a>(&'a ModelManager);
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        *lock(&self.0.cancel) = None;
        self.0.busy.store(false, Ordering::SeqCst);
    }
}

impl ModelManager {
    /// `root` is the models folder inside the app data folder.
    pub fn new(root: PathBuf) -> Self {
        let specs = for_platform(manifest(), current_platform());
        Self::with(root, specs, http_client(false), STALL)
    }

    fn with(root: PathBuf, specs: Vec<ModelSpec>, http: reqwest::Client, stall: Duration) -> Self {
        Self {
            root,
            specs,
            http,
            busy: AtomicBool::new(false),
            cancel: Mutex::new(None),
            stall,
        }
    }

    /// True once every download of the speech set is installed and verified.
    pub fn ready(&self) -> bool {
        self.set_ready(SET_SPEECH)
    }

    /// True once everything the wake word needs on this system is installed and verified. False on a
    /// system the wake word has no download for.
    #[cfg_attr(not(test), allow(dead_code))] // the wake word's own steps use it
    pub fn wake_ready(&self) -> bool {
        self.set_ready(SET_WAKE)
    }

    fn set_ready(&self, set: &str) -> bool {
        let mut members = self.specs.iter().filter(|s| s.set == set).peekable();
        members.peek().is_some() && members.all(|s| is_installed(&self.root, s))
    }

    /// The folder a model's files are in (the speech engine reads it).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn dir_of(&self, id: &str) -> Option<PathBuf> {
        self.specs
            .iter()
            .find(|s| s.id == id)
            .map(|s| self.root.join(s.rel_dir()))
    }

    /// Stops a running install (its partial download is kept for a resume). Safe when idle.
    #[cfg_attr(not(test), allow(dead_code))] // the window's cancel button needs a contract command (later)
    pub fn cancel_install(&self) {
        if let Some(t) = lock(&self.cancel).as_ref() {
            t.cancel();
        }
    }

    /// Downloads and verifies every download of the speech set that is not installed yet.
    /// `progress` is called with the archive's progress, at most every 250 ms.
    pub async fn install_default(
        &self,
        progress: &(dyn Fn(ModelProgress) + Send + Sync),
    ) -> Result<(), ModelError> {
        self.install_set(SET_SPEECH, progress).await
    }

    /// The same for one named set (`speech` or `wake`).
    pub async fn install_set(
        &self,
        set: &str,
        progress: &(dyn Fn(ModelProgress) + Send + Sync),
    ) -> Result<(), ModelError> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(ModelError::Busy);
        }
        let _busy = BusyGuard(self);
        let cancel = CancellationToken::new();
        *lock(&self.cancel) = Some(cancel.clone());
        for spec in self.specs.iter().filter(|s| s.set == set) {
            if is_installed(&self.root, spec) {
                continue;
            }
            self.install_one(spec, progress, &cancel).await?;
        }
        Ok(())
    }

    async fn install_one(
        &self,
        spec: &ModelSpec,
        progress: &(dyn Fn(ModelProgress) + Send + Sync),
        cancel: &CancellationToken,
    ) -> Result<(), ModelError> {
        let downloads = self.root.join(".downloads");
        tokio::fs::create_dir_all(&downloads).await.map_err(io)?;
        let part = downloads.join(format!("{}.part", spec.id));
        match tokio::time::timeout(DOWNLOAD_LIMIT, self.download(spec, &part, progress, cancel))
            .await
        {
            Ok(r) => r?,
            Err(_) => return Err(ModelError::Stalled),
        }
        // Checking and unpacking are disk and CPU work: off the async worker threads.
        let (root, spec2, part2, cancel2) = (
            self.root.clone(),
            spec.clone(),
            part.clone(),
            cancel.clone(),
        );
        tokio::task::spawn_blocking(move || -> Result<(), ModelError> {
            if sha256_file(&part2, &cancel2)? != spec2.archive_sha256 {
                // A wrong download is of no use to a resume: start again next time.
                let _ = std::fs::remove_file(&part2);
                return Err(ModelError::ChecksumMismatch);
            }
            let stage = Scratch(root.join(".staging").join(format!(
                "{}-{}",
                spec2.id,
                uuid::Uuid::now_v7()
            )));
            std::fs::create_dir_all(&stage.0).map_err(io)?;
            match extract(&part2, &stage.0, &spec2, &cancel2) {
                Ok(()) => {}
                Err(e) => {
                    // A archive with the right checksum that does not unpack as promised is not
                    // going to unpack better next time either.
                    if !matches!(e, ModelError::Cancelled) {
                        let _ = std::fs::remove_file(&part2);
                    }
                    return Err(e);
                }
            }
            put_in_place(&root, &stage.0, &spec2)?;
            std::mem::forget(stage); // moved into place
            let _ = std::fs::remove_file(&part2);
            Ok(())
        })
        .await
        .map_err(|e| ModelError::Io(e.to_string()))?
    }

    /// Downloads the archive into `part`, resuming what is already there.
    async fn download(
        &self,
        spec: &ModelSpec,
        part: &Path,
        progress: &(dyn Fn(ModelProgress) + Send + Sync),
        cancel: &CancellationToken,
    ) -> Result<(), ModelError> {
        let total = spec.archive_bytes;
        let mut have = tokio::fs::metadata(part)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if have > total {
            let _ = tokio::fs::remove_file(part).await;
            have = 0;
        }
        let report = |done: u64| {
            progress(ModelProgress {
                name: spec.name.clone(),
                done_bytes: done,
                total_bytes: total,
            })
        };
        if have == total {
            report(total);
            return Ok(()); // already complete: the checksum decides
        }
        let mut req = self.http.get(&spec.url);
        if have > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(ModelError::Cancelled),
            r = tokio::time::timeout(self.stall, req.send()) => match r {
                Err(_) => return Err(ModelError::Stalled),
                Ok(Err(_)) => return Err(ModelError::Offline),
                Ok(Ok(r)) => r,
            },
        };
        let status = resp.status().as_u16();
        let mut file = match status {
            206 if have > 0 => tokio::fs::OpenOptions::new()
                .append(true)
                .open(part)
                .await
                .map_err(io)?,
            // The server ignored the range, or this is a first download: start from the top.
            200 => {
                have = 0;
                tokio::fs::File::create(part).await.map_err(io)?
            }
            // The partial file no longer fits the file on the server: start again next time.
            416 => {
                let _ = tokio::fs::remove_file(part).await;
                return Err(ModelError::Http(416));
            }
            other => return Err(ModelError::Http(other)),
        };
        let mut done = have;
        let mut last = Instant::now() - PROGRESS_EVERY;
        let mut stream = resp.bytes_stream();
        // However the loop ends (done, cancelled, stalled, a broken connection), what has arrived
        // is flushed to the file before returning, so a resume starts from everything that came.
        // (A `tokio::fs::File` that is dropped without a flush can lose its last writes.)
        let outcome: Result<(), ModelError> = loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => break Err(ModelError::Cancelled),
                c = tokio::time::timeout(self.stall, stream.next()) => match c {
                    Err(_) => break Err(ModelError::Stalled),
                    Ok(c) => c,
                },
            };
            let Some(chunk) = chunk else { break Ok(()) };
            let chunk = match chunk {
                Ok(c) => c,
                Err(_) => break Err(ModelError::Offline),
            };
            done += chunk.len() as u64;
            if done > total {
                break Err(ModelError::TooBig);
            }
            if let Err(e) = file.write_all(&chunk).await {
                break Err(io(e));
            }
            if last.elapsed() >= PROGRESS_EVERY {
                last = Instant::now();
                report(done);
            }
        };
        let flushed = file.flush().await.map_err(io);
        drop(file);
        match outcome {
            Err(ModelError::TooBig) => {
                // More than promised is not a partial download worth resuming.
                let _ = tokio::fs::remove_file(part).await;
                return Err(ModelError::TooBig);
            }
            Err(e) => return Err(e),
            Ok(()) => {}
        }
        flushed?;
        report(done);
        if done != total {
            // The connection ended early. What arrived is kept, and the next try resumes.
            return Err(ModelError::Offline);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
