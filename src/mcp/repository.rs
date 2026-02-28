use crate::config::Config;
use crate::vcs;

/// MCP tools for interacting with the hoover git repository.
///
/// These are registered as additional tools on the MCP service.
#[must_use]
pub fn get_commit_log(config: &Config, limit: Option<usize>) -> String {
    let output_dir = Config::expand_path(&config.output.directory);
    let limit = limit.unwrap_or(20);

    match vcs::git::commit_log(&output_dir, limit) {
        Ok(entries) => {
            if entries.is_empty() {
                "No commits found.".to_string()
            } else {
                entries.join("\n")
            }
        }
        Err(e) => format!("Error: {e}"),
    }
}

#[must_use]
pub fn get_repo_status(config: &Config) -> String {
    let output_dir = Config::expand_path(&config.output.directory);
    match vcs::git::repo_status(&output_dir) {
        Ok(status) => status,
        Err(e) => format!("Error: {e}"),
    }
}

#[must_use]
pub fn get_diff(config: &Config, from_ref: Option<&str>, to_ref: Option<&str>) -> String {
    let output_dir = Config::expand_path(&config.output.directory);

    let repo = match git2::Repository::open(&output_dir) {
        Ok(r) => r,
        Err(e) => return format!("Error opening repo: {e}"),
    };

    let from_obj = from_ref.map(|r| repo.revparse_single(r));
    let to_obj = to_ref.map(|r| repo.revparse_single(r));

    let from_tree = from_obj.and_then(|obj| obj.ok().and_then(|o| o.peel_to_tree().ok()));
    let to_tree = to_obj.and_then(|obj| obj.ok().and_then(|o| o.peel_to_tree().ok()));

    let diff = match repo.diff_tree_to_tree(from_tree.as_ref(), to_tree.as_ref(), None) {
        Ok(d) => d,
        Err(e) => return format!("Error generating diff: {e}"),
    };

    let mut output = String::new();
    let _ = diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        if let Ok(content) = std::str::from_utf8(line.content()) {
            let prefix = match line.origin() {
                '+' => "+",
                '-' => "-",
                _ => " ",
            };
            output.push_str(prefix);
            output.push_str(content);
        }
        true
    });

    if output.is_empty() {
        "No differences found.".to_string()
    } else {
        output
    }
}

#[must_use]
pub fn get_file_history(config: &Config, date: &str) -> String {
    let output_dir = Config::expand_path(&config.output.directory);
    let filename = format!("{date}.md");

    let repo = match git2::Repository::open(&output_dir) {
        Ok(r) => r,
        Err(e) => return format!("Error opening repo: {e}"),
    };

    let mut revwalk = match repo.revwalk() {
        Ok(r) => r,
        Err(e) => return format!("Error: {e}"),
    };

    if revwalk.push_head().is_err() {
        return "No commits found.".to_string();
    }

    let _ = revwalk.set_sorting(git2::Sort::TIME);

    let mut entries = Vec::new();
    for oid in revwalk.flatten() {
        if let Ok(commit) = repo.find_commit(oid) {
            // Check if this commit touches the file
            let dominated = commit.parent(0).ok().and_then(|parent| {
                let parent_tree = parent.tree().ok()?;
                let commit_tree = commit.tree().ok()?;
                let diff = repo
                    .diff_tree_to_tree(Some(&parent_tree), Some(&commit_tree), None)
                    .ok()?;

                let touches_file = diff.deltas().any(|d| {
                    d.new_file()
                        .path()
                        .and_then(|p| p.to_str())
                        .is_some_and(|p| p == filename || p.ends_with(&filename))
                });

                if touches_file { Some(()) } else { None }
            });

            if dominated.is_some() || (commit.parent_count() == 0) {
                let message = commit.message().unwrap_or("(no message)").trim();
                let time = commit.time();
                let ts = chrono::DateTime::from_timestamp(time.seconds(), 0).map_or_else(
                    || "unknown".to_string(),
                    |dt| dt.format("%Y-%m-%d %H:%M:%S").to_string(),
                );

                entries.push(format!("{} {} {}", &oid.to_string()[..8], ts, message));
            }
        }

        if entries.len() >= 50 {
            break;
        }
    }

    if entries.is_empty() {
        format!("No history found for {filename}")
    } else {
        entries.join("\n")
    }
}
