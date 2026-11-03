//! openWakeWord's pretrained "hey jarvis" model, run through ONNX Runtime (spike only).
//!
//! The model files are downloaded by the user and never committed: they are licensed
//! CC BY-NC-SA 4.0 (personal, non-commercial use, with credit to openWakeWord by David Scripka).
//! Decision D-23 records that this project may use them privately and nothing else.
//!
//! Pipeline (as in openWakeWord's `AudioFeatures`): 16 kHz audio in 1280-sample hops (80 ms) ->
//! mel-spectrogram model over the hop plus 480 samples of context (8 new frames) -> scaled
//! `x / 10 + 2` -> embedding model over the last 76 frames (a 96-number vector) -> classifier over
//! the last 16 embeddings (about 1.3 s) -> one score between 0 and 1 per hop.

use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const HOP: usize = 1280;
const CONTEXT: usize = 480;
const MEL_BANDS: usize = 32;
const MEL_WINDOW: usize = 76;
const EMBED: usize = 96;
const CLASSIFIER_WINDOW: usize = 16;
/// Silence fed before a take so the buffers are full when the phrase starts, as they would be in a
/// program that has been listening all along.
pub const WARM_UP: usize = HOP * 24;

static RUNTIME: OnceLock<Result<Environment, String>> = OnceLock::new();

fn library_name() -> &'static str {
    if cfg!(windows) {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    }
}

