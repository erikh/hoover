pub mod openai;
pub mod vosk;
pub mod whisper;

use crate::audio::buffer::AudioChunk;
use crate::config::SttConfig;
use crate::error::{HooverError, Result};

/// A single segment of transcribed speech.
#[derive(Debug, Clone)]
pub struct TranscriptionSegment {
    pub text: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub duration_secs: f32,
    pub confidence: Option<f32>,
}

/// Trait for speech-to-text backends.
pub trait SttEngine: Send {
    fn transcribe(&mut self, chunk: &AudioChunk) -> Result<Vec<TranscriptionSegment>>;
    fn name(&self) -> &str;
}

/// Create an STT engine based on the config backend name.
pub fn create_engine(config: &SttConfig) -> Result<Box<dyn SttEngine>> {
    match config.backend.as_str() {
        "whisper" => Ok(Box::new(whisper::WhisperEngine::new(config)?)),
        "vosk" => Ok(Box::new(vosk::VoskEngine::new(config)?)),
        "openai" => Ok(Box::new(openai::OpenAiEngine::new(config)?)),
        other => Err(HooverError::Stt(format!(
            "unknown STT backend: {other} (available: whisper, vosk, openai)"
        ))),
    }
}
