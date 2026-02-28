use std::path::Path;

use git2::Repository;

use crate::config::VcsConfig;
use crate::error::{HooverError, Result};

/// Fully resolved GitHub configuration with all required fields present.
pub struct ResolvedGithub {
    pub token: String,
    pub owner: String,
    pub repo: String,
    pub workflow: Option<String>,
}

/// Fully resolved Gitea configuration with all required fields present.
pub struct ResolvedGitea {
    pub url: String,
    pub token: String,
    pub owner: String,
    pub repo: String,
}

/// Parse a git remote URL into `(base_url, owner, repo)`.
///
/// Handles SSH (`git@host:owner/repo.git`) and HTTPS (`https://host/owner/repo.git`) formats.
fn parse_remote_url(url: &str) -> Option<(String, String, String)> {
    // SSH format: git@host:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        let path = path.strip_suffix(".git").unwrap_or(path);
        let (owner, repo) = path.split_once('/')?;
        if owner.is_empty() || repo.is_empty() {
            return None;
        }
        return Some((format!("https://{host}"), owner.to_string(), repo.to_string()));
    }

    // HTTPS format: https://host/owner/repo.git or https://host/owner/repo
    if url.starts_with("https://") || url.starts_with("http://") {
        let without_scheme = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?;
        let parts: Vec<&str> = without_scheme.splitn(3, '/').collect();
        if parts.len() < 3 {
            return None;
        }
        let host = parts[0];
        let owner = parts[1];
        let repo = parts[2].strip_suffix(".git").unwrap_or(parts[2]);
        // Reject if there are extra path segments beyond owner/repo
        if owner.is_empty() || repo.is_empty() || repo.contains('/') {
            return None;
        }
        let scheme = if url.starts_with("https://") {
            "https"
        } else {
            "http"
        };
        return Some((
            format!("{scheme}://{host}"),
            owner.to_string(),
            repo.to_string(),
        ));
    }

    None
}

/// Read the URL of a named remote from a git repository at `path`.
fn get_remote_url(path: &Path, remote_name: &str) -> Option<String> {
    let repo = Repository::open(path).ok()?;
    let remote = repo.find_remote(remote_name).ok()?;
    remote.url().map(String::from)
}

/// Try to get a token from environment variables, then the `gh` CLI.
fn resolve_github_token() -> Option<String> {
    if let Ok(t) = std::env::var("GITHUB_TOKEN")
        && !t.is_empty()
    {
        return Some(t);
    }
    if let Ok(t) = std::env::var("GH_TOKEN")
        && !t.is_empty()
    {
        return Some(t);
    }
    // Try `gh auth token`
    std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let s = String::from_utf8(o.stdout).ok()?;
            let s = s.trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        })
}