/// Loads ONNX Runtime from the file next to the program (or from `ORT_DYLIB_PATH`) once.
fn runtime() -> Result<&'static Environment, String> {
    RUNTIME
        .get_or_init(|| {
            let mut candidates: Vec<PathBuf> = Vec::new();
            if let Ok(p) = std::env::var("ORT_DYLIB_PATH") {
                candidates.push(PathBuf::from(p));
            }
            if let Some(dir) = std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(Path::to_path_buf))
            {
                candidates.push(dir.join(library_name()));
            }
            for c in &candidates {
                if c.exists() {
                    return ort::init_from(c)
                        .map_err(|e| format!("cannot load {}: {e}", c.display()))?
                        .build()
                        .map_err(|e| format!("cannot start ONNX Runtime: {e}"));
                }
            }
            Err(format!(
                "ONNX Runtime not found. Put {} next to voice-cli, or set ORT_DYLIB_PATH to it.",
                library_name()
            ))
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn load(env: &Environment, path: &Path) -> Result<Session, String> {
    Session::builder(env)
        .and_then(|mut b| b.commit_from_file(path))
        .map_err(|e| format!("cannot load {}: {e}", path.display()))
}

/// Runs a session with one `f32` input of the given shape and returns its first output, flat.
fn run(s: &mut Session, shape: Vec<i64>, data: Vec<f32>) -> Result<Vec<f32>, String> {
    let name = s.inputs()[0].name().to_string();
    let out_name = s.outputs()[0].name().to_string();
    let tensor = Tensor::from_array((shape, data)).map_err(|e| e.to_string())?;
    let outputs = s
        .run(ort::inputs![name.as_str() => tensor])
        .map_err(|e| e.to_string())?;
    let (_, values) = outputs[out_name.as_str()]
        .try_extract_tensor::<f32>()
        .map_err(|e| e.to_string())?;
    Ok(values.to_vec())
}

pub struct Detector {
    mel: Session,
    embedding: Session,
    classifier: Session,
    pending: Vec<f32>,
    raw: VecDeque<f32>,
    frames: VecDeque<[f32; MEL_BANDS]>,
    features: VecDeque<[f32; EMBED]>,
}

impl Detector {
    /// `dir` holds `melspectrogram.onnx`, `embedding_model.onnx` and the wake-word model `model`.
    pub fn load(dir: &Path, model: &str) -> Result<Self, String> {
        let env = runtime()?;
        let mut d = Self {
            mel: load(env, &dir.join("melspectrogram.onnx"))?,
            embedding: load(env, &dir.join("embedding_model.onnx"))?,
            classifier: load(env, &dir.join(model))?,
            pending: Vec::new(),
            raw: VecDeque::new(),
            frames: VecDeque::new(),
            features: VecDeque::new(),
        };
        d.reset();
        Ok(d)
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        self.raw = std::iter::repeat_n(0.0, CONTEXT).collect();
        self.frames.clear();
        self.features.clear();
    }

    /// Feeds audio (16 kHz, -1..1) and returns one score per complete 80 ms hop. A score is
    /// produced only once enough audio has been heard (about 2 s after a reset).
    pub fn feed(&mut self, samples: &[f32]) -> Result<Vec<f32>, String> {
        self.pending.extend_from_slice(samples);
        let mut scores = Vec::new();
        while self.pending.len() >= HOP {
            let hop: Vec<f32> = self.pending.drain(..HOP).collect();
            self.raw.extend(hop);
            while self.raw.len() > HOP + CONTEXT {
                self.raw.pop_front();
            }
            // The mel model takes raw 16-bit sample values as floats, not -1..1.
            let input: Vec<f32> = self.raw.iter().map(|s| s * 32767.0).collect();
            let n = input.len() as i64;
            let mel = run(&mut self.mel, vec![1, n], input)?;
            for f in mel.as_chunks::<MEL_BANDS>().0 {
                let mut row = [0.0f32; MEL_BANDS];
                for (r, v) in row.iter_mut().zip(f) {
                    *r = v / 10.0 + 2.0;
                }
                self.frames.push_back(row);
            }
            while self.frames.len() > 400 {
                self.frames.pop_front();
            }
            if self.frames.len() >= MEL_WINDOW {
                let window: Vec<f32> = self
                    .frames
                    .iter()
                    .skip(self.frames.len() - MEL_WINDOW)
                    .flat_map(|f| f.iter().copied())
                    .collect();
                let e = run(
                    &mut self.embedding,
                    vec![1, MEL_WINDOW as i64, MEL_BANDS as i64, 1],
                    window,
                )?;
                let mut row = [0.0f32; EMBED];
                if e.len() < EMBED {
                    return Err(format!("embedding model returned {} numbers", e.len()));
                }
                row.copy_from_slice(&e[..EMBED]);
                self.features.push_back(row);
                while self.features.len() > 64 {
                    self.features.pop_front();
                }
            }
            if self.features.len() >= CLASSIFIER_WINDOW {
                let window: Vec<f32> = self
                    .features
                    .iter()
                    .skip(self.features.len() - CLASSIFIER_WINDOW)
                    .flat_map(|f| f.iter().copied())
                    .collect();
                let out = run(
                    &mut self.classifier,
                    vec![1, CLASSIFIER_WINDOW as i64, EMBED as i64],
                    window,
                )?;
                scores.push(out.first().copied().unwrap_or(0.0));
            }
        }
        Ok(scores)
    }
}

/// Every score the model gives for a recording. With `warm` the model first hears a stretch of
/// silence, so the first words of the recording are scored as they would be by a program that has
/// been listening all along.
pub fn scores(d: &mut Detector, samples: &[f32], warm: bool) -> Result<Vec<f32>, String> {
    d.reset();
    if warm {
        d.feed(&vec![0.0; WARM_UP])?;
    }
    let mut out = Vec::new();
    let tail = vec![0.0f32; HOP * 6];
    for chunk in samples
        .chunks(HOP * 8)
        .chain(std::iter::once(tail.as_slice()))
    {
        out.extend(d.feed(chunk)?);
    }
    Ok(out)
}

pub fn peak(scores: &[f32]) -> f32 {
    scores.iter().copied().fold(0.0, f32::max)
}

/// Times the score rises to `threshold` or more, counting a new event only after
/// `refractory_hops` hops below it (so one burst is one event).
pub fn events(scores: &[f32], threshold: f32, refractory_hops: usize) -> usize {
    let (mut n, mut quiet) = (0usize, refractory_hops);
    for &s in scores {
        if s >= threshold {
            if quiet >= refractory_hops {
                n += 1;
            }
            quiet = 0;
        } else {
            quiet = quiet.saturating_add(1);
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_count_bursts_not_hops() {
        let s = [0.0, 0.9, 0.95, 0.8, 0.0, 0.0, 0.0, 0.7, 0.0];
        assert_eq!(events(&s, 0.5, 2), 2);
        assert_eq!(
            events(&s, 0.5, 5),
            1,
            "the second burst is too soon after the first"
        );
        assert_eq!(events(&s, 0.99, 2), 0);
        assert_eq!(events(&[], 0.5, 2), 0);
        assert_eq!(peak(&s), 0.95);
        assert_eq!(peak(&[]), 0.0);
    }
}
