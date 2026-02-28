use audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::audioadapter::Adapter;
use rubato::{Fft, FixedSync, Resampler as RubatoResampler};

use crate::error::{HooverError, Result};

const TARGET_SAMPLE_RATE: u32 = 16000;

/// Resamples multi-channel audio to 16kHz mono f32.
pub struct Resampler {
    inner: Option<Fft<f32>>,
    channels: u16,
    input_buf: Vec<f32>,
}

impl Resampler {
    pub fn new(source_rate: u32, channels: u16) -> Result<Self> {
        let needs_resample = source_rate != TARGET_SAMPLE_RATE;

        let chunk_size = 1024;

        let inner = if needs_resample {
            Some(
                Fft::new(
                    source_rate as usize,
                    TARGET_SAMPLE_RATE as usize,
                    chunk_size,
                    2, // sub_chunks
                    1, // output is always mono
                    FixedSync::Input,
                )
                .map_err(|e| HooverError::Resample(format!("failed to create resampler: {e}")))?,
            )
        } else {
            None
        };

        Ok(Self {
            inner,
            channels,
            input_buf: Vec::new(),
        })
    }

    /// Process interleaved multi-channel samples into 16kHz mono.
    pub fn process(&mut self, interleaved: &[f32]) -> Result<Vec<f32>> {
        // Step 1: De-interleave and mix to mono
        let mono = if self.channels == 1 {
            interleaved.to_vec()
        } else {
            let ch = self.channels as usize;
            let frame_count = interleaved.len() / ch;
            let mut mono = Vec::with_capacity(frame_count);
            for i in 0..frame_count {
                let mut sum = 0.0f32;
                for c in 0..ch {
                    sum += interleaved[i * ch + c];
                }
                mono.push(sum / ch as f32);
            }
            mono
        };

        // Step 2: Resample if needed
        if let Some(ref mut resampler) = self.inner {
            self.input_buf.extend_from_slice(&mono);

            let mut output = Vec::new();
            let frames_needed = resampler.input_frames_next();

            while self.input_buf.len() >= frames_needed {
                let chunk: Vec<f32> = self.input_buf.drain(..frames_needed).collect();
                // Wrap as 1-channel sequential buffer for rubato 1.0
                let input_data = vec![chunk];
                let input_buf = SequentialSliceOfVecs::new(&input_data, 1, frames_needed)
                    .map_err(|e| HooverError::Resample(format!("buffer error: {e}")))?;
                let result = resampler
                    .process(&input_buf, 0, None)
                    .map_err(|e| HooverError::Resample(format!("resample error: {e}")))?;
                // Extract samples from InterleavedOwned output
                let out_frames = result.frames();
                for frame in 0..out_frames {
                    output.push(result.read_sample(0, frame).unwrap_or(0.0));
                }
            }

            Ok(output)
        } else {
            Ok(mono)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_16k_mono() {
        let mut r = Resampler::new(16000, 1).unwrap_or_else(|e| panic!("{e}"));
        let input: Vec<f32> = (0..1600).map(|i| (i as f32 / 1600.0).sin()).collect();
        let output = r.process(&input).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(output.len(), input.len());
    }

    #[test]
    fn stereo_to_mono() {
        let mut r = Resampler::new(16000, 2).unwrap_or_else(|e| panic!("{e}"));
        // Interleaved stereo: [L, R, L, R, ...]
        let input: Vec<f32> = (0..3200).map(|i| i as f32 / 3200.0).collect();
        let output = r.process(&input).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(output.len(), 1600); // mono frames
    }
}
