use std::io::Read;
use std::net::SocketAddr;
use std::path::Path;

use tokio::net::UdpSocket;

use crate::config::Config;
use crate::error::{HooverError, Result};
use crate::net::crypto::CryptoContext;
use crate::net::protocol::{MessageType, encode_packet};

/// Maximum audio payload per UDP packet (keep under typical MTU).
const MAX_PAYLOAD_SIZE: usize = 1400;

/// Run the UDP sender (`hoover send`).
pub async fn run_sender(
    config: &Config,
    target: &str,
    file: Option<&Path>,
    key_file_override: Option<&Path>,
) -> Result<()> {
    let target_addr: SocketAddr = target
        .parse()
        .map_err(|e| HooverError::Network(format!("invalid target address '{target}': {e}")))?;

    let key_path = key_file_override.map_or_else(
        || Config::expand_path(&config.udp.key_file),
        std::path::Path::to_path_buf,
    );

    let crypto = CryptoContext::from_key_file(&key_path)?;

    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| HooverError::Network(format!("failed to bind sender socket: {e}")))?;

    let audio_data = read_audio_data(file)?;

    tracing::info!(
        "sending {} bytes of audio to {target_addr}",
        audio_data.len()
    );

    let mut serial: u64 = 0;

    // Send audio in chunks
    for chunk in audio_data.chunks(MAX_PAYLOAD_SIZE) {
        let packet = encode_packet(serial, MessageType::AudioData, chunk, &crypto)?;
        socket
            .send_to(&packet, target_addr)
            .await
            .map_err(|e| HooverError::Network(format!("send failed: {e}")))?;
        serial += 1;

        // Small delay to avoid overwhelming the network
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
    }

    // Send end-of-stream marker
    let eos_packet = encode_packet(serial, MessageType::EndOfStream, &[], &crypto)?;
    socket
        .send_to(&eos_packet, target_addr)
        .await
        .map_err(|e| HooverError::Network(format!("failed to send EOS: {e}")))?;

    tracing::info!("sent {serial} packets + EOS to {target_addr}");
    Ok(())
}

/// Read audio data from a file or stdin.
///
/// If a WAV file is provided, reads the raw PCM data.
/// If stdin is used, reads raw bytes (expected to be i16 LE PCM 16kHz mono).
fn read_audio_data(file: Option<&Path>) -> Result<Vec<u8>> {
    if let Some(path) = file {
        // Try to read as WAV
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
        {
            return read_wav_pcm(path);
        }

        // Raw PCM file
        std::fs::read(path).map_err(|e| {
            HooverError::Network(format!("failed to read audio file {}: {e}", path.display()))
        })
    } else {
        // Read from stdin
        let mut data = Vec::new();
        std::io::stdin()
            .read_to_end(&mut data)
            .map_err(|e| HooverError::Network(format!("failed to read from stdin: {e}")))?;
        Ok(data)
    }
}

fn read_wav_pcm(path: &Path) -> Result<Vec<u8>> {
    let mut reader = hound::WavReader::open(path).map_err(|e| {
        HooverError::Network(format!("failed to open WAV file {}: {e}", path.display()))
    })?;

    let spec = reader.spec();

    // Convert to i16 LE bytes
    let samples: Vec<i16> = if spec.sample_format == hound::SampleFormat::Float {
        reader
            .samples::<f32>()
            .map(|s| {
                let s = s.map_err(|e| HooverError::Network(format!("WAV read error: {e}")))?;
                let clamped = s.clamp(-1.0, 1.0);
                Ok((clamped * f32::from(i16::MAX)) as i16)
            })
            .collect::<Result<Vec<i16>>>()?
    } else {
        reader
            .samples::<i16>()
            .map(|s| s.map_err(|e| HooverError::Network(format!("WAV read error: {e}"))))
            .collect::<Result<Vec<i16>>>()?
    };

    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in &samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }

    Ok(bytes)
}

/// Initiate a passphrase change with a remote server.
pub async fn change_passphrase(
    socket: &UdpSocket,
    target: SocketAddr,
    serial: u64,
    current_crypto: &CryptoContext,
    new_key: &[u8; 32],
) -> Result<()> {
    let packet = encode_packet(
        serial,
        MessageType::PassphraseChangeRequest,
        new_key,
        current_crypto,
    )?;

    socket
        .send_to(&packet, target)
        .await
        .map_err(|e| HooverError::Network(format!("failed to send passphrase change: {e}")))?;

    tracing::info!("sent passphrase change request to {target}");
    Ok(())
}
