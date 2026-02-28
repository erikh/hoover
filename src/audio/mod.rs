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

        let mut accumulator = ChunkAccumulator::new(chunk_duration, overlap);

        while let Ok(raw_samples) = raw_rx.recv() {
            let mono_16k = match resampler.process(&raw_samples) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("resample error: {e}");
                    continue;
                }
            };

            for chunk in accumulator.feed(&mono_16k) {
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
