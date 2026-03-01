#[cfg(all(feature = "cuda", feature = "rocm"))]
compile_error!("features `cuda` and `rocm` are mutually exclusive — use --no-default-features --features rocm for AMD GPUs");

#[cfg(all(feature = "nogpu", feature = "cuda"))]
compile_error!("features `nogpu` and `cuda` conflict — use --no-default-features --features nogpu to disable GPU");

#[cfg(all(feature = "nogpu", feature = "rocm"))]
compile_error!("features `nogpu` and `rocm` conflict");

pub mod audio;
pub mod config;
pub mod error;
pub mod mcp;
pub mod models;
pub mod net;
pub mod output;
pub mod recording;
pub mod speaker;
pub mod stt;
pub mod vcs;
