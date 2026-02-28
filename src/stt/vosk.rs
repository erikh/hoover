use vosk::{Model, Recognizer};

use crate::audio::buffer::AudioChunk;
use crate::config::SttConfig;
use crate::error::{HooverError, Result};

use super::{SttEngine, TranscriptionSegment};

pub struct VoskEngine {
    recognizer: Recognizer,
}

impl VoskEngine {
    pub fn new(config: &SttConfig) -> Result<Self> {
        let model_path = config.model_path.as_ref().ok_or_else(|| {
            HooverError::Stt("vosk requires stt.model_path to be set in config".to_string())
        })?;

        let path = crate::config::Config::expand_path(model_path);

        let model =
            Model::new(path.to_str().ok_or_else(|| {
                HooverError::Stt("model path contains invalid UTF-8".to_string())
            })?)
            .ok_or_else(|| HooverError::Stt("failed to load vosk model".to_string()))?;

        let recognizer = Recognizer::new(&model, 16000.0)
            .ok_or_else(|| HooverError::Stt("failed to create vosk recognizer".to_string()))?;

        Ok(Self { recognizer })
    }
}

impl SttEngine for VoskEngine {
    fn transcribe(&mut self, chunk: &AudioChunk) -> Result<Vec<TranscriptionSegment>> {
        let _ = self.recognizer.accept_waveform(&chunk.samples_i16);
        let result = self.recognizer.final_result();

        let text = result
            .single()
            .map_or_else(String::new, |r| r.text.to_string());
        let text = text.trim().to_string();

        if text.is_empty() {
            return Ok(Vec::new());
        }

        Ok(vec![TranscriptionSegment {
            text,
            timestamp: chunk.timestamp,
            duration_secs: chunk.duration_secs,
            confidence: None,
        }])
    }

    fn name(&self) -> &'static str {
        "vosk"
    }
}
