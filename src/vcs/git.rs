use std::path::Path;

use git2::{Repository, Signature};

use crate::error::{HooverError, Result};

/// Open an existing repo or initialize a new one.
pub fn open_or_init(path: &Path) -> Result<Repository> {
    if path.join(".git").exists() {
        Repository::open(path).map_err(Into::into)
    } else {
        Repository::init(path).map_err(Into::into)
    }
}

/// Stage all changes and create a commit.
pub fn add_and_commit(path: &Path, message: &str) -> Result<()> {
    let repo = open_or_init(path)?;

    let mut index = repo.index()?;
    index.add_all(std::iter::once("*"), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;

    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;

    let sig = Signature::now("hoover", "hoover@localhost")?;

    let parent = match repo.head() {
        Ok(head) => {
            let commit = head.peel_to_commit()?;
            Some(commit)
        }
        Err(_) => None, // Initial commit, no parent
    };

    let parents: Vec<&git2::Commit> = parent.iter().collect();

    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)?;

    tracing::info!("committed: {message}");
    Ok(())
}

/// Push to a named remote.
pub fn push_repo(path: &Path, remote_name: &str) -> Result<()> {
    let repo = Repository::open(path)?;

    let mut remote = repo.find_remote(remote_name).map_err(HooverError::Git)?;

    // Determine the current branch
    let head = repo.head()?;
    let refname = head
        .name()
        .ok_or_else(|| HooverError::Git(git2::Error::from_str("HEAD has no name")))?;

    let refspec = format!("{refname}:{refname}");

    remote.push(&[&refspec], None)?;

    tracing::info!("pushed to {remote_name}");
    Ok(())
}

/// Get the current git status of the output directory.
pub fn repo_status(path: &Path) -> Result<String> {
    let repo = Repository::open(path).map_err(HooverError::Git)?;

    let statuses = repo.statuses(None)?;
    let mut lines = Vec::new();

    if let Ok(head) = repo.head()
        && let Some(name) = head.shorthand()
    {
        lines.push(format!("branch: {name}"));
    }

    let modified = statuses
        .iter()
        .filter(|s| s.status() != git2::Status::CURRENT)
        .count();
    lines.push(format!("{modified} changed files"));

    Ok(lines.join("\n"))
}

/// Get recent commit log entries.
pub fn commit_log(path: &Path, limit: usize) -> Result<Vec<String>> {
    let repo = Repository::open(path).map_err(HooverError::Git)?;

    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.set_sorting(git2::Sort::TIME)?;

    let mut entries = Vec::new();
    for (i, oid) in revwalk.enumerate() {
        if i >= limit {
            break;
        }
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        let message = commit.message().unwrap_or("(no message)");
        let time = commit.time();
        let ts = chrono::DateTime::from_timestamp(time.seconds(), 0).map_or_else(
            || "unknown time".to_string(),
            |dt| dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        );

        entries.push(format!(
            "{} {} {}",
            &oid.to_string()[..8],
            ts,
            message.trim()
        ));
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_and_commit() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));

        // Write a test file
        std::fs::write(dir.path().join("test.md"), "hello").unwrap_or_else(|e| panic!("{e}"));

        // Init + commit
        add_and_commit(dir.path(), "initial commit").unwrap_or_else(|e| panic!("{e}"));

        // Verify repo exists
        let repo = Repository::open(dir.path()).unwrap_or_else(|e| panic!("{e}"));
        let head = repo.head().unwrap_or_else(|e| panic!("{e}"));
        let commit = head.peel_to_commit().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(commit.message(), Some("initial commit"));
    }

    #[test]
    fn commit_log_works() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(dir.path().join("a.md"), "a").unwrap_or_else(|e| panic!("{e}"));
        add_and_commit(dir.path(), "first").unwrap_or_else(|e| panic!("{e}"));

        std::fs::write(dir.path().join("b.md"), "b").unwrap_or_else(|e| panic!("{e}"));
        add_and_commit(dir.path(), "second").unwrap_or_else(|e| panic!("{e}"));

        let log = commit_log(dir.path(), 10).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(log.len(), 2);
        assert!(log[0].contains("second"));
        assert!(log[1].contains("first"));
    }
}
