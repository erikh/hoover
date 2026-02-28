use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{Datelike, Local, NaiveDate};

use crate::config::OutputConfig;
use crate::error::{HooverError, Result};
use crate::stt::TranscriptionSegment;

/// Writes transcription segments to daily markdown files.
pub struct MarkdownWriter {
    output_dir: PathBuf,
    timestamps: bool,
    current_date: Option<NaiveDate>,
    /// Trailing words from the last written segment, for overlap deduplication.
    last_trailing_words: Vec<String>,
}

impl MarkdownWriter {
    pub fn new(config: &OutputConfig) -> Result<Self> {
        let output_dir = crate::config::Config::expand_path(&config.directory);
        fs::create_dir_all(&output_dir)?;

        Ok(Self {
            output_dir,
            timestamps: config.timestamps,
            current_date: None,
            last_trailing_words: Vec::new(),
        })
    }

    /// Write a transcription segment, optionally with a speaker name.
    pub fn write_segment(
        &mut self,
        segment: &TranscriptionSegment,
        speaker: Option<&str>,
    ) -> Result<()> {
        let local_time = segment.timestamp.with_timezone(&Local);
        let date = local_time.date_naive();
        let path = self.file_path(date);

        // Write header if this is a new day
        let needs_header = self.current_date != Some(date);
        if needs_header {
            self.current_date = Some(date);
            self.last_trailing_words.clear();
            self.write_day_header(&path, date)?;
        }

        // Overlap deduplication
        let text = self.deduplicate_overlap(&segment.text);
        if text.is_empty() {
            return Ok(());
        }

        // Format the entry
        let entry = if self.timestamps {
            let time_str = local_time.format("%H:%M:%S");
            speaker.map_or_else(
                || format!("**[{time_str}]** {text}\n\n"),
                |name| format!("**[{time_str} — {name}]** {text}\n\n"),
            )
        } else {
            format!("{text}\n\n")
        };

        // Append to the file
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| HooverError::Output(format!("failed to open {}: {e}", path.display())))?;

        file.write_all(entry.as_bytes()).map_err(|e| {
            HooverError::Output(format!("failed to write to {}: {e}", path.display()))
        })?;

        // Store trailing words for next overlap check
        self.last_trailing_words = text
            .split_whitespace()
            .rev()
            .take(20)
            .map(str::to_lowercase)
            .collect();
        self.last_trailing_words.reverse();

        tracing::debug!("wrote segment to {}", path.display());
        Ok(())
    }

    fn file_path(&self, date: NaiveDate) -> PathBuf {
        self.output_dir
            .join(format!("{}.md", date.format("%Y-%m-%d")))
    }

    #[allow(clippy::unused_self)]
    fn write_day_header(&self, path: &Path, date: NaiveDate) -> Result<()> {
        if path.exists() {
            // File already exists (e.g., resuming), don't rewrite header
            return Ok(());
        }

        let weekday = date.weekday();
        let month = date.format("%B");
        let day = date.day();
        let year = date.year();
        let header = format!("# {weekday}, {month} {day}, {year}\n\n");

        fs::write(path, header.as_bytes()).map_err(|e| {
            HooverError::Output(format!("failed to write header to {}: {e}", path.display()))
        })
    }

    /// Remove overlapping prefix words from the new text.
    fn deduplicate_overlap(&self, text: &str) -> String {
        if self.last_trailing_words.is_empty() {
            return text.to_string();
        }

        let new_words: Vec<&str> = text.split_whitespace().collect();
        if new_words.is_empty() {
            return String::new();
        }

        let trailing = &self.last_trailing_words;

        // Find the longest prefix of new_words that matches a suffix of last_trailing_words
        let max_overlap = trailing.len().min(new_words.len());
        let mut best_overlap = 0;

        for overlap_len in 1..=max_overlap {
            let trailing_suffix = &trailing[trailing.len() - overlap_len..];
            let new_prefix: Vec<String> = new_words[..overlap_len]
                .iter()
                .map(|w| w.to_lowercase())
                .collect();

            if trailing_suffix == new_prefix.as_slice() {
                best_overlap = overlap_len;
            }
        }

        if best_overlap > 0 {
            new_words[best_overlap..].join(" ")
        } else {
            text.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn test_config(dir: &Path) -> OutputConfig {
        OutputConfig {
            directory: dir.to_string_lossy().to_string(),
            timestamps: true,
        }
    }

    #[test]
    fn creates_daily_file_with_header() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let mut writer =
            MarkdownWriter::new(&test_config(dir.path())).unwrap_or_else(|e| panic!("{e}"));

        let segment = TranscriptionSegment {
            text: "hello world".to_string(),
            timestamp: Utc::now(),
            duration_secs: 1.0,
            confidence: None,
        };

        writer
            .write_segment(&segment, None)
            .unwrap_or_else(|e| panic!("{e}"));

        let date = Local::now().date_naive();
        let file = dir.path().join(format!("{}.md", date.format("%Y-%m-%d")));
        assert!(file.exists());

        let content = fs::read_to_string(&file).unwrap_or_else(|e| panic!("{e}"));
        assert!(content.contains("# "));
        assert!(content.contains("hello world"));
    }

    #[test]
    fn writes_speaker_tag() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let mut writer =
            MarkdownWriter::new(&test_config(dir.path())).unwrap_or_else(|e| panic!("{e}"));

        let segment = TranscriptionSegment {
            text: "important note".to_string(),
            timestamp: Utc::now(),
            duration_secs: 1.0,
            confidence: None,
        };

        writer
            .write_segment(&segment, Some("Erik"))
            .unwrap_or_else(|e| panic!("{e}"));

        let date = Local::now().date_naive();
        let file = dir.path().join(format!("{}.md", date.format("%Y-%m-%d")));
        let content = fs::read_to_string(&file).unwrap_or_else(|e| panic!("{e}"));
        assert!(content.contains("Erik"));
    }

    #[test]
    fn deduplicates_overlap() {
        let writer = MarkdownWriter {
            output_dir: PathBuf::from("/tmp"),
            timestamps: true,
            current_date: None,
            last_trailing_words: vec![
                "the".to_string(),
                "quick".to_string(),
                "brown".to_string(),
                "fox".to_string(),
            ],
        };

        let result = writer.deduplicate_overlap("brown fox jumps over");
        assert_eq!(result, "jumps over");
    }

    #[test]
    fn no_overlap_passes_through() {
        let writer = MarkdownWriter {
            output_dir: PathBuf::from("/tmp"),
            timestamps: true,
            current_date: None,
            last_trailing_words: vec!["hello".to_string(), "world".to_string()],
        };

        let result = writer.deduplicate_overlap("completely different text");
        assert_eq!(result, "completely different text");
    }

    #[test]
    fn empty_trailing_passes_through() {
        let writer = MarkdownWriter {
            output_dir: PathBuf::from("/tmp"),
            timestamps: true,
            current_date: None,
            last_trailing_words: Vec::new(),
        };

        let result = writer.deduplicate_overlap("first segment");
        assert_eq!(result, "first segment");
    }
}
