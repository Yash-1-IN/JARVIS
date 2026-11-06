//! Speech to text with Whisper through the `sherpa-onnx` crate (D-04, D-26, D-31).
//!
//! The model files are not part of the installer. They live in a folder the app is told about, and
//! are loaded the first time something is said (so a start-up never pays for the model, and an app
//! that never uses voice never holds it in memory). Until the files are there every request says
//! "The speech models are not installed yet."
//!
//! Runs only on the voice recognition thread (`VoiceController::transcribe_bounded`), never on a
//! Tokio worker, and never writes the audio anywhere.

use super::transcribe::{Transcriber, VoiceError};
use sherpa_onnx::{OfflineModelConfig, OfflineRecognizer, OfflineRecognizerConfig};
use std::path::{Path, PathBuf};

/// The sample rate the capture engine delivers and the model expects.
const RATE: i32 = 16_000;

/// Environment variable that asks for another ONNX Runtime execution provider (`coreml` on a Mac,
/// for example). Unset means `cpu`. It is an experiment switch for measuring on real hardware
/// (D-31); a provider that cannot be used makes the model load fail, and the engine then retries
/// on the CPU.
const PROVIDER_VAR: &str = "JARVIS_STT_PROVIDER";

/// First file in `dir` whose lower-case name contains every `has` part, none of `not`, and ends
/// with `ext`. An `int8` file wins over a full-precision one. The sherpa-onnx release archives
/// differ a little in file names between model versions, so names are matched by pattern.
fn pick(dir: &Path, has: &[&str], not: &[&str], ext: &str) -> Option<String> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .is_some_and(|n| {
                    n.ends_with(ext)
                        && has.iter().all(|h| n.contains(h))
                        && !not.iter().any(|x| n.contains(x))
                })
        })
        .collect();
    found.sort_by_key(|p| {
        let int8 = p.to_string_lossy().to_lowercase().contains("int8");
        (!int8, p.clone())
    });
    found
        .into_iter()
        .next()
        .map(|p| p.to_string_lossy().into_owned())
}

/// The Whisper files found in a folder, before anything is loaded.
#[derive(Debug, PartialEq, Eq)]
struct Files {
    tokens: String,
    encoder: String,
    decoder: String,
}

/// Finds the files, or says which kind of problem it is: no folder or no tokens file means the
/// models are not installed; a folder with some files but not a whole model is a failure the
/// person can fix by downloading again.
fn find_files(dir: &Path) -> Result<Files, VoiceError> {
    if !dir.is_dir() {
        return Err(VoiceError::ModelMissing);
    }
    let Some(tokens) = pick(dir, &["tokens"], &[], ".txt") else {
        return Err(VoiceError::ModelMissing);
    };
    let incomplete =
        || VoiceError::Failed("the speech model folder is incomplete; download it again".into());
    Ok(Files {
        tokens,
        encoder: pick(dir, &["encoder"], &[], ".onnx").ok_or_else(incomplete)?,
        decoder: pick(dir, &["decoder"], &[], ".onnx").ok_or_else(incomplete)?,
    })
}

fn config(f: &Files, threads: i32, provider: &str) -> OfflineRecognizerConfig {
    let mut m = OfflineModelConfig {
        num_threads: threads,
        provider: Some(provider.into()),
        tokens: Some(f.tokens.clone()),
        ..Default::default()
    };
    m.whisper.encoder = Some(f.encoder.clone());
    m.whisper.decoder = Some(f.decoder.clone());
    // The English-only model: no language detection pass, and transcribe rather than translate.
    m.whisper.language = Some("en".into());
    m.whisper.task = Some("transcribe".into());
    OfflineRecognizerConfig {
        model_config: m,
        ..Default::default()
    }
}

/// Whisper, loaded on first use from `dir`.
pub struct SherpaTranscriber {
    dir: PathBuf,
    threads: i32,
    recogniser: Option<OfflineRecognizer>,
}

impl SherpaTranscriber {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            // Whisper small is heavier than the model it replaced: four threads keep a short
            // utterance to a couple of seconds on a laptop.
            threads: 4,
            recogniser: None,
        }
    }

    fn load(&self) -> Result<OfflineRecognizer, VoiceError> {
        let files = find_files(&self.dir)?;
        let wanted = std::env::var(PROVIDER_VAR)
            .ok()
            .map(|v| v.trim().to_lowercase())
            .filter(|v| !v.is_empty() && v != "cpu");
        if let Some(p) = wanted {
            if let Some(rec) = OfflineRecognizer::create(&config(&files, self.threads, &p)) {
                tracing::info!(provider = %p, "speech model loaded");
                return Ok(rec);
            }
            tracing::warn!(provider = %p, "speech provider not usable; using the CPU");
        }
        OfflineRecognizer::create(&config(&files, self.threads, "cpu"))
            .ok_or_else(|| VoiceError::Failed("the speech model could not be loaded".into()))
    }
}