/// Try to get a Gitea token from environment variables.
fn resolve_gitea_token() -> Option<String> {
    std::env::var("GITEA_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
}

/// Resolve a complete GitHub configuration from config values, environment, and git remote.
///
/// Priority for token: config > `GITHUB_TOKEN` > `GH_TOKEN` > `gh auth token`.
/// Owner/repo fall back to parsing the git remote URL.
pub fn resolve_github(
    config: &VcsConfig,
    output_dir: &Path,
    remote: &str,
) -> Result<ResolvedGithub> {
    let gh = config.github.as_ref();

    let token = gh
        .and_then(|g| g.token.clone())
        .or_else(resolve_github_token)
        .ok_or_else(|| {
            HooverError::Config(
                "GitHub token not found: set github.token in config, \
                 or set GITHUB_TOKEN / GH_TOKEN, or run `gh auth login`"
                    .to_string(),
            )
        })?;

    let (remote_owner, remote_repo) = get_remote_url(output_dir, remote)
        .and_then(|u| parse_remote_url(&u))
        .map_or((None, None), |(_, o, r)| (Some(o), Some(r)));

    let owner = gh
        .and_then(|g| g.owner.clone())
        .or(remote_owner)
        .ok_or_else(|| {
            HooverError::Config(
                "GitHub owner not found: set github.owner in config \
                 or ensure the git remote URL is parseable"
                    .to_string(),
            )
        })?;

    let repo = gh
        .and_then(|g| g.repo.clone())
        .or(remote_repo)
        .ok_or_else(|| {
            HooverError::Config(
                "GitHub repo not found: set github.repo in config \
                 or ensure the git remote URL is parseable"
                    .to_string(),
            )
        })?;

    let workflow = gh.and_then(|g| g.workflow.clone());

    Ok(ResolvedGithub {
        token,
        owner,
        repo,
        workflow,
    })
}

/// Resolve a complete Gitea configuration from config values, environment, and git remote.
///
/// Priority for token: config > `GITEA_TOKEN`.
/// URL, owner, repo fall back to parsing the git remote URL.
pub fn resolve_gitea(
    config: &VcsConfig,
    output_dir: &Path,
    remote: &str,
) -> Result<ResolvedGitea> {
    let gt = config.gitea.as_ref();

    let token = gt
        .and_then(|g| g.token.clone())
        .or_else(resolve_gitea_token)
        .ok_or_else(|| {
            HooverError::Config(
                "Gitea token not found: set gitea.token in config or set GITEA_TOKEN".to_string(),
            )
        })?;

    let (remote_url, remote_owner, remote_repo) = get_remote_url(output_dir, remote)
        .and_then(|u| parse_remote_url(&u))
        .map_or((None, None, None), |(url, o, r)| {
            (Some(url), Some(o), Some(r))
        });

    let url = gt
        .and_then(|g| g.url.clone())
        .or(remote_url)
        .ok_or_else(|| {
            HooverError::Config(
                "Gitea URL not found: set gitea.url in config \
                 or ensure the git remote URL is parseable"
                    .to_string(),
            )
        })?;

    let owner = gt
        .and_then(|g| g.owner.clone())
        .or(remote_owner)
        .ok_or_else(|| {
            HooverError::Config(
                "Gitea owner not found: set gitea.owner in config \
                 or ensure the git remote URL is parseable"
                    .to_string(),
            )
        })?;

    let repo = gt
        .and_then(|g| g.repo.clone())
        .or(remote_repo)
        .ok_or_else(|| {
            HooverError::Config(
                "Gitea repo not found: set gitea.repo in config \
                 or ensure the git remote URL is parseable"
                    .to_string(),
            )
        })?;

    Ok(ResolvedGitea {
        url,
        token,
        owner,
        repo,
    })
}

/// Extract a push token from the VCS config (if any forge is configured with a token).
/// Used to authenticate git push over HTTPS.
pub fn get_push_token(config: &VcsConfig) -> Option<String> {
    if let Some(ref gh) = config.github
        && let Some(ref t) = gh.token
    {
        return Some(t.clone());
    }
    if let Some(ref gt) = config.gitea
        && let Some(ref t) = gt.token
    {
        return Some(t.clone());
    }
    // Fall back to environment
    resolve_github_token().or_else(resolve_gitea_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ssh_url() {
        let (base, owner, repo) =
            parse_remote_url("git@github.com:erikh/hoover.git").expect("should parse");
        assert_eq!(base, "https://github.com");
        assert_eq!(owner, "erikh");
        assert_eq!(repo, "hoover");
    }

    #[test]
    fn parse_https_url() {
        let (base, owner, repo) =
            parse_remote_url("https://github.com/erikh/hoover.git").expect("should parse");
        assert_eq!(base, "https://github.com");
        assert_eq!(owner, "erikh");
        assert_eq!(repo, "hoover");
    }

    #[test]
    fn parse_https_no_dot_git() {
        let (base, owner, repo) =
            parse_remote_url("https://github.com/erikh/hoover").expect("should parse");
        assert_eq!(base, "https://github.com");
        assert_eq!(owner, "erikh");
        assert_eq!(repo, "hoover");
    }

    #[test]
    fn parse_ssh_no_dot_git() {
        let (_, owner, repo) =
            parse_remote_url("git@github.com:erikh/hoover").expect("should parse");
        assert_eq!(owner, "erikh");
        assert_eq!(repo, "hoover");
    }

    #[test]
    fn parse_invalid_url() {
        assert!(parse_remote_url("not-a-url").is_none());
        assert!(parse_remote_url("https://github.com").is_none());
        assert!(parse_remote_url("https://github.com/").is_none());
        assert!(parse_remote_url("git@github.com:").is_none());
    }

    #[test]
    fn parse_gitea_https_url() {
        let (base, owner, repo) =
            parse_remote_url("https://gitea.example.com/myorg/myrepo.git").expect("should parse");
        assert_eq!(base, "https://gitea.example.com");
        assert_eq!(owner, "myorg");
        assert_eq!(repo, "myrepo");
    }
}
