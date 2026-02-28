pub mod git;
#[cfg(feature = "gitea")]
pub mod gitea;
#[cfg(feature = "github")]
pub mod github;
pub mod resolve;

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
    let token = resolve::get_push_token(&config.vcs);
    git::push_repo(&output_dir, &config.vcs.remote, token.as_deref())
}

/// Trigger a forge action (GitHub/Gitea workflow).
#[allow(unused_variables)]
pub async fn trigger(config: &Config) -> Result<()> {
    let output_dir = Config::expand_path(&config.output.directory);
    let remote = &config.vcs.remote;

    #[cfg(feature = "github")]
    if config.vcs.github.is_some() {
        let resolved = resolve::resolve_github(&config.vcs, &output_dir, remote)?;
        return github::trigger_workflow(&resolved).await;
    }

    #[cfg(feature = "gitea")]
    if config.vcs.gitea.is_some() {
        let resolved = resolve::resolve_gitea(&config.vcs, &output_dir, remote)?;
        return gitea::trigger_workflow(&resolved).await;
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
