//! Speech to text. The real model (Moonshine through sherpa-onnx, D-04) arrives with its own
//! dependency decision; until then the app has [`MissingModels`], which says so plainly.

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

/// What the app uses until the models are installed: every request says so.
pub struct MissingModels;

impl Transcriber for MissingModels {
    fn transcribe(&mut self, _audio: &[f32]) -> Result<String, VoiceError> {
        Err(VoiceError::ModelMissing)
    }
}
