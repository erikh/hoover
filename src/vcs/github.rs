use octocrab::Octocrab;

use super::resolve::ResolvedGithub;
use crate::error::{HooverError, Result};

/// Trigger a GitHub Actions workflow dispatch.
pub async fn trigger_workflow(config: &ResolvedGithub) -> Result<()> {
    let workflow = config.workflow.as_deref().ok_or_else(|| {
        HooverError::Config("github.workflow must be set to trigger a workflow".to_string())
    })?;

    let octocrab = Octocrab::builder()
        .personal_token(config.token.clone())
        .build()
        .map_err(|e| HooverError::Other(format!("failed to create GitHub client: {e}")))?;

    octocrab
        .actions()
        .create_workflow_dispatch(&config.owner, &config.repo, workflow, "main")
        .send()
        .await
        .map_err(|e| HooverError::Other(format!("failed to dispatch workflow: {e}")))?;

    tracing::info!(
        "triggered workflow '{workflow}' on {}/{}",
        config.owner,
        config.repo
    );
    Ok(())
}
