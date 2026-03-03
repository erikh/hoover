use std::path::Path;

use ort::session::Session;
use ort::value::Tensor;

use crate::error::{HooverError, Result};

/// Number of audio samples per VAD frame at 16kHz.
const WINDOW_SIZE: usize = 512;

/// Context samples prepended to each frame for temporal continuity.
const CONTEXT_SIZE: usize = 64;

/// Internal hidden state dimensions: `[2, 1, 128]`.
const STATE_DIM: usize = 128;

/// Lightweight wrapper around the Silero VAD v5 ONNX model.
///
/// The model expects three inputs per frame:
///   - `input`:  `[1, WINDOW_SIZE + CONTEXT_SIZE]` float32
///   - `state`:  `[2, 1, STATE_DIM]` float32  (RNN hidden state)
///   - `sr`:     `[1]` int64  (sample rate, always 16000)
///
/// And produces two outputs:
///   - `output`: speech probability (float32 scalar)
///   - `stateN`: updated hidden state `[2, 1, STATE_DIM]`
pub struct SileroVad {
    session: Session,
    state: Vec<f32>,
    context: Vec<f32>,
}

impl SileroVad {
    /// Load the Silero VAD model from an ONNX file.
    pub fn new(model_path: &Path) -> Result<Self> {
        let session = Session::builder()
            .map_err(|e| HooverError::Audio(format!("VAD session builder error: {e}")))?
            .commit_from_file(model_path)
            .map_err(|e| HooverError::Audio(format!("failed to load VAD model: {e}")))?;

        Ok(Self {
            session,
            state: vec![0.0f32; 2 * STATE_DIM],
            context: vec![0.0f32; CONTEXT_SIZE],
        })
    }

    /// Process a single 512-sample frame and return the speech probability (0.0–1.0).
    ///
    /// Returns `None` if inference fails.
    pub fn process_chunk(&mut self, samples: &[f32]) -> Option<f32> {
        if samples.len() != WINDOW_SIZE {
            return None;
        }

        // Build input: context (64) + samples (512) = 576
        let mut input_data = Vec::with_capacity(CONTEXT_SIZE + WINDOW_SIZE);
        input_data.extend_from_slice(&self.context);
        input_data.extend_from_slice(samples);

        // Update context for next call (last CONTEXT_SIZE samples of input)
        self.context
            .copy_from_slice(&samples[WINDOW_SIZE - CONTEXT_SIZE..]);

        let input_tensor =
            Tensor::from_array(([1usize, CONTEXT_SIZE + WINDOW_SIZE], input_data)).ok()?;

        let state_tensor =
            Tensor::from_array(([2usize, 1usize, STATE_DIM], self.state.clone())).ok()?;

        let sr_tensor = Tensor::from_array(([1usize], vec![16000_i64])).ok()?;

        let outputs = self
            .session
            .run(ort::inputs![input_tensor, state_tensor, sr_tensor])
            .ok()?;

        // Extract speech probability
        let (_shape, prob_data) = outputs[0].try_extract_tensor::<f32>().ok()?;
        let prob = prob_data.first().copied().unwrap_or(0.0);

        // Update internal state from the second output
        if let Ok((_shape, new_state)) = outputs[1].try_extract_tensor::<f32>() {
            self.state = new_state.to_vec();
        }

        Some(prob)
    }

    /// Reset internal state for a new audio stream.
    pub fn reset(&mut self) {
        self.state.fill(0.0);
        self.context.fill(0.0);
    }
}
