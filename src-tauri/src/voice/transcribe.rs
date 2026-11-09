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

/// Brings a quiet recording up to a level the model hears well. A laptop microphone array speaks at
/// an RMS near 0.02, a third of what Whisper is happiest with, and that was the likeliest reason it
/// missed words (the Windows test of 2026-11-09). The gain is taken from the loudest third of the
/// 20 ms frames, so a pause does not pull it up, it is at most 6 times, and it never clips. Audio
/// with no clear speech in it (a level below `FLOOR`) is returned as it is: noise is not amplified.
/// Returns the audio and the gain applied (1.0 when none).
pub fn with_speech_level(audio: &[f32]) -> (Vec<f32>, f32) {
    const FRAME: usize = 320;
    const TARGET: f32 = 0.06;
    const MAX_GAIN: f32 = 6.0;
    const FLOOR: f32 = 0.004;
    let mut levels: Vec<f32> = audio
        .chunks_exact(FRAME)
        .map(|f| (f.iter().map(|s| s * s).sum::<f32>() / FRAME as f32).sqrt())
        .collect();
    if levels.is_empty() {
        return (audio.to_vec(), 1.0);
    }
    levels.sort_by(|a, b| b.total_cmp(a));
    let top = &levels[..levels.len().div_ceil(3)];
    let speech = top.iter().sum::<f32>() / top.len() as f32;
    if speech < FLOOR {
        return (audio.to_vec(), 1.0);
    }
    let peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let gain = (TARGET / speech)
        .min(MAX_GAIN)
        .min(0.95 / peak.max(f32::EPSILON));
    if gain <= 1.05 {
        return (audio.to_vec(), 1.0);
    }
    (audio.iter().map(|s| s * gain).collect(), gain)
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
    fn quiet_speech_is_brought_up_and_loud_speech_is_left_alone() {
        let quiet = tone(2.0, 0.02); // the owner's microphone: an RMS of about 0.02
        let (out, gain) = with_speech_level(&quiet);
        assert!(gain > 2.0 && gain <= 6.0, "{gain}");
        assert!(out.iter().all(|s| s.abs() <= 0.95));
        let loud = tone(2.0, 0.3);
        let (same, g) = with_speech_level(&loud);
        assert_eq!(g, 1.0);
        assert_eq!(same, loud);
    }

    #[test]
    fn noise_and_silence_are_not_amplified_and_the_gain_never_clips() {
        let (_, g) = with_speech_level(&tone(2.0, 0.003));
        assert_eq!(g, 1.0);
        assert_eq!(with_speech_level(&[]).1, 1.0);
        // A quiet speaker with one loud click: the click limits the gain, nothing clips.
        let mut a = tone(2.0, 0.02);
        a[100] = 0.5;
        let (out, _) = with_speech_level(&a);
        assert!(out.iter().all(|s| s.abs() <= 0.951));
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
