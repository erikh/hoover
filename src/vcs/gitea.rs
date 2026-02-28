use crate::config::GiteaConfig;
use crate::error::{HooverError, Result};

/// Trigger a Gitea Actions workflow or perform API operations.
pub async fn trigger_workflow(config: &GiteaConfig) -> Result<()> {
    let _client = gitea_sdk::Client::new(&config.url, gitea_sdk::Auth::Token(&config.token));

    // Gitea's API for dispatching workflows: POST /repos/{owner}/{repo}/actions/workflows/{workflow}/dispatches
    // The gitea-sdk may not have this directly; use the raw API if needed.
    // For now, we'll create an issue as a trigger signal.
    tracing::info!(
        "triggering action on {}/{} at {}",
        config.owner,
        config.repo,
        config.url
    );

    // Use the raw API client to trigger a workflow dispatch
    let url = format!(
        "{}/api/v1/repos/{}/{}/actions/workflows/ci.yml/dispatches",
        config.url, config.owner, config.repo
    );

    let http_client = reqwest::Client::new();
    let resp = http_client
        .post(&url)
        .header("Authorization", format!("token {}", config.token))
        .json(&serde_json::json!({ "ref": "main" }))
        .send()
        .await
        .map_err(|e| HooverError::Other(format!("failed to trigger Gitea workflow: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(HooverError::Other(format!(
            "Gitea API returned {status}: {body}"
        )));
    }

    tracing::info!(
        "triggered Gitea workflow for {}/{}",
        config.owner,
        config.repo
    );
    Ok(())
}
