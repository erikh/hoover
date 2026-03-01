use std::path::{Path, PathBuf};

use ort::session::Session;

use crate::config::SpeakerConfig;
use crate::error::Result;

use super::enroll::SpeakerProfile;
use super::{cosine_similarity, extract_embedding};

/// Blending factor for continuous training (exponential moving average).
/// Small values evolve the profile slowly; large values adapt faster.
const EMA_ALPHA: f32 = 0.05;

/// Save updated profiles to disk every N successful identifications.
const SAVE_INTERVAL: u32 = 10;

/// Speaker identifier: holds loaded profiles and the embedding model session.
pub struct SpeakerIdentifier {
    profiles: Vec<SpeakerProfile>,
    session: Session,
    min_confidence: f32,
    filter_unknown: bool,
    profiles_dir: PathBuf,
    updates_since_save: u32,
}

/// Result of a speaker identification attempt.
#[derive(Debug, Clone)]
pub struct SpeakerMatch {
    pub name: Option<String>,
    pub confidence: f32,
}

impl SpeakerIdentifier {
    pub fn new(config: &SpeakerConfig) -> Result<Self> {
        let model_path = super::enroll::resolve_speaker_model(config.model_path.as_deref())?;
        let session = super::load_embedding_model(&model_path)?;
        let profiles_dir = crate::config::Config::expand_path(&config.profiles_dir);
        let profiles = load_all_profiles(&profiles_dir)?;

        tracing::info!("loaded {} speaker profiles", profiles.len());

        Ok(Self {
            profiles,
            session,
            min_confidence: config.min_confidence,
            filter_unknown: config.filter_unknown,
            profiles_dir,
            updates_since_save: 0,
        })
    }

    /// Identify the speaker from 16kHz mono audio samples.
    ///
    /// When a speaker is identified with high confidence, their stored
    /// embedding is refined using an exponential moving average of the new
    /// embedding. Updated profiles are saved to disk periodically.
    ///
    /// Returns `None` if `filter_unknown` is true and no speaker matches.
    pub fn identify(&mut self, samples: &[f32]) -> Result<Option<SpeakerMatch>> {
        if self.profiles.is_empty() {
            return Ok(Some(SpeakerMatch {
                name: None,
                confidence: 0.0,
            }));
        }

        let embedding = extract_embedding(&mut self.session, samples)?;

        let mut best_idx = 0;
        let mut best_score = f32::NEG_INFINITY;

        for (i, profile) in self.profiles.iter().enumerate() {
            let score = cosine_similarity(&embedding, &profile.embedding);
            if score > best_score {
                best_score = score;
                best_idx = i;
            }
        }

        if best_score >= self.min_confidence {
            // Continuously refine the matched profile via EMA
            for (stored, &new) in self.profiles[best_idx].embedding.iter_mut().zip(embedding.iter()) {
                *stored = (1.0 - EMA_ALPHA).mul_add(*stored, EMA_ALPHA * new);
            }
            let name = self.profiles[best_idx].name.clone();

            self.updates_since_save += 1;
            if self.updates_since_save >= SAVE_INTERVAL {
                self.save_profiles();
                self.updates_since_save = 0;
            }

            Ok(Some(SpeakerMatch {
                name: Some(name),
                confidence: best_score,
            }))
        } else if self.filter_unknown {
            Ok(None)
        } else {
            Ok(Some(SpeakerMatch {
                name: None,
                confidence: best_score,
            }))
        }
    }

    /// Save all profiles that have been updated back to disk.
    fn save_profiles(&self) {
        for profile in &self.profiles {
            if let Err(e) = profile.save(&self.profiles_dir) {
                tracing::warn!("failed to save profile '{}': {e}", profile.name);
            }
        }
        tracing::debug!("saved {} speaker profiles", self.profiles.len());
    }

    /// Flush any pending profile updates to disk (for graceful shutdown).
    pub fn flush(&self) {
        if self.updates_since_save > 0 {
            self.save_profiles();
        }
    }
}

fn load_all_profiles(dir: &Path) -> Result<Vec<SpeakerProfile>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut profiles = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("bin") {
            match SpeakerProfile::load(&path) {
                Ok(profile) => profiles.push(profile),
                Err(e) => tracing::warn!("failed to load speaker profile {}: {e}", path.display()),
            }
        }
    }

    Ok(profiles)
}
