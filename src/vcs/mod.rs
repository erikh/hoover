pub mod git;
#[cfg(feature = "gitea")]
pub mod gitea;
#[cfg(feature = "github")]
pub mod github;

use crate::config::Config;
use crate::error::{HooverError, Result};

/// Push the output repository to the configured remote.
pub fn push(config: &Config) -> Result<()> {
    if !config.vcs.enabled {
        return Err(HooverError::Config(
            "VCS is not enabled in config".to_string(),
        ));
    }

    let output_dir = Config::expand_path(&config.output.directory);
    git::push_repo(&output_dir, &config.vcs.remote)
}

/// Trigger a forge action (GitHub/Gitea workflow).
#[allow(unused_variables)]
pub async fn trigger(config: &Config) -> Result<()> {
    #[cfg(feature = "github")]
    if let Some(ref gh) = config.vcs.github {
        return github::trigger_workflow(gh).await;
    }

    #[cfg(feature = "gitea")]
    if let Some(ref gt) = config.vcs.gitea {
        return gitea::trigger_workflow(gt).await;
    }

    Err(HooverError::Config(
        "no forge configured (enable github or gitea feature and configure in config)".to_string(),
    ))
}

/// Auto-commit the output directory if VCS is enabled and `auto_commit` is on.
pub fn auto_commit(config: &Config) -> Result<()> {
    if !config.vcs.enabled || !config.vcs.auto_commit {
        return Ok(());
    }

    let output_dir = Config::expand_path(&config.output.directory);
    git::add_and_commit(&output_dir, "auto-commit transcription update")
}

/// Auto-push if VCS is enabled and `auto_push` is on.
pub fn auto_push(config: &Config) -> Result<()> {
    if !config.vcs.enabled || !config.vcs.auto_push {
        return Ok(());
    }

    push(config)
}
