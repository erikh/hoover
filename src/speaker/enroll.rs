use std::fs;
use std::path::PathBuf;

use crate::audio::capture::AudioCapture;
use crate::audio::resample::Resampler;
use crate::config::Config;
use crate::error::{HooverError, Result};

use super::{extract_embedding, load_embedding_model};

/// Speaker profile: a name and averaged embedding vector.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpeakerProfile {
    pub name: String,
    pub embedding: Vec<f32>,
}

impl SpeakerProfile {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let data = fs::read(path)?;
        bincode_deserialize(&data)
    }

    pub fn save(&self, dir: &std::path::Path) -> Result<PathBuf> {
        fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}.bin", self.name));
        let data = bincode_serialize(self);
        fs::write(&path, data)?;
        Ok(path)
    }
}

fn bincode_serialize(profile: &SpeakerProfile) -> Vec<u8> {
    // Simple binary format: name_len(u32) + name_bytes + embedding_len(u32) + embedding_f32s
    let mut data = Vec::new();
    let name_bytes = profile.name.as_bytes();
    data.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
    data.extend_from_slice(name_bytes);
    data.extend_from_slice(&(profile.embedding.len() as u32).to_le_bytes());
    for &v in &profile.embedding {
        data.extend_from_slice(&v.to_le_bytes());
    }
    data
}

fn bincode_deserialize(data: &[u8]) -> Result<SpeakerProfile> {
    if data.len() < 4 {
        return Err(HooverError::Speaker("profile data too short".to_string()));
    }
    let mut pos = 0;

    let name_len = u32::from_le_bytes(
        data[pos..pos + 4]
            .try_into()
            .map_err(|_| HooverError::Speaker("invalid profile data".to_string()))?,
    ) as usize;
    pos += 4;

    if data.len() < pos + name_len + 4 {
        return Err(HooverError::Speaker("profile data truncated".to_string()));
    }
    let name = String::from_utf8(data[pos..pos + name_len].to_vec())
        .map_err(|e| HooverError::Speaker(format!("invalid profile name: {e}")))?;
    pos += name_len;

    let emb_len = u32::from_le_bytes(
        data[pos..pos + 4]
            .try_into()
            .map_err(|_| HooverError::Speaker("invalid profile data".to_string()))?,
    ) as usize;
    pos += 4;

    if data.len() < pos + emb_len * 4 {
        return Err(HooverError::Speaker("profile data truncated".to_string()));
    }

    let mut embedding = Vec::with_capacity(emb_len);
    for _ in 0..emb_len {
        let val = f32::from_le_bytes(
            data[pos..pos + 4]
                .try_into()
                .map_err(|_| HooverError::Speaker("invalid float data".to_string()))?,
        );
        embedding.push(val);
        pos += 4;
    }

    Ok(SpeakerProfile { name, embedding })
}

/// List all enrolled speaker profile names from the profiles directory.
pub fn list_profiles(profiles_dir: &std::path::Path) -> Result<Vec<String>> {
    if !profiles_dir.exists() {
        return Ok(Vec::new());
    }

    let mut names = Vec::new();
    for entry in fs::read_dir(profiles_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("bin") {
            match SpeakerProfile::load(&path) {
                Ok(profile) => names.push(profile.name),
                Err(e) => tracing::warn!("failed to load profile {}: {e}", path.display()),
            }
        }
    }

    names.sort();
    Ok(names)
}

/// Remove an enrolled speaker profile by name.
pub fn remove_profile(profiles_dir: &std::path::Path, name: &str) -> Result<()> {
    let path = profiles_dir.join(format!("{name}.bin"));
    if !path.exists() {
        return Err(HooverError::Speaker(format!(
            "no profile found for '{name}'"
        )));
    }
    fs::remove_file(&path)?;
    Ok(())
}

