use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::sync::mpsc;

use crate::audio::buffer::AudioChunk;
use crate::config::UdpConfig;
use crate::error::{HooverError, Result};
use crate::net::crypto::CryptoContext;
use crate::net::firewall::FirewallManager;
use crate::net::protocol::{DecodedMessage, MessageType, PacketOrderer, decode_packet};

/// UDP audio receiver server.
pub struct UdpServer {
    socket: Arc<UdpSocket>,
    crypto: Arc<Mutex<CryptoContext>>,
    orderer: PacketOrderer,
    firewall: Option<FirewallManager>,
    chunk_tx: mpsc::Sender<AudioChunk>,
    audio_buffer: Vec<i16>,
}

impl UdpServer {
    pub async fn bind(config: &UdpConfig, chunk_tx: mpsc::Sender<AudioChunk>) -> Result<Self> {
        let socket = UdpSocket::bind(&config.bind).await.map_err(|e| {
            HooverError::Network(format!("failed to bind UDP socket to {}: {e}", config.bind))
        })?;

        tracing::info!("UDP server listening on {}", config.bind);

        let key_path = crate::config::Config::expand_path(&config.key_file);
        let crypto = CryptoContext::from_key_file(&key_path)?;

        let firewall = if config.firewall.enabled {
            Some(FirewallManager::new(&config.firewall))
        } else {
            None
        };

        Ok(Self {
            socket: Arc::new(socket),
            crypto: Arc::new(Mutex::new(crypto)),
            orderer: PacketOrderer::new(config.backlog),
            firewall,
            chunk_tx,
            audio_buffer: Vec::new(),
        })
    }

    /// Run the server loop. This blocks until the provided cancellation signal fires.
    pub async fn run(&mut self, mut cancel: tokio::sync::watch::Receiver<bool>) -> Result<()> {
        let mut buf = vec![0u8; 65536];

        loop {
            tokio::select! {
                result = self.socket.recv_from(&mut buf) => {
                    match result {
                        Ok((len, addr)) => {
                            self.handle_packet(&buf[..len], addr).await;
                        }
                        Err(e) => {
                            tracing::error!("UDP recv error: {e}");
                        }
                    }
                }
                _ = cancel.changed() => {
                    tracing::info!("UDP server shutting down");
                    // Flush any remaining audio
                    self.flush_audio_buffer();
                    break;
                }
            }
        }

        Ok(())
    }

    async fn handle_packet(&mut self, data: &[u8], addr: SocketAddr) {
        let crypto = self.crypto.lock().await;
        let decoded = match decode_packet(data, &crypto) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::warn!("failed to decode packet from {addr}: {e}");
                drop(crypto);
                // Trigger firewall block on decryption failure
                if let Some(ref mut fw) = self.firewall {
                    fw.block_ip(addr.ip()).await;
                }
                return;
            }
        };
        drop(crypto);

        match decoded.message_type {
            MessageType::PassphraseChangeRequest => {
                self.handle_passphrase_change(decoded, addr).await;
            }
            MessageType::EndOfStream => {
                tracing::info!("end of stream from {addr}");
                self.flush_audio_buffer();
            }
            _ => {
                // Process through orderer
                let ready = self.orderer.insert(decoded);
                for msg in &ready {
                    self.process_message(msg);
                }
            }
        }
    }

    fn process_message(&mut self, msg: &DecodedMessage) {
        const SAMPLES_PER_CHUNK: usize = 16000;

        if msg.message_type != MessageType::AudioData {
            return;
        }

        // Convert bytes to i16 samples (little-endian)
        let samples: Vec<i16> = msg
            .data
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        self.audio_buffer.extend_from_slice(&samples);

        // Once we have ~1 second of audio (16000 samples), emit a chunk
        while self.audio_buffer.len() >= SAMPLES_PER_CHUNK {
            let chunk_i16: Vec<i16> = self.audio_buffer.drain(..SAMPLES_PER_CHUNK).collect();
            let chunk_f32: Vec<f32> = chunk_i16
                .iter()
                .map(|&s| f32::from(s) / f32::from(i16::MAX))
                .collect();

            let audio_chunk = AudioChunk {
                samples_f32: chunk_f32,
                samples_i16: chunk_i16,
                timestamp: chrono::Utc::now(),
                duration_secs: 1.0,
            };

            if self.chunk_tx.blocking_send(audio_chunk).is_err() {
                tracing::debug!("chunk receiver dropped");
                return;
            }
        }
    }

    fn flush_audio_buffer(&mut self) {
        if self.audio_buffer.is_empty() {
            return;
        }

        let chunk_i16: Vec<i16> = self.audio_buffer.drain(..).collect();
        let duration = chunk_i16.len() as f32 / 16000.0;
        let chunk_f32: Vec<f32> = chunk_i16
            .iter()
            .map(|&s| f32::from(s) / f32::from(i16::MAX))
            .collect();

        let audio_chunk = AudioChunk {
            samples_f32: chunk_f32,
            samples_i16: chunk_i16,
            timestamp: chrono::Utc::now(),
            duration_secs: duration,
        };

        let _ = self.chunk_tx.blocking_send(audio_chunk);
    }

    async fn handle_passphrase_change(&self, msg: DecodedMessage, addr: SocketAddr) {
        if msg.data.len() != 32 {
            tracing::warn!("invalid passphrase change request from {addr}: wrong key length");
            return;
        }

        let mut new_key = [0u8; 32];
        new_key.copy_from_slice(&msg.data);

        let mut crypto = self.crypto.lock().await;
        crypto.update_key(&new_key);
        tracing::info!("passphrase updated from request by {addr}");

        // Send ack
        let ack = crate::net::protocol::encode_packet(
            msg.serial,
            MessageType::PassphraseChangeAck,
            &[],
            &crypto,
        );

        drop(crypto);

        if let Ok(packet) = ack {
            let _ = self.socket.send_to(&packet, addr).await;
        }
    }
}
