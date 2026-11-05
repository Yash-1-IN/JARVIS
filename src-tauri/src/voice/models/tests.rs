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
        default: true,
        url,
        archive_bytes: archive.len() as u64,
        archive_sha256: sha(archive),
        dir: "stt/test".into(),
        files: vec![
            FileSpec {
                name: "tokens.txt".into(),
                bytes: TOKENS.len() as u64,
                sha256: sha(TOKENS),
            },
            FileSpec {
                name: "model.onnx".into(),
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
    let mut default_total = 0;
    for s in m {
        s.validate(false).unwrap();
        assert!(
            s.url
                .starts_with("https://github.com/k2-fsa/sherpa-onnx/releases/download/")
        );
        assert!(is_hex64(&s.archive_sha256));
        assert!(s.files.iter().all(|f| is_hex64(&f.sha256)));
        // The licence file of a model travels with it.
        assert!(s.files.iter().any(|f| f.name == "LICENSE"), "{}", s.id);
        if s.default {
            default_total += s.archive_bytes;
        }
    }
    assert!(
        default_total <= DEFAULT_SET_LIMIT,
        "default set is {default_total} bytes"
    );
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
    let spec = manifest().iter().find(|s| s.default).unwrap().clone();
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
