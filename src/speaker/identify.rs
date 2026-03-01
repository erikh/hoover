use std::path::Path;

use ort::session::Session;

use crate::config::SpeakerConfig;
use crate::error::Result;

use super::enroll::SpeakerProfile;
use super::{cosine_similarity, extract_embedding};

/// Speaker identifier: holds loaded profiles and the embedding model session.
pub struct SpeakerIdentifier {
    profiles: Vec<SpeakerProfile>,
    session: Session,
    min_confidence: f32,
    filter_unknown: bool,
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
        let profiles =
            load_all_profiles(&crate::config::Config::expand_path(&config.profiles_dir))?;

        tracing::info!("loaded {} speaker profiles", profiles.len());

        Ok(Self {
            profiles,
            session,
            min_confidence: config.min_confidence,
            filter_unknown: config.filter_unknown,
        })
    }

    /// Identify the speaker from 16kHz mono audio samples.
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

        let mut best_name = None;
        let mut best_score = f32::NEG_INFINITY;

        for profile in &self.profiles {
            let score = cosine_similarity(&embedding, &profile.embedding);
            if score > best_score {
                best_score = score;
                best_name = Some(profile.name.clone());
            }
        }

        if best_score >= self.min_confidence {
            Ok(Some(SpeakerMatch {
                name: best_name,
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
