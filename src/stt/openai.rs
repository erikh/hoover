use std::io::Cursor;

use hound::{SampleFormat, WavSpec, WavWriter};
use reqwest::Client;
use serde::Deserialize;

use crate::audio::buffer::AudioChunk;
use crate::config::SttConfig;
use crate::error::{HooverError, Result};

use super::{SttEngine, TranscriptionSegment};

pub struct OpenAiEngine {
    client: Client,
    api_key: String,
    model: String,
    language: String,
}

impl OpenAiEngine {
    pub fn new(config: &SttConfig) -> Result<Self> {
        let api_key = config.openai_api_key.clone().ok_or_else(|| {
            HooverError::Stt("openai backend requires stt.openai_api_key to be set".to_string())
        })?;

        Ok(Self {
            client: Client::new(),
            api_key,
            model: config.openai_model.clone(),
            language: config.language.clone(),
        })
    }

    fn encode_wav(chunk: &AudioChunk) -> Result<Vec<u8>> {
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };

        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = WavWriter::new(&mut cursor, spec)
                .map_err(|e| HooverError::Stt(format!("failed to create WAV writer: {e}")))?;
            for &sample in &chunk.samples_i16 {
                writer
                    .write_sample(sample)
                    .map_err(|e| HooverError::Stt(format!("failed to write WAV sample: {e}")))?;
            }
            writer
                .finalize()
                .map_err(|e| HooverError::Stt(format!("failed to finalize WAV: {e}")))?;
        }

        Ok(cursor.into_inner())
    }
}

impl SttEngine for OpenAiEngine {
    fn transcribe(&mut self, chunk: &AudioChunk) -> Result<Vec<TranscriptionSegment>> {
        let wav_data = Self::encode_wav(chunk)?;

        let rt = tokio::runtime::Handle::try_current().map_err(|e| {
            HooverError::Stt(format!("openai backend requires a tokio runtime: {e}"))
        })?;

        let response = rt.block_on(async {
            let file_part = reqwest::multipart::Part::bytes(wav_data)
                .file_name("audio.wav")
                .mime_str("audio/wav")
                .map_err(|e| HooverError::Stt(format!("failed to set MIME type: {e}")))?;

            let form = reqwest::multipart::Form::new()
                .text("model", self.model.clone())
                .text("language", self.language.clone())
                .text("response_format", "verbose_json")
                .part("file", file_part);

            let resp = self
                .client
                .post("https://api.openai.com/v1/audio/transcriptions")
                .bearer_auth(&self.api_key)
                .multipart(form)
                .send()
                .await
                .map_err(|e| HooverError::Stt(format!("OpenAI API request failed: {e}")))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(HooverError::Stt(format!(
                    "OpenAI API returned {status}: {body}"
                )));
            }

            resp.json::<OpenAiResponse>()
                .await
                .map_err(|e| HooverError::Stt(format!("failed to parse OpenAI response: {e}")))
        })?;

        let text = response.text.trim().to_string();
        if text.is_empty() {
            return Ok(Vec::new());
        }

        // If word-level timestamps are available, create segments from them
        if let Some(words) = response.words {
            let segments = words
                .into_iter()
                .map(|w| {
                    #[allow(clippy::cast_possible_truncation)]
                    let offset = chrono::Duration::milliseconds((w.start * 1000.0) as i64);
                    TranscriptionSegment {
                        text: w.word,
                        timestamp: chunk.timestamp + offset,
                        duration_secs: w.end - w.start,
                        confidence: None,
                    }
                })
                .collect();
            return Ok(segments);
        }

        // Fallback: single segment for the whole chunk
        Ok(vec![TranscriptionSegment {
            text,
            timestamp: chunk.timestamp,
            duration_secs: chunk.duration_secs,
            confidence: None,
        }])
    }

    fn name(&self) -> &'static str {
        "openai"
    }
}

#[derive(Deserialize)]
struct OpenAiResponse {
    text: String,
    words: Option<Vec<OpenAiWord>>,
}

#[derive(Deserialize)]
struct OpenAiWord {
    word: String,
    start: f32,
    end: f32,
}
