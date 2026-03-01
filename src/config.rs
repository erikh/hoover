use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{HooverError, Result};

const fn default_chunk_duration_secs() -> u64 {
    30
}

const fn default_overlap_secs() -> u64 {
    5
}

fn default_stt_backend() -> String {
    "whisper".to_string()
}

fn default_language() -> String {
    "en".to_string()
}

fn default_whisper_model_size() -> String {
    "base".to_string()
}

fn default_openai_model() -> String {
    "whisper-1".to_string()
}

const fn default_min_confidence() -> f32 {
    0.7
}

fn default_output_directory() -> String {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join("hoover").to_string_lossy().to_string()
}

const fn default_true() -> bool {
    true
}

fn default_remote() -> String {
    "origin".to_string()
}

fn default_bind() -> String {
    "0.0.0.0:9700".to_string()
}

fn default_key_file() -> String {
    let config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".config"));
    config_dir
        .join("hoover/udp.key")
        .to_string_lossy()
        .to_string()
}

const fn default_backlog() -> usize {
    1000
}

fn default_firewall_backend() -> String {
    "firewalld".to_string()
}

const fn default_block_duration_secs() -> u64 {
    3600
}

fn default_profiles_dir() -> String {
    let data_dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".local/share"));
    data_dir
        .join("hoover/speakers")
        .to_string_lossy()
        .to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub audio: AudioConfig,

    #[serde(default)]
    pub stt: SttConfig,

    #[serde(default)]
    pub speaker: SpeakerConfig,

    #[serde(default)]
    pub output: OutputConfig,

    #[serde(default)]
    pub vcs: VcsConfig,

    #[serde(default)]
    pub udp: UdpConfig,

    #[serde(default)]
    pub mcp: McpConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AudioConfig {
    pub device: Option<String>,

    #[serde(default = "default_chunk_duration_secs")]
    pub chunk_duration_secs: u64,

    #[serde(default = "default_overlap_secs")]
    pub overlap_secs: u64,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            device: None,
            chunk_duration_secs: default_chunk_duration_secs(),
            overlap_secs: default_overlap_secs(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SttConfig {
    #[serde(default = "default_stt_backend")]
    pub backend: String,

    #[serde(default = "default_language")]
    pub language: String,

    #[serde(default = "default_whisper_model_size")]
    pub whisper_model_size: String,

    pub model_path: Option<String>,

    pub openai_api_key: Option<String>,

    #[serde(default = "default_openai_model")]
    pub openai_model: String,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            backend: default_stt_backend(),
            language: default_language(),
            whisper_model_size: default_whisper_model_size(),
            model_path: None,
            openai_api_key: None,
            openai_model: default_openai_model(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpeakerConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_profiles_dir")]
    pub profiles_dir: String,

    #[serde(default = "default_min_confidence")]
    pub min_confidence: f32,

    #[serde(default)]
    pub filter_unknown: bool,

    pub model_path: Option<String>,
}

impl Default for SpeakerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            profiles_dir: default_profiles_dir(),
            min_confidence: default_min_confidence(),
            filter_unknown: false,
            model_path: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OutputConfig {
    #[serde(default = "default_output_directory")]
    pub directory: String,

    #[serde(default = "default_true")]
    pub timestamps: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            directory: default_output_directory(),
            timestamps: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct VcsConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub auto_commit: bool,

    #[serde(default)]
    pub auto_push: bool,

    #[serde(default = "default_remote")]
    pub remote: String,

    pub github: Option<GithubConfig>,

    pub gitea: Option<GiteaConfig>,
}

impl Default for VcsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_commit: false,
            auto_push: false,
            remote: default_remote(),
            github: None,
            gitea: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GithubConfig {
    pub token: Option<String>,
    pub owner: Option<String>,
    pub repo: Option<String>,
    pub workflow: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GiteaConfig {
    pub url: Option<String>,
    pub token: Option<String>,
    pub owner: Option<String>,
    pub repo: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UdpConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_bind")]
    pub bind: String,

    #[serde(default = "default_key_file")]
    pub key_file: String,

    #[serde(default = "default_backlog")]
    pub backlog: usize,

    #[serde(default)]
    pub firewall: FirewallConfig,
}

impl Default for UdpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: default_bind(),
            key_file: default_key_file(),
            backlog: default_backlog(),
            firewall: FirewallConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FirewallConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_firewall_backend")]
    pub backend: String,

    #[serde(default = "default_block_duration_secs")]
    pub block_duration_secs: u64,
}

impl Default for FirewallConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: default_firewall_backend(),
            block_duration_secs: default_block_duration_secs(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct McpConfig {
    #[serde(default)]
    pub enabled: bool,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(HooverError::Config(format!(
                "config file not found: {} — create it or use --config to specify a path",
                path.display()
            )));
        }

        let contents = std::fs::read_to_string(path).map_err(|e| {
            HooverError::Config(format!(
                "failed to read config file {}: {e}",
                path.display()
            ))
        })?;

        let config: Self = serde_yaml_ng::from_str(&contents).map_err(|e| {
            HooverError::Config(format!(
                "failed to parse config file {}: {e}",
                path.display()
            ))
        })?;

        Ok(config)
    }

    #[must_use]
    pub fn default_path() -> PathBuf {
        let config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".config"));
        config_dir.join("hoover/config.yaml")
    }

    /// Expand `~` in a path string to the user's home directory.
    #[must_use]
    pub fn expand_path(path: &str) -> PathBuf {
        if let Some(rest) = path.strip_prefix("~/")
            && let Some(home) = dirs::home_dir()
        {
            return home.join(rest);
        }
        PathBuf::from(path)
    }

    /// Update the `audio.device` field in the config file, preserving all other content.
    pub fn set_audio_device(config_path: &Path, device: &str) -> Result<()> {
        let contents = if config_path.exists() {
            std::fs::read_to_string(config_path).map_err(|e| {
                HooverError::Config(format!(
                    "failed to read config file {}: {e}",
                    config_path.display()
                ))
            })?
        } else {
            String::from("{}")
        };

        let mut value: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&contents).map_err(|e| {
                HooverError::Config(format!(
                    "failed to parse config file {}: {e}",
                    config_path.display()
                ))
            })?;

        let map = value
            .as_mapping_mut()
            .ok_or_else(|| HooverError::Config("config root is not a mapping".to_string()))?;

        let audio_key = serde_yaml_ng::Value::String("audio".to_string());
        let audio = map
            .entry(audio_key)
            .or_insert_with(|| serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new()));

        let audio_map = audio
            .as_mapping_mut()
            .ok_or_else(|| HooverError::Config("audio section is not a mapping".to_string()))?;

        audio_map.insert(
            serde_yaml_ng::Value::String("device".to_string()),
            serde_yaml_ng::Value::String(device.to_string()),
        );

        let yaml = serde_yaml_ng::to_string(&value).map_err(|e| {
            HooverError::Config(format!("failed to serialize config: {e}"))
        })?;

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                HooverError::Config(format!(
                    "failed to create config directory {}: {e}",
                    parent.display()
                ))
            })?;
        }

        std::fs::write(config_path, yaml).map_err(|e| {
            HooverError::Config(format!(
                "failed to write config file {}: {e}",
                config_path.display()
            ))
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config() {
        let yaml = "{}";
        let config: Config =
            serde_yaml_ng::from_str(yaml).unwrap_or_else(|e| panic!("parse failed: {e}"));
        assert_eq!(config.audio.chunk_duration_secs, 30);
        assert_eq!(config.stt.backend, "whisper");
        assert!(!config.speaker.enabled);
    }

    #[test]
    fn parse_full_config() {
        let yaml = r#"
audio:
  device: "My Microphone"
  chunk_duration_secs: 15
  overlap_secs: 3

stt:
  backend: vosk
  language: de
  model_path: /models/vosk-de

speaker:
  enabled: true
  min_confidence: 0.8
  filter_unknown: true

output:
  directory: /tmp/hoover-test
  timestamps: false

vcs:
  enabled: true
  auto_commit: true
  auto_push: false
  remote: upstream
  github:
    token: ghp_xxx
    owner: erikh
    repo: hoover
    workflow: ci.yml

udp:
  enabled: true
  bind: "127.0.0.1:9800"
  backlog: 500
  firewall:
    enabled: true
    backend: nftables
    block_duration_secs: 7200

mcp:
  enabled: true
"#;
        let config: Config =
            serde_yaml_ng::from_str(yaml).unwrap_or_else(|e| panic!("parse failed: {e}"));
        assert_eq!(config.audio.device.as_deref(), Some("My Microphone"));
        assert_eq!(config.audio.chunk_duration_secs, 15);
        assert_eq!(config.stt.backend, "vosk");
        assert_eq!(config.stt.language, "de");
        assert!(config.speaker.enabled);
        assert!(config.speaker.filter_unknown);
        assert_eq!(config.output.directory, "/tmp/hoover-test");
        assert!(!config.output.timestamps);
        assert!(config.vcs.enabled);
        assert!(config.vcs.auto_commit);
        assert!(config.vcs.github.is_some());
        assert!(config.udp.enabled);
        assert_eq!(config.udp.bind, "127.0.0.1:9800");
        assert!(config.udp.firewall.enabled);
        assert_eq!(config.udp.firewall.backend, "nftables");
        assert!(config.mcp.enabled);
    }

    #[test]
    fn missing_config_file_gives_error() {
        let result = Config::load(Path::new("/nonexistent/config.yaml"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("config file not found"));
    }

    #[test]
    fn expand_tilde_path() {
        let expanded = Config::expand_path("~/hoover");
        assert!(!expanded.to_string_lossy().starts_with('~'));
    }

    #[test]
    fn parse_minimal_github_config() {
        let yaml = r#"
vcs:
  enabled: true
  github: {}
"#;
        let config: Config =
            serde_yaml_ng::from_str(yaml).unwrap_or_else(|e| panic!("parse failed: {e}"));
        assert!(config.vcs.enabled);
        let gh = config.vcs.github.unwrap_or_else(|| panic!("github missing"));
        assert!(gh.token.is_none());
        assert!(gh.owner.is_none());
        assert!(gh.repo.is_none());
    }

    #[test]
    fn set_audio_device_creates_and_updates() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("config.yaml");

        // Create from scratch
        Config::set_audio_device(&path, "My Mic").unwrap_or_else(|e| panic!("{e}"));
        let config = Config::load(&path).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(config.audio.device.as_deref(), Some("My Mic"));

        // Update existing
        Config::set_audio_device(&path, "Other Mic").unwrap_or_else(|e| panic!("{e}"));
        let config = Config::load(&path).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(config.audio.device.as_deref(), Some("Other Mic"));
    }
}
