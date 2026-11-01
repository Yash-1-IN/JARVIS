//! Builds `sherpa-onnx` configs from model folders. File names are found by pattern, so the
//! spike works with the folders as downloaded from the sherpa-onnx release pages.

use sherpa_onnx::*;
use std::path::{Path, PathBuf};

/// First file in `dir` whose name contains every `has` part, none of `not`, and ends with
/// `ext`. An `int8` file wins over a full-precision one.
fn pick(dir: &Path, has: &[&str], not: &[&str], ext: &str) -> Result<String, String> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase());
            n.is_some_and(|n| {
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
        .ok_or_else(|| format!("no file matching {has:?}*{ext} in {}", dir.display()))
}

pub fn vad(model: &str, min_silence: f32) -> Result<VoiceActivityDetector, String> {
    let cfg = VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model.into()),
            threshold: 0.5,
            min_silence_duration: min_silence,
            min_speech_duration: 0.25,
            window_size: 512,
            max_speech_duration: 15.0, // spec §6.2: hard cap per request
        },
        sample_rate: 16_000,
        num_threads: 1,
        ..Default::default()
    };
    VoiceActivityDetector::create(&cfg, 30.0)
        .ok_or_else(|| format!("cannot load the VAD model {model}"))
}

/// `kind` is `moonshine` or `whisper`.
pub fn asr(kind: &str, dir: &Path, threads: i32) -> Result<OfflineRecognizer, String> {
    let mut m = OfflineModelConfig {
        num_threads: threads,
        tokens: Some(pick(dir, &["tokens"], &[], ".txt")?),
        ..Default::default()
    };
    match kind {
        "whisper" => {
            m.whisper.encoder = Some(pick(dir, &["encoder"], &[], ".onnx")?);
            m.whisper.decoder = Some(pick(dir, &["decoder"], &[], ".onnx")?);
            m.whisper.language = Some("en".into());
            m.whisper.task = Some("transcribe".into());
        }
        "moonshine" => {
            if let Ok(merged) = pick(dir, &["merged"], &[], ".onnx") {
                m.moonshine.encoder = Some(pick(dir, &["encode"], &["merged"], ".onnx")?);
                m.moonshine.merged_decoder = Some(merged);
            } else {
                m.moonshine.preprocessor = Some(pick(dir, &["preprocess"], &[], ".onnx")?);
                m.moonshine.encoder = Some(pick(dir, &["encode"], &[], ".onnx")?);
                m.moonshine.uncached_decoder = Some(pick(dir, &["uncached"], &[], ".onnx")?);
                m.moonshine.cached_decoder = Some(pick(dir, &["cached"], &["uncached"], ".onnx")?);
            }
        }
        other => return Err(format!("unknown --asr {other:?}; use moonshine or whisper")),
    }
    let cfg = OfflineRecognizerConfig {
        model_config: m,
        ..Default::default()
    };
    OfflineRecognizer::create(&cfg)
        .ok_or_else(|| format!("cannot load the {kind} model in {}", dir.display()))
}

pub fn kws(
    dir: &Path,
    keywords_file: &str,
    threshold: f32,
    score: f32,
) -> Result<KeywordSpotter, String> {
    let mut cfg = KeywordSpotterConfig {
        keywords_file: Some(keywords_file.into()),
        keywords_threshold: threshold,
        keywords_score: score,
        ..Default::default()
    };
    cfg.model_config.transducer.encoder = Some(pick(dir, &["encoder"], &[], ".onnx")?);
    cfg.model_config.transducer.decoder = Some(pick(dir, &["decoder"], &[], ".onnx")?);
    cfg.model_config.transducer.joiner = Some(pick(dir, &["joiner"], &[], ".onnx")?);
    cfg.model_config.tokens = Some(pick(dir, &["tokens"], &[], ".txt")?);
    cfg.model_config.num_threads = 1;
    KeywordSpotter::create(&cfg)
        .ok_or_else(|| format!("cannot load the wake-word model in {}", dir.display()))
}

/// Kokoro in lexicon mode (sherpa-onnx 2.x no longer uses espeak-ng; the language comes from
/// the lexicon files). An `espeak-ng-data` folder (GPL-3.0) is deliberately never passed.
pub fn tts(dir: &Path) -> Result<OfflineTts, String> {
    let mut lexicons: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .is_some_and(|n| n.starts_with("lexicon") && n.ends_with(".txt"))
        })
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    lexicons.sort();
    if lexicons.is_empty() {
        return Err(format!("no lexicon*.txt in {}", dir.display()));
    }
    let mut cfg = OfflineTtsConfig::default();
    cfg.model.kokoro.model = Some(pick(dir, &["model"], &[], ".onnx")?);
    cfg.model.kokoro.voices = Some(pick(dir, &["voices"], &[], ".bin")?);
    cfg.model.kokoro.tokens = Some(pick(dir, &["tokens"], &[], ".txt")?);
    cfg.model.kokoro.lexicon = Some(lexicons.join(","));
    cfg.model.num_threads = 2;
    OfflineTts::create(&cfg)
        .ok_or_else(|| format!("cannot load the voice model in {}", dir.display()))
}
