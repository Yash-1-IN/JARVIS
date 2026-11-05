use super::*;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn temp_root(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jarvis-models-{tag}-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sha(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

/// What goes into a test archive.
enum Item<'a> {
    File(&'a str, &'a [u8]),
    /// A path written as given, even if it has `..` (the builder would refuse it).
    RawFile(&'a str, &'a [u8]),
    Symlink(&'a str, &'a str),
}

fn archive(items: &[Item]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    for it in items {
        match it {
            Item::File(path, data) => {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                tar.append_data(&mut h, path, *data).unwrap();
            }
            Item::RawFile(path, data) => {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                let name = &mut h.as_old_mut().name;
                name.fill(0);
                name[..path.len()].copy_from_slice(path.as_bytes());
                h.set_cksum();
                tar.append(&h, *data).unwrap();
            }
            Item::Symlink(path, to) => {
                let mut h = tar::Header::new_gnu();
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_size(0);
                h.set_mode(0o777);
                h.set_cksum();
                tar.append_link(&mut h, path, to).unwrap();
            }
        }
    }
    let raw = tar.into_inner().unwrap();
    let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    enc.write_all(&raw).unwrap();
    enc.finish().unwrap()
}

/// The same items as a `.tgz`.
fn archive_tgz(items: &[Item]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    for it in items {
        match it {
            Item::File(path, data) | Item::RawFile(path, data) => {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                tar.append_data(&mut h, path, *data).unwrap();
            }
            Item::Symlink(path, to) => {
                let mut h = tar::Header::new_gnu();
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_size(0);
                h.set_mode(0o777);
                h.set_cksum();
                tar.append_link(&mut h, path, to).unwrap();
            }
        }
    }
    let raw = tar.into_inner().unwrap();
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    enc.write_all(&raw).unwrap();
    enc.finish().unwrap()
}

/// A deflated `.zip` of `(name, bytes)` entries; names are written as given.
fn archive_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in entries {
        zw.start_file(*name, opts).unwrap();
        zw.write_all(data).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

const TOKENS: &[u8] = b"a 1\nb 2\n";
const MODEL: &[u8] = b"model-bytes-model-bytes-model-bytes";

fn good_items() -> Vec<Item<'static>> {
    vec![
        Item::File("pack/tokens.txt", TOKENS),
        Item::File("pack/model.onnx", MODEL),
        // Not in the manifest: skipped, never written.
        Item::File("pack/test_wavs/0.wav", b"riff"),
    ]
}

fn spec_for(archive: &[u8], url: String) -> ModelSpec {
    ModelSpec {
        id: "stt-test".into(),
        name: "Test model".into(),
        purpose: "speech-to-text".into(),
        licence: "MIT".into(),
        set: SET_SPEECH.into(),
        platform: None,
        format: Format::TarBz2,
        url,
        archive_bytes: archive.len() as u64,
        archive_sha256: sha(archive),
        dir: "stt/test".into(),
        files: vec![
            FileSpec {
                name: "tokens.txt".into(),
                path: None,
                bytes: TOKENS.len() as u64,
                sha256: sha(TOKENS),
            },
            FileSpec {
                name: "model.onnx".into(),
                path: None,
                bytes: MODEL.len() as u64,
                sha256: sha(MODEL),
            },
        ],
    }
}

/// A tiny HTTP server for one file, with `Range` support and ways to misbehave.
struct Server {
    url: String,
    ranges: Arc<Mutex<Vec<Option<String>>>>,
    hits: Arc<AtomicUsize>,
    cut_after: Arc<Mutex<Option<usize>>>,
    ignore_range: Arc<AtomicBool>,
    stall: Arc<AtomicBool>,
}

async fn serve(body: Vec<u8>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/model.tar.bz2", listener.local_addr().unwrap());
    let s = Server {
        url,
        ranges: Arc::default(),
        hits: Arc::default(),
        cut_after: Arc::default(),
        ignore_range: Arc::default(),
        stall: Arc::default(),
    };
    let (ranges, hits, cut, ignore, stall) = (
        s.ranges.clone(),
        s.hits.clone(),
        s.cut_after.clone(),
        s.ignore_range.clone(),
        s.stall.clone(),
    );
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let (body, ranges, hits, cut, ignore, stall) = (
                body.clone(),
                ranges.clone(),
                hits.clone(),
                cut.clone(),
                ignore.clone(),
                stall.clone(),
            );
            tokio::spawn(async move {
                let mut req = Vec::new();
                let mut buf = [0u8; 1024];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => req.extend_from_slice(&buf[..n]),
                    }
                }
                hits.fetch_add(1, Ordering::SeqCst);
                let text = String::from_utf8_lossy(&req).to_lowercase();
                let range = text
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .map(|r| r.trim_end_matches('-').to_owned());
                lock(&ranges).push(range.clone());
                let start = match (&range, ignore.load(Ordering::SeqCst)) {
                    (Some(r), false) => r.parse::<usize>().unwrap_or(0),
                    _ => 0,
                };
                let (head, rest) = if start > 0 {
                    (
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                            body.len() - start,
                            start,
                            body.len() - 1,
                            body.len()
                        ),
                        &body[start..],
                    )
                } else {
                    (
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        ),
                        &body[..],
                    )
                };
                let _ = sock.write_all(head.as_bytes()).await;
                if stall.load(Ordering::SeqCst) {
                    let _ = sock.write_all(&rest[..rest.len().min(8)]).await;
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    return;
                }
                let cut = lock(&cut).take();
                let end = cut.map(|c| c.min(rest.len())).unwrap_or(rest.len());
                let _ = sock.write_all(&rest[..end]).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    s
}