/// Run speaker enrollment: record audio, extract embeddings, save profile.
pub async fn run_enrollment(config: &Config, name: &str) -> Result<()> {
    let model_path = resolve_speaker_model(config.speaker.model_path.as_deref())?;
    let mut session = load_embedding_model(&model_path)?;

    tracing::info!("Recording audio for speaker enrollment of '{name}'...");
    tracing::info!("Speak for 10-30 seconds, then press Ctrl+C to stop.");

    let capture = AudioCapture::new(&config.audio)?;
    let raw_rx = capture.receiver();
    let sample_rate = capture.sample_rate();
    let channels = capture.channels();

    capture.start()?;

    let mut resampler = Resampler::new(sample_rate, channels)?;
    let mut all_samples = Vec::new();

    // Record until Ctrl+C
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();

    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = stop_tx.send(());
    });

    loop {
        tokio::task::yield_now().await;
        match raw_rx.try_recv() {
            Ok(samples) => {
                let mono = resampler.process(&samples)?;
                all_samples.extend_from_slice(&mono);
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
            Err(crossbeam_channel::TryRecvError::Disconnected) => break,
        }
    }

    capture.pause()?;

    let duration_secs = all_samples.len() as f32 / 16000.0;
    tracing::info!("Recorded {duration_secs:.1} seconds of audio");

    if duration_secs < 3.0 {
        return Err(HooverError::Speaker(
            "recording too short — need at least 3 seconds for enrollment".to_string(),
        ));
    }

    // Split into 3-second segments and extract embeddings
    let segment_samples = 16000 * 3;
    let mut embeddings = Vec::new();

    for chunk_start in (0..all_samples.len()).step_by(segment_samples) {
        let chunk_end = (chunk_start + segment_samples).min(all_samples.len());
        if chunk_end - chunk_start < 16000 {
            break; // skip segments < 1 second
        }
        let segment = &all_samples[chunk_start..chunk_end];
        let emb = extract_embedding(&mut session, segment)?;
        embeddings.push(emb);
    }

    if embeddings.is_empty() {
        return Err(HooverError::Speaker(
            "failed to extract any embeddings".to_string(),
        ));
    }

    // Average embeddings
    let dim = embeddings[0].len();
    let mut avg = vec![0.0f32; dim];
    for emb in &embeddings {
        for (i, &v) in emb.iter().enumerate() {
            avg[i] += v;
        }
    }
    let n = embeddings.len() as f32;
    for v in &mut avg {
        *v /= n;
    }

    let profile = SpeakerProfile {
        name: name.to_string(),
        embedding: avg,
    };

    let profiles_dir = Config::expand_path(&config.speaker.profiles_dir);
    let saved_path = profile.save(&profiles_dir)?;
    tracing::info!("Speaker profile saved to {}", saved_path.display());

    Ok(())
}

const SPEAKER_MODEL_URL: &str =
    "https://huggingface.co/Wespeaker/wespeaker-ecapa-tdnn512-LM/resolve/main/voxceleb_ECAPA512_LM.onnx";

pub(crate) fn resolve_speaker_model(custom_path: Option<&str>) -> Result<std::path::PathBuf> {
    if let Some(path) = custom_path {
        let expanded = Config::expand_path(path);
        if !expanded.exists() {
            return Err(HooverError::Speaker(format!(
                "speaker model not found: {}",
                expanded.display()
            )));
        }
        return Ok(expanded);
    }

    let data_dir = dirs::data_dir()
        .ok_or_else(|| HooverError::Speaker("could not determine data directory".to_string()))?;

    let model_path = data_dir.join("hoover/models/speaker_embedding.onnx");

    crate::models::ensure_model(
        &model_path,
        SPEAKER_MODEL_URL,
        "ECAPA-TDNN speaker embedding model",
    )?;

    Ok(model_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_round_trip() {
        let profile = SpeakerProfile {
            name: "test_speaker".to_string(),
            embedding: vec![0.1, 0.2, 0.3, -0.5, 1.0],
        };

        let data = bincode_serialize(&profile);
        let restored = bincode_deserialize(&data).unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(restored.name, profile.name);
        assert_eq!(restored.embedding.len(), profile.embedding.len());
        for (a, b) in restored.embedding.iter().zip(profile.embedding.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn profile_save_and_load() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let profile = SpeakerProfile {
            name: "alice".to_string(),
            embedding: vec![1.0, 2.0, 3.0],
        };

        let path = profile.save(dir.path()).unwrap_or_else(|e| panic!("{e}"));
        assert!(path.exists());

        let loaded = SpeakerProfile::load(&path).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(loaded.name, "alice");
        assert_eq!(loaded.embedding.len(), 3);
    }
}
