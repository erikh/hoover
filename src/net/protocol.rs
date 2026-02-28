use std::collections::BTreeMap;

use crate::error::{HooverError, Result};
use crate::net::crypto::CryptoContext;

/// Message types in the UDP protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    AudioData = 0x01,
    PassphraseChangeRequest = 0x02,
    PassphraseChangeAck = 0x03,
    EndOfStream = 0xFF,
}

impl MessageType {
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::AudioData),
            0x02 => Some(Self::PassphraseChangeRequest),
            0x03 => Some(Self::PassphraseChangeAck),
            0xFF => Some(Self::EndOfStream),
            _ => None,
        }
    }
}

/// A decrypted message from a UDP packet.
#[derive(Debug, Clone)]
pub struct DecodedMessage {
    pub serial: u64,
    pub message_type: MessageType,
    pub data: Vec<u8>,
}

/// Wire format:
/// ```text
/// [ serial: u64 (8 bytes, big-endian, PLAINTEXT) ]
/// [ nonce: 12 bytes ]
/// [ ciphertext: variable (AES-256-GCM encrypted payload + 16-byte tag) ]
/// ```
///
/// Payload (after decryption):
/// ```text
/// [ message_type: u8 ]
/// [ data: variable ]
/// ```
///
/// Minimum packet size: 8 (serial) + 12 (nonce) + 1 (min ciphertext) + 16 (tag) = 37
const MIN_PACKET_SIZE: usize = 8 + 12 + 1 + 16;

/// Encode a message into a wire-format packet.
pub fn encode_packet(
    serial: u64,
    message_type: MessageType,
    data: &[u8],
    crypto: &CryptoContext,
) -> Result<Vec<u8>> {
    // Build plaintext payload
    let mut payload = Vec::with_capacity(1 + data.len());
    payload.push(message_type as u8);
    payload.extend_from_slice(data);

    // Encrypt
    let (ciphertext, nonce) = crypto.encrypt(&payload)?;

    // Build wire packet
    let mut packet = Vec::with_capacity(8 + 12 + ciphertext.len());
    packet.extend_from_slice(&serial.to_be_bytes());
    packet.extend_from_slice(&nonce);
    packet.extend_from_slice(&ciphertext);

    Ok(packet)
}

/// Decode a wire-format packet.
pub fn decode_packet(packet: &[u8], crypto: &CryptoContext) -> Result<DecodedMessage> {
    if packet.len() < MIN_PACKET_SIZE {
        return Err(HooverError::Network(format!(
            "packet too small: {} bytes (min {MIN_PACKET_SIZE})",
            packet.len()
        )));
    }

    // Extract serial (plaintext)
    let serial = u64::from_be_bytes(
        packet[..8]
            .try_into()
            .map_err(|_| HooverError::Network("invalid serial bytes".to_string()))?,
    );

    // Extract nonce
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&packet[8..20]);

    // Decrypt remainder
    let ciphertext = &packet[20..];
    let payload = crypto.decrypt(&nonce, ciphertext)?;

    if payload.is_empty() {
        return Err(HooverError::Network(
            "empty payload after decryption".to_string(),
        ));
    }

    let message_type = MessageType::from_u8(payload[0]).ok_or_else(|| {
        HooverError::Network(format!("unknown message type: 0x{:02x}", payload[0]))
    })?;

    let data = payload[1..].to_vec();

    Ok(DecodedMessage {
        serial,
        message_type,
        data,
    })
}

/// Manages serial ordering and buffering of out-of-order packets.
pub struct PacketOrderer {
    expected_serial: u64,
    buffer: BTreeMap<u64, DecodedMessage>,
    backlog: usize,
}

impl PacketOrderer {
    #[must_use]
    pub const fn new(backlog: usize) -> Self {
        Self {
            expected_serial: 0,
            buffer: BTreeMap::new(),
            backlog,
        }
    }

    /// Insert a decoded message, returning any messages now ready for processing (in order).
    pub fn insert(&mut self, msg: DecodedMessage) -> Vec<DecodedMessage> {
        if msg.serial < self.expected_serial {
            // Duplicate / old packet — discard
            tracing::debug!("discarding old packet serial={}", msg.serial);
            return Vec::new();
        }

        if msg.serial == self.expected_serial {
            // Process immediately, then drain consecutive buffered packets
            let mut ready = vec![msg];
            self.expected_serial += 1;

            while let Some(next) = self.buffer.remove(&self.expected_serial) {
                ready.push(next);
                self.expected_serial += 1;
            }

            return ready;
        }

        // Future packet — buffer it
        self.buffer.insert(msg.serial, msg);

        // If buffer exceeds backlog, drop oldest and advance expected
        if self.buffer.len() > self.backlog {
            self.drop_oldest();
        }

        Vec::new()
    }