fn manager(root: &Path, spec: &ModelSpec) -> ModelManager {
    ModelManager::with(
        root.to_path_buf(),
        vec![spec.clone()],
        http_client(true),
        Duration::from_secs(3),
    )
}

type Seen = Arc<Mutex<Vec<ModelProgress>>>;

/// Like `run`, for one named set.
async fn run_set(m: &ModelManager, set: &str) -> Result<(), ModelError> {
    m.install_set(set, &|_| {}).await
}

async fn run(m: &ModelManager) -> (Result<(), ModelError>, Vec<ModelProgress>) {
    let seen: Seen = Arc::default();
    let s2 = seen.clone();
    let r = m.install_default(&move |p| lock(&s2).push(p)).await;
    let v = lock(&seen).clone();
    (r, v)
}

#[test]
fn the_shipped_manifest_is_valid_and_pins_everything() {
    let m = manifest();
    assert!(!m.is_empty());
    let mut speech_total = 0;
    let mut dirs = std::collections::HashSet::new();
    for s in m {
        s.validate(false).unwrap();
        assert!(ALLOWED_SOURCES.iter().any(|p| s.url.starts_with(p)));
        assert!(is_hex64(&s.archive_sha256));
        assert!(s.files.iter().all(|f| is_hex64(&f.sha256)));
        // Two downloads must never share a folder: installing one replaces the folder.
        assert!(dirs.insert(s.dir.clone()), "{} shares its folder", s.id);
        // The licence file of anything that comes as an archive travels with it.
        if s.format != Format::File {
            assert!(s.files.iter().any(|f| f.name == "LICENSE"), "{}", s.id);
        }
        if s.set == SET_SPEECH {
            speech_total += s.archive_bytes;
        }
    }
    assert!(
        speech_total <= DEFAULT_SET_LIMIT,
        "speech set is {speech_total} bytes"
    );
    // The wake word has its three model files, and a runtime for every system the alpha runs on.
    let wake: Vec<&ModelSpec> = m.iter().filter(|s| s.set == SET_WAKE).collect();
    for id in [
        "wake-oww-melspectrogram",
        "wake-oww-embedding",
        "wake-oww-hey-jarvis",
    ] {
        let s = wake
            .iter()
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("{id}"));
        assert_eq!(s.format, Format::File);
        assert!(s.platform.is_none() && s.licence.contains("CC-BY-NC-SA-4.0"));
    }
    for plat in PLATFORMS {
        let s = wake
            .iter()
            .find(|s| s.platform.as_deref() == Some(*plat))
            .unwrap_or_else(|| panic!("no ONNX Runtime for {plat}"));
        // The one library that gets loaded is listed with its exact path in the archive.
        let lib = s
            .files
            .iter()
            .filter(|f| f.name != "LICENSE" && !f.name.contains("providers"));
        assert_eq!(lib.count(), 1, "{plat}");
        assert!(s.files.iter().all(|f| f.path.is_some()), "{plat}");
    }
}

