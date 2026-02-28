use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Stream, StreamConfig};
use crossbeam_channel::{Receiver, bounded};

use crate::config::AudioConfig;
use crate::error::{HooverError, Result};

/// Manages microphone capture via cpal.
pub struct AudioCapture {
    stream: Stream,
    receiver: Receiver<Vec<f32>>,
    sample_rate: u32,
    channels: u16,
}

impl AudioCapture {
    pub fn new(config: &AudioConfig) -> Result<Self> {
        let host = cpal::default_host();

        let device = if let Some(ref name) = config.device {
            host.input_devices()
                .map_err(|e| HooverError::Audio(format!("failed to enumerate input devices: {e}")))?
                .find(|d| {
                    d.description()
                        .ok()
                        .map(|desc| desc.name().to_string())
                        .as_deref()
                        == Some(name.as_str())
                })
                .ok_or_else(|| HooverError::Audio(format!("input device not found: {name}")))?
        } else {
            host.default_input_device().ok_or_else(|| {
                HooverError::Audio("no default input device available".to_string())
            })?
        };

        let supported = device
            .default_input_config()
            .map_err(|e| HooverError::Audio(format!("failed to get default input config: {e}")))?;

        let sample_rate = supported.sample_rate();
        let channels = supported.channels();

        let stream_config = StreamConfig {
            channels,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        // Bounded channel — try_send in audio callback to avoid blocking
        let (tx, rx) = bounded::<Vec<f32>>(64);

        let err_fn = |err: cpal::StreamError| {
            tracing::error!("audio stream error: {err}");
        };

        let stream = device
            .build_input_stream(
                &stream_config,
                move |data: &[f32], _info: &cpal::InputCallbackInfo| {
                    // try_send to stay lock-free in the audio callback
                    let _ = tx.try_send(data.to_vec());
                },
                err_fn,
                None,
            )
            .map_err(|e| HooverError::Audio(format!("failed to build input stream: {e}")))?;

        Ok(Self {
            stream,
            receiver: rx,
            sample_rate,
            channels,
        })
    }

    /// Start the audio stream.
    pub fn start(&self) -> Result<()> {
        self.stream
            .play()
            .map_err(|e| HooverError::Audio(format!("failed to start audio stream: {e}")))
    }

    /// Stop the audio stream.
    pub fn pause(&self) -> Result<()> {
        self.stream
            .pause()
            .map_err(|e| HooverError::Audio(format!("failed to pause audio stream: {e}")))
    }

    #[must_use]
    pub fn receiver(&self) -> Receiver<Vec<f32>> {
        self.receiver.clone()
    }

    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub const fn channels(&self) -> u16 {
        self.channels
    }
}
