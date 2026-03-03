pub mod buffer;
pub mod capture;
pub mod resample;
pub mod vad;

use std::path::PathBuf;

use tokio::sync::mpsc;

use crate::config::AudioConfig;
use crate::error::{HooverError, Result};

use self::buffer::{AudioChunk, ChunkAccumulator, Chunker, VadChunkAccumulator};
use self::capture::AudioCapture;
use self::resample::Resampler;
use self::vad::SileroVad;

const VAD_MODEL_URL: &str =
    "https://huggingface.co/onnx-community/silero-vad/resolve/main/onnx/model.onnx";

/// Resolve the Silero VAD ONNX model, downloading it if necessary.
fn resolve_vad_model() -> Result<PathBuf> {
    let data_dir = dirs::data_dir()
        .ok_or_else(|| HooverError::Audio("could not determine data directory".to_string()))?;

    let model_path = data_dir.join("hoover/models/silero_vad.onnx");

    crate::models::ensure_model(&model_path, VAD_MODEL_URL, "Silero VAD model")?;

    Ok(model_path)
}

/// Runs the audio pipeline in a dedicated thread: capture → resample → chunk → send.
///
/// Returns a receiver that yields `AudioChunk`s ready for STT processing.
pub fn start_audio_pipeline(
    config: &AudioConfig,
    chunk_tx: mpsc::Sender<AudioChunk>,
) -> Result<AudioCapture> {
    // Resolve VAD model before spawning the thread so errors propagate to caller.
    let chunker = if config.vad_enabled {
        let model_path = resolve_vad_model()?;
        let vad = SileroVad::new(&model_path)?;
        Chunker::Vad(VadChunkAccumulator::new(
            vad,
            config.min_chunk_secs,
            config.max_chunk_secs,
            config.overlap_secs,
            config.silence_threshold_ms,
        ))
    } else {
        Chunker::Fixed(ChunkAccumulator::new(
            config.chunk_duration_secs,
            config.overlap_secs,
        ))
    };

    let capture = AudioCapture::new(config)?;
    let sample_rate = capture.sample_rate();
    let channels = capture.channels();
    let raw_rx = capture.receiver();

    let chunk_duration = config.chunk_duration_secs;
    let overlap = config.overlap_secs;
    let vad_enabled = config.vad_enabled;

    std::thread::spawn(move || {
        let mut resampler = match Resampler::new(sample_rate, channels) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("failed to create resampler: {e}");
                return;
            }
        };

        if vad_enabled {
            tracing::debug!(
                "audio pipeline: source_rate={sample_rate}, channels={channels}, VAD chunking"
            );
        } else {
            tracing::debug!(
                "audio pipeline: source_rate={sample_rate}, channels={channels}, chunk={chunk_duration}s, overlap={overlap}s"
            );
        }

        let mut accumulator = chunker;
        let mut total_raw = 0usize;
        let mut total_resampled = 0usize;

        while let Ok(raw_samples) = raw_rx.recv() {
            total_raw += raw_samples.len();
            let mono_16k = match resampler.process(&raw_samples) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("resample error: {e}");
                    continue;
                }
            };

            total_resampled += mono_16k.len();

            if total_raw % (sample_rate as usize * channels as usize * 10) < raw_samples.len() {
                tracing::debug!(
                    "audio pipeline: raw={total_raw} samples, resampled={total_resampled} samples ({:.1}s at 16kHz)",
                    total_resampled as f64 / 16000.0
                );
            }

            for chunk in accumulator.feed(&mono_16k) {
                tracing::info!(
                    "audio chunk ready: {:.1}s of audio",
                    chunk.duration_secs
                );
                if chunk_tx.blocking_send(chunk).is_err() {
                    tracing::debug!("chunk receiver dropped, stopping audio pipeline");
                    return;
                }
            }
        }

        // Flush remaining samples
        if let Some(chunk) = accumulator.flush() {
            let _ = chunk_tx.blocking_send(chunk);
        }

        tracing::debug!("audio pipeline thread exiting");
    });

    Ok(capture)
}