impl Transcriber for SherpaTranscriber {
    fn transcribe(&mut self, audio: &[f32]) -> Result<String, VoiceError> {
        if self.recogniser.is_none() {
            // A model that is not there yet is checked again on every request, so installing it
            // does not need a restart.
            self.recogniser = Some(self.load()?);
        }
        let Some(rec) = self.recogniser.as_ref() else {
            return Err(VoiceError::ModelMissing);
        };
        let stream = rec.create_stream();
        stream.accept_waveform(RATE, audio);
        rec.decode(&stream);
        Ok(stream
            .get_result()
            .map(|r| r.text.trim().to_owned())
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jarvis-stt-{tag}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(d: &Path, names: &[&str]) {
        for n in names {
            std::fs::write(d.join(n), b"x").unwrap();
        }
    }

    #[test]
    fn no_folder_or_no_tokens_means_the_models_are_not_installed() {
        let missing = std::env::temp_dir().join("jarvis-stt-does-not-exist");
        assert_eq!(find_files(&missing), Err(VoiceError::ModelMissing));
        let empty = dir("empty");
        assert_eq!(find_files(&empty), Err(VoiceError::ModelMissing));
        let mut t = SherpaTranscriber::new(missing);
        assert_eq!(t.transcribe(&[0.0; 160]), Err(VoiceError::ModelMissing));
        let _ = std::fs::remove_dir_all(empty);
    }

    #[test]
    fn a_partly_downloaded_model_is_a_failure_that_says_so_not_a_crash() {
        let d = dir("partial");
        touch(&d, &["small.en-tokens.txt", "small.en-encoder.int8.onnx"]);
        match find_files(&d) {
            Err(VoiceError::Failed(m)) => assert!(m.contains("incomplete")),
            other => panic!("{other:?}"),
        }
        let mut t = SherpaTranscriber::new(d.clone());
        assert!(matches!(
            t.transcribe(&[0.0; 160]),
            Err(VoiceError::Failed(_))
        ));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn the_whisper_files_are_found_and_int8_wins() {
        let d = dir("whisper");
        touch(
            &d,
            &[
                "small.en-tokens.txt",
                "small.en-encoder.onnx",
                "small.en-encoder.int8.onnx",
                "small.en-decoder.onnx",
                "small.en-decoder.int8.onnx",
            ],
        );
        let f = find_files(&d).unwrap();
        assert!(
            f.encoder.ends_with("small.en-encoder.int8.onnx"),
            "{}",
            f.encoder
        );
        assert!(
            f.decoder.ends_with("small.en-decoder.int8.onnx"),
            "{}",
            f.decoder
        );
        assert!(f.tokens.ends_with("small.en-tokens.txt"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn the_config_asks_for_english_transcription_on_the_chosen_provider() {
        let f = Files {
            tokens: "t.txt".into(),
            encoder: "e.onnx".into(),
            decoder: "d.onnx".into(),
        };
        let c = config(&f, 4, "coreml");
        assert_eq!(c.model_config.provider.as_deref(), Some("coreml"));
        assert_eq!(c.model_config.whisper.language.as_deref(), Some("en"));
        assert_eq!(c.model_config.whisper.task.as_deref(), Some("transcribe"));
        assert_eq!(c.model_config.num_threads, 4);
    }

    /// Needs the real model: `JARVIS_TEST_WHISPER_DIR=<folder> cargo test -- --ignored`.
    /// A second of silence must give an answer (empty text is fine), not an error or a crash.
    #[test]
    #[ignore = "needs the Whisper model files"]
    fn the_real_model_answers_for_silence() {
        let Ok(d) = std::env::var("JARVIS_TEST_WHISPER_DIR") else {
            panic!("set JARVIS_TEST_WHISPER_DIR to the model folder");
        };
        let mut t = SherpaTranscriber::new(PathBuf::from(d));
        assert!(t.transcribe(&vec![0.0; 16_000]).is_ok());
    }

    /// The model's own sample recording, `test_wavs/0.wav` (16 kHz, 16-bit mono), against the
    /// sentence it ships with in `test_wavs/trans.txt`. Same environment variable.
    #[test]
    #[ignore = "needs the Whisper model files"]
    fn the_real_model_hears_its_own_sample_sentence() {
        let Ok(d) = std::env::var("JARVIS_TEST_WHISPER_DIR") else {
            panic!("set JARVIS_TEST_WHISPER_DIR to the model folder");
        };
        let d = PathBuf::from(d);
        let bytes = std::fs::read(d.join("test_wavs/0.wav")).unwrap();
        // A plain 44-byte header, then 16-bit little-endian samples.
        let pcm = &bytes[44..];
        let audio: Vec<f32> = (0..pcm.len() / 2)
            .map(|i| i16::from_le_bytes([pcm[2 * i], pcm[2 * i + 1]]) as f32 / 32768.0)
            .collect();
        let expected = std::fs::read_to_string(d.join("test_wavs/trans.txt")).unwrap();
        let expected = expected
            .lines()
            .next()
            .unwrap()
            .split_once(' ')
            .map(|(_, t)| t)
            .unwrap_or_default()
            .to_lowercase();
        let mut t = SherpaTranscriber::new(d);
        let got = t.transcribe(&audio).unwrap().to_lowercase();
        let words = |s: &str| {
            s.split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        assert_eq!(words(&got), words(&expected), "heard {got:?}");
    }
}
