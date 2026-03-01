pub mod buffer;
pub mod capture;
pub mod resample;

use tokio::sync::mpsc;

use crate::config::AudioConfig;
use crate::error::Result;

use self::buffer::{AudioChunk, ChunkAccumulator};
use self::capture::AudioCapture;
use self::resample::Resampler;

/// Runs the audio pipeline in a dedicated thread: capture → resample → chunk → send.
///
/// Returns a receiver that yields `AudioChunk`s ready for STT processing.
pub fn start_audio_pipeline(
    config: &AudioConfig,
    chunk_tx: mpsc::Sender<AudioChunk>,
) -> Result<AudioCapture> {
    let capture = AudioCapture::new(config)?;
    let sample_rate = capture.sample_rate();
    let channels = capture.channels();
    let raw_rx = capture.receiver();

    let chunk_duration = config.chunk_duration_secs;
    let overlap = config.overlap_secs;

    std::thread::spawn(move || {
        let mut resampler = match Resampler::new(sample_rate, channels) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("failed to create resampler: {e}");
                return;
            }
        };

        tracing::debug!(
            "audio pipeline: source_rate={sample_rate}, channels={channels}, chunk={chunk_duration}s, overlap={overlap}s"
        );

        let mut accumulator = ChunkAccumulator::new(chunk_duration, overlap);
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
