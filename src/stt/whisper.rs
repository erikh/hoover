use std::path::PathBuf;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::buffer::AudioChunk;
use crate::config::SttConfig;
use crate::error::{HooverError, Result};

use super::{SttEngine, TranscriptionSegment};

/// Segments with `no_speech` probability above this threshold are discarded.
const NO_SPEECH_THRESHOLD: f32 = 0.6;

/// Common Whisper hallucinations from non-speech audio (keyboard tapping, etc.).
fn is_hallucinated_noise(text: &str) -> bool {
    let lower = text.to_lowercase();
    // Whisper tends to hallucinate these from percussive/mechanical sounds
    lower.starts_with('[') && lower.ends_with(']')
        || lower.starts_with('(') && lower.ends_with(')')
        || lower.contains("thank you")
            && lower.len() < 30
        || lower.contains("thanks for watching")
        || lower.contains("subscribe")
}

pub struct WhisperEngine {
    ctx: WhisperContext,
    language: String,
}

impl WhisperEngine {
    pub fn new(config: &SttConfig) -> Result<Self> {
        let model_path = resolve_model_path(config)?;

        let ctx = WhisperContext::new_with_params(
            model_path
                .to_str()
                .ok_or_else(|| HooverError::Stt("model path contains invalid UTF-8".to_string()))?,
            WhisperContextParameters::default(),
        )
        .map_err(|e| HooverError::Stt(format!("failed to load whisper model: {e}")))?;

        Ok(Self {
            ctx,
            language: config.language.clone(),
        })
    }
}

impl SttEngine for WhisperEngine {
    fn transcribe(&mut self, chunk: &AudioChunk) -> Result<Vec<TranscriptionSegment>> {
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| HooverError::Stt(format!("failed to create whisper state: {e}")))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(&self.language));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state
            .full(params, &chunk.samples_f32)
            .map_err(|e| HooverError::Stt(format!("whisper transcription failed: {e}")))?;

        let n_segments = state.full_n_segments();

        let mut segments = Vec::new();
        for i in 0..n_segments {
            let segment = state
                .get_segment(i)
                .ok_or_else(|| HooverError::Stt(format!("segment {i} out of bounds")))?;

            let no_speech_prob = segment.no_speech_probability();
            if no_speech_prob > NO_SPEECH_THRESHOLD {
                tracing::debug!("skipping segment {i}: no_speech_prob={no_speech_prob:.2}");
                continue;
            }

            let text = segment
                .to_str()
                .map_err(|e| HooverError::Stt(format!("failed to get segment text: {e}")))?
                .trim()
                .to_string();

            if text.is_empty() || is_hallucinated_noise(&text) {
                continue;
            }

            let start_ts = segment.start_timestamp();
            let end_ts = segment.end_timestamp();

            // Whisper timestamps are in centiseconds (10ms units)
            let duration_secs = (end_ts - start_ts) as f32 / 100.0;

            // Offset the chunk timestamp by the segment start time
            let segment_ts = chunk.timestamp + chrono::Duration::milliseconds(start_ts * 10);

            segments.push(TranscriptionSegment {
                text,
                timestamp: segment_ts,
                duration_secs,
                confidence: None,
            });
        }

        Ok(segments)
    }

    fn name(&self) -> &'static str {
        "whisper"
    }
}

fn resolve_model_path(config: &SttConfig) -> Result<PathBuf> {
    if let Some(ref explicit) = config.model_path {
        let path = crate::config::Config::expand_path(explicit);
        if !path.exists() {
            return Err(HooverError::Stt(format!(
                "whisper model not found at {}",
                path.display()
            )));
        }
        return Ok(path);
    }

    // Auto-resolve from model_size
    let data_dir = dirs::data_dir()
        .ok_or_else(|| HooverError::Stt("could not determine data directory".to_string()))?;

    let model_file = format!("ggml-{}.en.bin", config.whisper_model_size);
    let path = data_dir.join("hoover/models").join(&model_file);

    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{}.en.bin",
        config.whisper_model_size
    );
    let desc = format!("Whisper {} model", config.whisper_model_size);
    crate::models::ensure_model(&path, &url, &desc)?;

    Ok(path)
}