#[test]
fn a_manifest_entry_that_could_escape_or_come_from_elsewhere_is_refused() {
    let a = archive(&good_items());
    let ok = spec_for(
        &a,
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/x/y.tar.bz2".into(),
    );
    assert!(ok.validate(false).is_ok());
    let mut s = ok.clone();
    s.url = "https://example.com/y.tar.bz2".into();
    assert!(s.validate(false).is_err());
    let mut s = ok.clone();
    s.url = "http://github.com/k2-fsa/sherpa-onnx/releases/download/x/y".into();
    assert!(s.validate(false).is_err());
    for dir in ["../x", "/abs", "a//b", "a/../b", ".hidden", "a/b/c/d"] {
        let mut s = ok.clone();
        s.dir = dir.into();
        assert!(s.validate(false).is_err(), "{dir}");
    }
    for name in ["../a", "a/b", "", ".."] {
        let mut s = ok.clone();
        s.files[0].name = name.into();
        assert!(s.validate(false).is_err(), "{name:?}");
    }
    let mut s = ok.clone();
    s.archive_sha256 = "ABC".into();
    assert!(s.validate(false).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_is_checked_unpacked_and_installed_and_a_second_run_does_nothing() {
    let a = archive(&good_items());
    let srv = serve(a.clone()).await;
    let spec = spec_for(&a, srv.url.clone());
    let root = temp_root("ok");
    let m = manager(&root, &spec);
    assert!(!m.ready());
    let (r, seen) = run(&m).await;
    assert_eq!(r, Ok(()));
    assert!(m.ready());
    // Progress goes up to the whole archive and never past it.
    assert!(!seen.is_empty());
    assert!(seen.windows(2).all(|w| w[0].done_bytes <= w[1].done_bytes));
    let last = seen.last().unwrap();
    assert_eq!(
        (last.done_bytes, last.total_bytes),
        (a.len() as u64, a.len() as u64)
    );
    assert_eq!(last.name, "Test model");
    // The files are there, the unlisted sample recording is not, and nothing else is left over.
    let dir = m.dir_of("stt-test").unwrap();
    assert_eq!(std::fs::read(dir.join("tokens.txt")).unwrap(), TOKENS);
    assert_eq!(std::fs::read(dir.join("model.onnx")).unwrap(), MODEL);
    assert!(!dir.join("test_wavs").exists() && !dir.join("0.wav").exists());
    assert!(!root.join(".downloads/stt-test.part").exists());
    assert_eq!(
        std::fs::read_dir(root.join(".staging"))
            .map(|d| d.count())
            .unwrap_or(0),
        0
    );
    // Already installed: no request at all.
    let before = srv.hits.load(Ordering::SeqCst);
    assert_eq!(run(&m).await.0, Ok(()));
    assert_eq!(srv.hits.load(Ordering::SeqCst), before);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_interrupted_download_resumes_from_where_it_stopped() {
    let a = archive(&good_items());
    let srv = serve(a.clone()).await;
    *lock(&srv.cut_after) = Some(a.len() / 2);
    let spec = spec_for(&a, srv.url.clone());
    let root = temp_root("resume");
    let m = manager(&root, &spec);
    let (r, _) = run(&m).await;
    assert_eq!(r, Err(ModelError::Offline));
    let part = root.join(".downloads/stt-test.part");
    let kept = std::fs::metadata(&part).unwrap().len();
    assert_eq!(kept, (a.len() / 2) as u64, "the half that arrived is kept");
    assert!(!m.ready());
    // The second try asks only for the rest.
    let (r, seen) = run(&m).await;
    assert_eq!(r, Ok(()));
    assert!(m.ready());
    assert_eq!(
        lock(&srv.ranges).last().unwrap().as_deref(),
        Some(&*kept.to_string())
    );
    assert_eq!(seen.first().map(|p| p.done_bytes >= kept), Some(true));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_ignores_the_range_makes_it_start_over_cleanly() {
    let a = archive(&good_items());
    let srv = serve(a.clone()).await;
    srv.ignore_range.store(true, Ordering::SeqCst);
    let spec = spec_for(&a, srv.url.clone());
    let root = temp_root("ignore");
    std::fs::create_dir_all(root.join(".downloads")).unwrap();
    std::fs::write(root.join(".downloads/stt-test.part"), &a[..10]).unwrap();
    let m = manager(&root, &spec);
    assert_eq!(run(&m).await.0, Ok(()));
    assert!(m.ready());
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_with_the_wrong_checksum_is_thrown_away_and_nothing_is_installed() {
    let a = archive(&good_items());
    let mut tampered = a.clone();
    let n = tampered.len() / 2;
    tampered[n] ^= 0xff;
    let srv = serve(tampered).await;
    let spec = spec_for(&a, srv.url.clone()); // pinned to the good one
    let root = temp_root("sum");
    let m = manager(&root, &spec);
    assert_eq!(run(&m).await.0, Err(ModelError::ChecksumMismatch));
    assert!(!m.ready());
    assert!(!m.dir_of("stt-test").unwrap().exists());
    assert!(!root.join(".downloads/stt-test.part").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_larger_than_promised_is_stopped() {
    let a = archive(&good_items());
    let mut longer = a.clone();
    longer.extend_from_slice(&[0u8; 64]);
    let srv = serve(longer).await;
    let spec = spec_for(&a, srv.url.clone());
    let root = temp_root("big");
    let m = manager(&root, &spec);
    assert_eq!(run(&m).await.0, Err(ModelError::TooBig));
    assert!(!m.ready());
    let _ = std::fs::remove_dir_all(root);
}

/// An archive that passes the checksum (it is what the manifest pins) but is hostile: nothing may
/// leave the staging folder, and nothing is installed.
async fn hostile(tag: &str, items: Vec<Item<'static>>) {
    let a = archive(&items);
    let srv = serve(a.clone()).await;
    let spec = spec_for(&a, srv.url.clone());
    let root = temp_root(tag);
    let m = manager(&root, &spec);
    let (r, _) = run(&m).await;
    assert!(matches!(r, Err(ModelError::BadArchive(_))), "{tag}: {r:?}");
    assert!(!m.ready(), "{tag}");
    assert!(!m.dir_of("stt-test").unwrap().exists(), "{tag}");
    assert_eq!(
        std::fs::read_dir(root.join(".staging"))
            .map(|d| d.count())
            .unwrap_or(0),
        0,
        "{tag}: staging left behind"
    );
    // Nothing was written next to the models folder either.
    assert!(!root.parent().unwrap().join("evil.txt").exists(), "{tag}");
    assert!(!root.join("evil.txt").exists(), "{tag}");
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hostile_archives_are_refused() {
    // Climbs out of the folder.
    hostile(
        "dotdot",
        vec![
            Item::RawFile("../evil.txt", b"x"),
            Item::File("pack/tokens.txt", TOKENS),
            Item::File("pack/model.onnx", MODEL),
        ],
    )
    .await;
    // An absolute path.
    hostile(
        "abs",
        vec![
            Item::RawFile("/evil.txt", b"x"),
            Item::File("pack/tokens.txt", TOKENS),
            Item::File("pack/model.onnx", MODEL),
        ],
    )
    .await;
    // A link, even to somewhere harmless-looking.
    hostile(
        "link",
        vec![
            Item::Symlink("pack/tokens.txt", "/etc/passwd"),
            Item::File("pack/model.onnx", MODEL),
        ],
    )
    .await;
    // A listed file with the wrong size or wrong content.
    hostile(
        "size",
        vec![
            Item::File("pack/tokens.txt", b"short"),
            Item::File("pack/model.onnx", MODEL),
        ],
    )
    .await;
    // Same size, different content: the per-file checksum catches it.
    let mut other = MODEL.to_vec();
    other[0] ^= 1;
    let a = archive(&[
        Item::File("pack/tokens.txt", TOKENS),
        Item::RawFile("pack/model.onnx", Box::leak(other.into_boxed_slice())),
    ]);
    let srv = serve(a.clone()).await;
    let spec = spec_for(&a, srv.url.clone());
    let root = temp_root("content");
    let m = manager(&root, &spec);
    assert_eq!(run(&m).await.0, Err(ModelError::ChecksumMismatch));
    assert!(!m.ready());
    let _ = std::fs::remove_dir_all(root);
    // A listed file is missing.
    hostile("missing", vec![Item::File("pack/tokens.txt", TOKENS)]).await;
    // The same file twice.
    hostile(
        "twice",
        vec![
            Item::File("pack/tokens.txt", TOKENS),
            Item::File("pack/tokens.txt", TOKENS),
            Item::File("pack/model.onnx", MODEL),
        ],
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_stops_a_running_download_and_keeps_what_arrived() {
    let a = archive(&good_items());
    let srv = serve(a.clone()).await;
    srv.stall.store(true, Ordering::SeqCst);
    let spec = spec_for(&a, srv.url.clone());
    let root = temp_root("cancel");
    let m = Arc::new(manager(&root, &spec));
    let m2 = m.clone();
    let task = tokio::spawn(async move { run(&m2).await.0 });
    for _ in 0..200 {
        if srv.hits.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    m.cancel_install();
    assert_eq!(task.await.unwrap(), Err(ModelError::Cancelled));
    assert!(!m.ready());
    // Idle again: another install can start (and cancelling while idle is safe).
    m.cancel_install();
    srv.stall.store(false, Ordering::SeqCst);
    assert_eq!(run(&m).await.0, Ok(()));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_stops_answering_gives_up_after_the_stall_time() {
    let a = archive(&good_items());
    let srv = serve(a.clone()).await;
    srv.stall.store(true, Ordering::SeqCst);
    let spec = spec_for(&a, srv.url.clone());
    let root = temp_root("stall");
    let m = ModelManager::with(
        root.clone(),
        vec![spec],
        http_client(true),
        Duration::from_millis(300),
    );
    assert_eq!(run(&m).await.0, Err(ModelError::Stalled));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_install_while_one_runs_is_refused() {
    let a = archive(&good_items());
    let srv = serve(a.clone()).await;
    srv.stall.store(true, Ordering::SeqCst);
    let spec = spec_for(&a, srv.url.clone());
    let root = temp_root("busy");
    let m = Arc::new(manager(&root, &spec));
    let m2 = m.clone();
    let first = tokio::spawn(async move { run(&m2).await.0 });
    for _ in 0..200 {
        if srv.hits.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(run(&m).await.0, Err(ModelError::Busy));
    m.cancel_install();
    assert_eq!(first.await.unwrap(), Err(ModelError::Cancelled));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_broken_install_is_not_ready_even_if_the_marker_is_there() {
    let a = archive(&good_items());
    let spec = spec_for(&a, "http://x/y".into());
    let root = temp_root("ready");
    let dir = root.join("stt/test");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tokens.txt"), TOKENS).unwrap();
    std::fs::write(dir.join("model.onnx"), MODEL).unwrap();
    assert!(!is_installed(&root, &spec), "no marker");
    std::fs::write(dir.join(MARKER), "something-else").unwrap();
    assert!(!is_installed(&root, &spec), "marker for another archive");
    std::fs::write(dir.join(MARKER), &spec.archive_sha256).unwrap();
    assert!(is_installed(&root, &spec));
    std::fs::write(dir.join("model.onnx"), b"truncated").unwrap();
    assert!(!is_installed(&root, &spec), "a file changed size");
    std::fs::remove_file(dir.join("tokens.txt")).unwrap();
    assert!(!is_installed(&root, &spec), "a file is gone");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn import_from_a_folder_checks_every_file() {
    let a = archive(&good_items());
    let spec = spec_for(&a, "http://x/y".into());
    let root = temp_root("import");
    let src = temp_root("import-src");
    let cancel = CancellationToken::new();
    // Missing files.
    assert!(matches!(
        import_from_folder(&root, &spec, &src, &cancel),
        Err(ModelError::BadArchive(_))
    ));
    std::fs::write(src.join("tokens.txt"), TOKENS).unwrap();
    // Right size, wrong content.
    let mut bad = MODEL.to_vec();
    bad[0] ^= 1;
    std::fs::write(src.join("model.onnx"), &bad).unwrap();
    assert_eq!(
        import_from_folder(&root, &spec, &src, &cancel),
        Err(ModelError::ChecksumMismatch)
    );
    assert!(!is_installed(&root, &spec));
    assert_eq!(
        std::fs::read_dir(root.join(".staging"))
            .map(|d| d.count())
            .unwrap_or(0),
        0
    );
    // The right files.
    std::fs::write(src.join("model.onnx"), MODEL).unwrap();
    import_from_folder(&root, &spec, &src, &cancel).unwrap();
    assert!(is_installed(&root, &spec));
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(src);
}

#[cfg(unix)]
#[test]
fn import_refuses_a_link_in_place_of_a_file() {
    let a = archive(&good_items());
    let spec = spec_for(&a, "http://x/y".into());
    let root = temp_root("import-link");
    let src = temp_root("import-link-src");
    let real = src.join("real-model");
    std::fs::write(&real, MODEL).unwrap();
    std::fs::write(src.join("tokens.txt"), TOKENS).unwrap();
    std::os::unix::fs::symlink(&real, src.join("model.onnx")).unwrap();
    assert!(matches!(
        import_from_folder(&root, &spec, &src, &CancellationToken::new()),
        Err(ModelError::BadArchive(_))
    ));
    assert!(!is_installed(&root, &spec));
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(src);
}

/// Needs the real archive on disk: `JARVIS_TEST_MOONSHINE_ARCHIVE=<file.tar.bz2> cargo test -- --ignored`.
/// Proves the pinned checksums and the unpacking rules fit the real model.
#[test]
#[ignore = "needs the real model archive"]
fn the_real_archive_matches_the_manifest_and_unpacks() {
    let Ok(path) = std::env::var("JARVIS_TEST_MOONSHINE_ARCHIVE") else {
        panic!("set JARVIS_TEST_MOONSHINE_ARCHIVE");
    };
    let spec = manifest()
        .iter()
        .find(|s| s.set == SET_SPEECH)
        .unwrap()
        .clone();
    let cancel = CancellationToken::new();
    let archive = PathBuf::from(path);
    assert_eq!(sha256_file(&archive, &cancel).unwrap(), spec.archive_sha256);
    assert_eq!(
        std::fs::metadata(&archive).unwrap().len(),
        spec.archive_bytes
    );
    let root = temp_root("real-archive");
    let stage = root.join("stage");
    std::fs::create_dir_all(&stage).unwrap();
    extract(&archive, &stage, &spec, &cancel).unwrap();
    put_in_place(&root, &stage, &spec).unwrap();
    assert!(is_installed(&root, &spec));
    let _ = std::fs::remove_dir_all(root);
}

/// A real download from github.com through the manager. Needs internet and about 250 MB.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads about 250 MB from github.com"]
async fn a_real_download_from_github_installs_the_model() {
    let root = temp_root("real-download");
    let m = ModelManager::new(root.clone());
    let (r, seen) = run(&m).await;
    assert_eq!(r, Ok(()), "{seen:?}");
    assert!(m.ready());
    let last = seen.last().unwrap();
    assert_eq!(last.done_bytes, last.total_bytes);
    let _ = std::fs::remove_dir_all(root);
}

// ---- Step 1 of the wake word (D-29): per-system downloads, tgz and zip, single files, two sets.

const LIB: &[u8] = b"pretend-this-is-a-native-library-0123456789";
const LICENSE_TXT: &[u8] = b"MIT License";

/// A spec for a runtime archive: the library is at `lib/libfoo.so.1.2` inside the archive and is
/// called `libfoo.so` on disk.
fn runtime_spec(archive: &[u8], url: String, format: Format, platform: Option<&str>) -> ModelSpec {
    ModelSpec {
        id: "ort-test".into(),
        name: "Runtime".into(),
        purpose: "wake-word runtime".into(),
        licence: "MIT".into(),
        set: SET_WAKE.into(),
        platform: platform.map(str::to_owned),
        format,
        url,
        archive_bytes: archive.len() as u64,
        archive_sha256: sha(archive),
        dir: "ort/test".into(),
        files: vec![
            FileSpec {
                name: "LICENSE".into(),
                path: Some("LICENSE".into()),
                bytes: LICENSE_TXT.len() as u64,
                sha256: sha(LICENSE_TXT),
            },
            FileSpec {
                name: "libfoo.so".into(),
                path: Some("lib/libfoo.so.1.2".into()),
                bytes: LIB.len() as u64,
                sha256: sha(LIB),
            },
        ],
    }
}

fn read(dir: &Path, name: &str) -> Vec<u8> {
    std::fs::read(dir.join(name)).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tgz_with_dot_slash_paths_and_links_installs_only_the_listed_files_under_the_manifests_names()
 {
    // The shape of the real ONNX Runtime archives: `./top/...`, a symlink next to the real file,
    // and things nobody asked for.
    let a = archive_tgz(&[
        Item::File("./top/LICENSE", LICENSE_TXT),
        Item::File("./top/lib/libfoo.so.1.2", LIB),
        Item::Symlink("./top/lib/libfoo.so", "libfoo.so.1.2"),
        Item::Symlink("./top/lib/libfoo.so.1", "libfoo.so.1.2"),
        Item::File("./top/include/foo.h", b"header"),
        Item::File("./top/ThirdPartyNotices.txt", b"notices"),
    ]);
    let srv = serve(a.clone()).await;
    let spec = runtime_spec(&a, srv.url.clone(), Format::Tgz, None);
    let root = temp_root("tgz");
    let m = manager(&root, &spec);
    assert_eq!(run_set(&m, SET_WAKE).await, Ok(()));
    assert!(m.wake_ready() && !m.ready());
    let dir = root.join("ort/test");
    // Named by the manifest ("libfoo.so", not the archive's "libfoo.so.1.2"), and nothing else.
    assert_eq!(read(&dir, "libfoo.so"), LIB);
    assert_eq!(read(&dir, "LICENSE"), LICENSE_TXT);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, [".verified", "LICENSE", "libfoo.so"]);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deflated_zip_installs_the_listed_files_and_refuses_a_path_that_climbs() {
    let a = archive_zip(&[
        ("top/LICENSE", LICENSE_TXT),
        ("top/lib/libfoo.so.1.2", LIB),
        ("top/lib/other.dll", b"not listed"),
    ]);
    let srv = serve(a.clone()).await;
    let spec = runtime_spec(&a, srv.url.clone(), Format::Zip, None);
    let root = temp_root("zip");
    let m = manager(&root, &spec);
    assert_eq!(run_set(&m, SET_WAKE).await, Ok(()));
    assert_eq!(read(&root.join("ort/test"), "libfoo.so"), LIB);
    assert!(!root.join("ort/test/other.dll").exists());
    let _ = std::fs::remove_dir_all(root);

    // An entry that leaves the folder is refused, listed or not, and nothing is installed.
    let a = archive_zip(&[
        ("top/LICENSE", LICENSE_TXT),
        ("../evil.txt", b"x"),
        ("top/lib/libfoo.so.1.2", LIB),
    ]);
    let srv = serve(a.clone()).await;
    let spec = runtime_spec(&a, srv.url.clone(), Format::Zip, None);
    let root = temp_root("zip-slip");
    let m = manager(&root, &spec);
    assert!(matches!(
        run_set(&m, SET_WAKE).await,
        Err(ModelError::BadArchive(_))
    ));
    assert!(!m.wake_ready());
    assert!(!root.parent().unwrap().join("evil.txt").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listed_path_must_be_exactly_where_the_manifest_says() {
    // The right name in the wrong place, a link in the right place, and a path that only ends the
    // same way are none of them the listed file.
    for (tag, items) in [
        (
            "wrong-folder",
            vec![
                Item::File("top/LICENSE", LICENSE_TXT),
                Item::File("top/other/libfoo.so.1.2", LIB),
            ],
        ),
        (
            "no-top-folder",
            vec![
                Item::File("top/LICENSE", LICENSE_TXT),
                Item::File("lib/libfoo.so.1.2", LIB),
            ],
        ),
        (
            "link",
            vec![
                Item::File("top/LICENSE", LICENSE_TXT),
                Item::Symlink("top/lib/libfoo.so.1.2", "/usr/lib/libc.so"),
            ],
        ),
    ] {
        let a = archive_tgz(&items);
        let srv = serve(a.clone()).await;
        let spec = runtime_spec(&a, srv.url.clone(), Format::Tgz, None);
        let root = temp_root(tag);
        let m = manager(&root, &spec);
        assert!(
            matches!(run_set(&m, SET_WAKE).await, Err(ModelError::BadArchive(_))),
            "{tag}"
        );
        assert!(!m.wake_ready(), "{tag}");
        assert!(!root.join("ort/test").exists(), "{tag}");
        let _ = std::fs::remove_dir_all(root);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_single_file_download_is_checked_and_installed_as_it_is() {
    let spec_for_file = |body: &[u8], url: String| ModelSpec {
        id: "wake-test".into(),
        name: "A model".into(),
        purpose: "wake-word".into(),
        licence: "CC-BY-NC-SA-4.0".into(),
        set: SET_WAKE.into(),
        platform: None,
        format: Format::File,
        url,
        archive_bytes: body.len() as u64,
        archive_sha256: sha(body),
        dir: "wake/test".into(),
        files: vec![FileSpec {
            name: "model.onnx".into(),
            path: None,
            bytes: body.len() as u64,
            sha256: sha(body),
        }],
    };
    let body = b"onnx-model-bytes".to_vec();
    let srv = serve(body.clone()).await;
    let spec = spec_for_file(&body, srv.url.clone());
    assert!(spec.validate(true).is_ok());
    let root = temp_root("file");
    let m = manager(&root, &spec);
    assert_eq!(run_set(&m, SET_WAKE).await, Ok(()));
    assert!(m.wake_ready());
    assert_eq!(read(&root.join("wake/test"), "model.onnx"), body);
    let _ = std::fs::remove_dir_all(root);

    // A different file of the same length is caught.
    let mut other = body.clone();
    other[0] ^= 1;
    let srv = serve(other).await;
    let spec = spec_for_file(&body, srv.url.clone());
    let root = temp_root("file-bad");
    let m = manager(&root, &spec);
    assert_eq!(
        run_set(&m, SET_WAKE).await,
        Err(ModelError::ChecksumMismatch)
    );
    assert!(!m.wake_ready() && !root.join("wake/test").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_set_is_installed_and_reported_on_its_own() {
    let speech_a = archive(&good_items());
    let wake_a = archive_tgz(&[
        Item::File("top/LICENSE", LICENSE_TXT),
        Item::File("top/lib/libfoo.so.1.2", LIB),
    ]);
    let (s1, s2) = (serve(speech_a.clone()).await, serve(wake_a.clone()).await);
    let speech = spec_for(&speech_a, s1.url.clone());
    let wake = runtime_spec(&wake_a, s2.url.clone(), Format::Tgz, None);
    let root = temp_root("sets");
    let m = ModelManager::with(
        root.clone(),
        vec![speech, wake],
        http_client(true),
        Duration::from_secs(3),
    );
    assert!(!m.ready() && !m.wake_ready());
    // The wake set alone: speech is untouched, and nothing of it is requested.
    let seen: Seen = Arc::default();
    let s3 = seen.clone();
    m.install_set(SET_WAKE, &move |p| lock(&s3).push(p))
        .await
        .unwrap();
    assert!(m.wake_ready() && !m.ready());
    assert_eq!(
        s1.hits.load(Ordering::SeqCst),
        0,
        "the speech set was not fetched"
    );
    // Then the speech set, as the existing command does.
    assert_eq!(run(&m).await.0, Ok(()));
    assert!(m.ready() && m.wake_ready());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn only_the_downloads_for_this_system_are_considered() {
    let a = archive_tgz(&[Item::File("top/LICENSE", LICENSE_TXT)]);
    let mk = |id: &str, plat: Option<&str>| {
        let mut s = runtime_spec(&a, "http://x/y".into(), Format::Tgz, plat);
        s.id = id.into();
        s
    };
    let all = vec![
        mk("any", None),
        mk("win", Some("windows-x86_64")),
        mk("mac", Some("macos-aarch64")),
        mk("linux", Some("linux-x86_64")),
    ];
    let ids = |here: &str| -> Vec<String> {
        for_platform(&all, here).into_iter().map(|s| s.id).collect()
    };
    assert_eq!(ids("windows-x86_64"), ["any", "win"]);
    assert_eq!(ids("macos-aarch64"), ["any", "mac"]);
    assert_eq!(ids("linux-x86_64"), ["any", "linux"]);
    // A system with no download of its own gets only the shared ones, so the set is not "ready".
    assert_eq!(ids("other"), ["any"]);
    assert!(PLATFORMS.contains(&current_platform()) || current_platform() == "macos-x86_64");
}

#[test]
fn the_new_manifest_fields_are_checked_too() {
    let a = archive_tgz(&[Item::File("top/LICENSE", LICENSE_TXT)]);
    let https = "https://github.com/microsoft/onnxruntime/releases/download/v1/x.tgz".to_string();
    let ok = runtime_spec(&a, https.clone(), Format::Tgz, Some("windows-x86_64"));
    assert!(ok.validate(false).is_ok());
    let mut s = ok.clone();
    s.set = "other".into();
    assert!(s.validate(false).is_err());
    let mut s = ok.clone();
    s.platform = Some("plan9-mips".into());
    assert!(s.validate(false).is_err());
    for path in [
        "../lib/x",
        "/lib/x",
        "lib//x",
        "lib/../x",
        ".hidden/x",
        "a/b/c/d/e",
    ] {
        let mut s = ok.clone();
        s.files[1].path = Some(path.into());
        assert!(s.validate(false).is_err(), "{path}");
    }
    // The other approved projects are allowed; others are not.
    let mut s = ok.clone();
    s.url = "https://github.com/dscripka/openWakeWord/releases/download/v0.5.1/a.onnx".into();
    assert!(s.validate(false).is_ok());
    let mut s = ok.clone();
    s.url = "https://github.com/microsoft/onnxruntime/archive/refs/heads/main.zip".into();
    assert!(s.validate(false).is_err());
    let mut s = ok.clone();
    s.url = "https://github.com/someone-else/onnxruntime/releases/download/v1/x.tgz".into();
    assert!(s.validate(false).is_err());
    // A single-file download must be exactly its one file.
    let mut s = ok.clone();
    s.format = Format::File;
    assert!(
        s.validate(false).is_err(),
        "two files and a path in a single-file download"
    );
}

/// Needs the real downloads: `JARVIS_TEST_WAKE_DIR=<folder> cargo test -- --ignored`, where the
/// folder holds `ort-win/p.zip`, `ort-linux/p.tgz`, `ort-mac/p.tgz` and `oww/*.onnx`. Proves the
/// pinned checksums, paths and unpacking rules fit the real ONNX Runtime and openWakeWord files.
#[test]
#[ignore = "needs the real ONNX Runtime archives and openWakeWord files"]
fn the_real_wake_word_downloads_match_the_manifest_and_unpack() {
    let Ok(dir) = std::env::var("JARVIS_TEST_WAKE_DIR") else {
        panic!("set JARVIS_TEST_WAKE_DIR");
    };
    let dir = PathBuf::from(dir);
    let cancel = CancellationToken::new();
    let local = |id: &str| -> PathBuf {
        match id {
            "ort-windows-x86_64" => dir.join("ort-win/p.zip"),
            "ort-linux-x86_64" => dir.join("ort-linux/p.tgz"),
            "ort-macos-aarch64" => dir.join("ort-mac/p.tgz"),
            "wake-oww-melspectrogram" => dir.join("oww/melspectrogram.onnx"),
            "wake-oww-embedding" => dir.join("oww/embedding_model.onnx"),
            "wake-oww-hey-jarvis" => dir.join("oww/hey_jarvis_v0.1.onnx"),
            other => panic!("{other}"),
        }
    };
    for spec in manifest().iter().filter(|s| s.set == SET_WAKE) {
        let archive = local(&spec.id);
        assert_eq!(
            sha256_file(&archive, &cancel).unwrap(),
            spec.archive_sha256,
            "{}",
            spec.id
        );
        assert_eq!(
            std::fs::metadata(&archive).unwrap().len(),
            spec.archive_bytes,
            "{}",
            spec.id
        );
        let root = temp_root("real-wake");
        let stage = root.join("stage");
        std::fs::create_dir_all(&stage).unwrap();
        extract(&archive, &stage, spec, &cancel).unwrap_or_else(|e| panic!("{}: {e}", spec.id));
        put_in_place(&root, &stage, spec).unwrap();
        assert!(is_installed(&root, spec), "{}", spec.id);
        let _ = std::fs::remove_dir_all(root);
    }
}

/// A real download of the wake set for this system from github.com (about 15 MB on Linux, about
/// 85 MB on Windows). Needs internet.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads the wake word files from github.com"]
async fn a_real_download_of_the_wake_set_installs_it() {
    let root = temp_root("real-wake-download");
    let m = ModelManager::new(root.clone());
    assert!(!m.wake_ready());
    m.install_set(SET_WAKE, &|_| {})
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(m.wake_ready());
    let _ = std::fs::remove_dir_all(root);
}