    /// Get the next expected serial number.
    #[must_use]
    pub const fn expected_serial(&self) -> u64 {
        self.expected_serial
    }

    /// Number of buffered out-of-order packets.
    #[must_use]
    pub fn buffered_count(&self) -> usize {
        self.buffer.len()
    }

    fn drop_oldest(&mut self) {
        // Advance expected_serial to the oldest buffered serial
        if let Some(&oldest) = self.buffer.keys().next() {
            tracing::warn!(
                "backlog overflow: skipping serials {}..{oldest}",
                self.expected_serial
            );
            self.expected_serial = oldest;
            // Now drain consecutive from the new expected
            while let Some(entry) = self.buffer.remove(&self.expected_serial) {
                // Drop these — they were skipped due to overflow
                drop(entry);
                self.expected_serial += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_crypto() -> CryptoContext {
        CryptoContext::new(&[0xABu8; 32])
    }

    #[test]
    fn encode_decode_round_trip() {
        let crypto = test_crypto();
        let data = b"hello audio data";

        let packet = encode_packet(42, MessageType::AudioData, data, &crypto)
            .unwrap_or_else(|e| panic!("{e}"));
        let decoded = decode_packet(&packet, &crypto).unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(decoded.serial, 42);
        assert_eq!(decoded.message_type, MessageType::AudioData);
        assert_eq!(decoded.data, data);
    }

    #[test]
    fn wrong_key_rejects() {
        let crypto1 = CryptoContext::new(&[1u8; 32]);
        let crypto2 = CryptoContext::new(&[2u8; 32]);

        let packet = encode_packet(0, MessageType::AudioData, b"data", &crypto1)
            .unwrap_or_else(|e| panic!("{e}"));
        let result = decode_packet(&packet, &crypto2);
        assert!(result.is_err());
    }

    #[test]
    fn orderer_in_order() {
        let mut orderer = PacketOrderer::new(100);

        for i in 0..5 {
            let msg = DecodedMessage {
                serial: i,
                message_type: MessageType::AudioData,
                data: vec![i as u8],
            };
            let ready = orderer.insert(msg);
            assert_eq!(ready.len(), 1);
            assert_eq!(ready[0].serial, i);
        }
    }

    #[test]
    fn orderer_out_of_order() {
        let mut orderer = PacketOrderer::new(100);

        // Send packets 2, 1, 0 (reversed)
        let msg2 = DecodedMessage {
            serial: 2,
            message_type: MessageType::AudioData,
            data: vec![2],
        };
        assert!(orderer.insert(msg2).is_empty());

        let msg1 = DecodedMessage {
            serial: 1,
            message_type: MessageType::AudioData,
            data: vec![1],
        };
        assert!(orderer.insert(msg1).is_empty());

        // Now send serial 0 — should drain all three
        let msg0 = DecodedMessage {
            serial: 0,
            message_type: MessageType::AudioData,
            data: vec![0],
        };
        let ready = orderer.insert(msg0);
        assert_eq!(ready.len(), 3);
        assert_eq!(ready[0].serial, 0);
        assert_eq!(ready[1].serial, 1);
        assert_eq!(ready[2].serial, 2);
    }

    #[test]
    fn orderer_discards_duplicates() {
        let mut orderer = PacketOrderer::new(100);

        let msg0 = DecodedMessage {
            serial: 0,
            message_type: MessageType::AudioData,
            data: vec![0],
        };
        assert_eq!(orderer.insert(msg0).len(), 1);

        // Duplicate serial 0
        let msg0_dup = DecodedMessage {
            serial: 0,
            message_type: MessageType::AudioData,
            data: vec![0],
        };
        assert!(orderer.insert(msg0_dup).is_empty());
    }

    #[test]
    fn orderer_backlog_overflow() {
        let mut orderer = PacketOrderer::new(3);

        // Buffer serials 1, 2, 3 (expected is 0)
        for i in 1..=3 {
            orderer.insert(DecodedMessage {
                serial: i,
                message_type: MessageType::AudioData,
                data: vec![i as u8],
            });
        }
        assert_eq!(orderer.buffered_count(), 3);

        // Insert serial 4 — triggers overflow
        orderer.insert(DecodedMessage {
            serial: 4,
            message_type: MessageType::AudioData,
            data: vec![4],
        });

        // Expected serial should have advanced past the dropped packets
        assert!(orderer.expected_serial() > 0);
    }
}
