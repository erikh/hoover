use std::io;

#[derive(Debug, thiserror::Error)]
pub enum HooverError {
    #[error("audio error: {0}")]
    Audio(String),

    #[error("resample error: {0}")]
    Resample(String),

    #[error("STT error: {0}")]
    Stt(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("output error: {0}")]
    Output(String),

    #[error("git error: {0}")]
    Git(#[from] git2::Error),

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("firewall error: {0}")]
    Firewall(String),

    #[error("speaker identification error: {0}")]
    Speaker(String),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, HooverError>;
