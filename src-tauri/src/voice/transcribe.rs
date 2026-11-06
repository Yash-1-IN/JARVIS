//! Speech to text. The real model is Whisper through sherpa-onnx (`sherpa.rs`, D-04, D-26, D-31); this
//! file holds the trait it implements and the errors the pipeline understands.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VoiceError {
    #[error("the speech models are not installed")]
    ModelMissing,
    #[error("speech recognition took too long")]
    TimedOut,
    #[error("speech recognition failed: {0}")]
    Failed(String),
}

/// Turns 16 kHz mono audio into text. Runs on its own thread, never on a Tokio worker, and a call
/// is bounded by a timeout in the controller.
pub trait Transcriber: Send + 'static {
    fn transcribe(&mut self, audio: &[f32]) -> Result<String, VoiceError>;

    /// Loads the model ahead of the first request, so the first sentence does not pay for it inside
    /// the time limit. Never an error: a model that is not there yet is simply loaded later.
    fn warm_up(&mut self) {}
}

/// Speech models sometimes invent a stock phrase for silence or noise ("Thank you.", "you",
/// "[BLANK_AUDIO]"). Returns the text to use: empty when the text is a bracketed tag, or one of
/// those phrases heard in audio that was short or quiet. A real "thank you" spoken at a normal
/// level and length is kept. Pure; the caller logs the fact, never the words.
pub fn without_invented_text(text: &str, audio: &[f32]) -> String {
    const PHRASES: &[&str] = &[
        "you",
        "thank you",
        "thanks",
        "thanks for watching",
        "thank you for watching",
        "bye",
        "bye bye",
        "okay",
        "ok",
        "so",
        "uh",
        "um",
        "hmm",
    ];
    let t = text.trim();
    let bracketed =
        (t.starts_with('[') && t.ends_with(']')) || (t.starts_with('(') && t.ends_with(')'));
    if t.is_empty() || bracketed {
        return String::new();
    }
    let norm: String = t
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    let norm = norm.split_whitespace().collect::<Vec<_>>().join(" ");
    if !PHRASES.contains(&norm.as_str()) {
        return t.to_owned();
    }
    let rms = if audio.is_empty() {
        0.0
    } else {
        (audio.iter().map(|s| s * s).sum::<f32>() / audio.len() as f32).sqrt()
    };
    let seconds = audio.len() as f32 / 16_000.0;
    if rms < QUIET_RMS || seconds < SHORT_SECONDS {
        String::new()
    } else {
        t.to_owned()
    }
}

/// Average level (0 to 1) below which a stock phrase is taken as invented.
const QUIET_RMS: f32 = 0.01;
/// A stock phrase in less audio than this is taken as invented.
const SHORT_SECONDS: f32 = 0.6;

/// A stand-in that says the models are missing, for tests of the pipeline without a model.
#[cfg_attr(not(test), allow(dead_code))]
pub struct MissingModels;

impl Transcriber for MissingModels {
    fn transcribe(&mut self, _audio: &[f32]) -> Result<String, VoiceError> {
        Err(VoiceError::ModelMissing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f32, level: f32) -> Vec<f32> {
        (0..(seconds * 16_000.0) as usize)
            .map(|i| if i % 2 == 0 { level } else { -level })
            .collect()
    }

    #[test]
    fn a_stock_phrase_in_quiet_audio_is_dropped() {
        assert_eq!(without_invented_text("Thank you.", &tone(3.0, 0.002)), "");
        assert_eq!(without_invented_text(" you ", &tone(2.0, 0.0)), "");
    }

    #[test]
    fn a_stock_phrase_in_very_short_audio_is_dropped() {
        assert_eq!(without_invented_text("Thank you.", &tone(0.3, 0.2)), "");
    }

    #[test]
    fn the_same_words_at_a_normal_level_and_length_are_kept() {
        assert_eq!(
            without_invented_text("Thank you.", &tone(1.5, 0.1)),
            "Thank you."
        );
    }

    #[test]
    fn real_sentences_are_never_touched() {
        let quiet = tone(3.0, 0.002);
        assert_eq!(
            without_invented_text("  open the calendar ", &quiet),
            "open the calendar"
        );
    }

    #[test]
    fn bracketed_tags_and_empty_text_are_dropped() {
        let a = tone(2.0, 0.1);
        assert_eq!(without_invented_text("[BLANK_AUDIO]", &a), "");
        assert_eq!(without_invented_text("(silence)", &a), "");
        assert_eq!(without_invented_text("   ", &a), "");
    }
}
