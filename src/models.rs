use std::fs;
use std::io;
use std::path::Path;

use crate::error::{HooverError, Result};

/// Ensure a model file exists at `path`, downloading it from `url` if missing.
///
/// Downloads to a `{path}.part` temp file first, then renames into place so
/// interrupted downloads don't leave a corrupt file behind.
pub fn ensure_model(path: &Path, url: &str, description: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let part_path = path.with_extension(
        path.extension()
            .map_or_else(|| "part".to_string(), |e| format!("{}.part", e.to_string_lossy())),
    );

    eprintln!("Downloading {description}...");

    let response = ureq::get(url)
        .call()
        .map_err(|e| HooverError::Network(format!("failed to download {description}: {e}")))?;

    let mut reader = response.into_body().into_reader();
    let mut file = fs::File::create(&part_path)?;
    io::copy(&mut reader, &mut file)?;

    fs::rename(&part_path, path)?;

    eprintln!("Downloaded {description} to {}", path.display());
    Ok(())
}
