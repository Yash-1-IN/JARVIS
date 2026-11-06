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
}

/// A stand-in that says the models are missing, for tests of the pipeline without a model.
#[cfg_attr(not(test), allow(dead_code))]
pub struct MissingModels;

impl Transcriber for MissingModels {
    fn transcribe(&mut self, _audio: &[f32]) -> Result<String, VoiceError> {
        Err(VoiceError::ModelMissing)
    }
}
