pub mod enroll;
pub mod identify;

use std::path::Path;

use ort::session::Session;

use crate::error::{HooverError, Result};

/// Load the ONNX speaker embedding model.
pub fn load_embedding_model(model_path: &Path) -> Result<Session> {
    Session::builder()
        .and_then(|b| b.commit_from_file(model_path))
        .map_err(|e| HooverError::Speaker(format!("failed to load speaker embedding model: {e}")))
}

/// Extract a speaker embedding from 16kHz mono audio samples.
///
/// Dynamically adapts the input tensor shape to match the model's expected input rank
/// (rank 2 for `[batch, samples]` or rank 3 for `[batch, 1, samples]`).
pub fn extract_embedding(session: &mut Session, samples: &[f32]) -> Result<Vec<f32>> {
    let input_rank = match session.inputs().first() {
        Some(input) => match input.dtype() {
            ort::value::ValueType::Tensor { shape, .. } => shape.len(),
            other => {
                return Err(HooverError::Speaker(format!(
                    "expected tensor input, got: {other:?}"
                )));
            }
        },
        None => {
            return Err(HooverError::Speaker(
                "model has no inputs".to_string(),
            ));
        }
    };

    let input_tensor = match input_rank {
        2 => ort::value::Tensor::from_array(([1usize, samples.len()], samples.to_vec())),
        3 => ort::value::Tensor::from_array(([1usize, 1, samples.len()], samples.to_vec())),
        n => {
            return Err(HooverError::Speaker(format!(
                "unsupported input tensor rank {n}, expected 2 or 3"
            )));
        }
    }
    .map_err(|e| HooverError::Speaker(format!("failed to create input tensor: {e}")))?;

    let outputs = session
        .run(ort::inputs![input_tensor])
        .map_err(|e| HooverError::Speaker(format!("model inference failed: {e}")))?;

    let (_shape, data) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| HooverError::Speaker(format!("failed to extract embedding tensor: {e}")))?;

    let embedding: Vec<f32> = data.to_vec();

    Ok(embedding)
}

/// Cosine similarity between two vectors.
#[must_use]
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_similarity_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_opposite() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_empty() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn cosine_similarity_mismatched_length() {
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), 0.0);
    }
}
