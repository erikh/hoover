use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;

use crate::error::{HooverError, Result};

/// AES-256-GCM encryption context.
pub struct CryptoContext {
    cipher: Aes256Gcm,
    key_bytes: [u8; 32],
}

impl CryptoContext {
    /// Create a context from a 32-byte key.
    #[must_use]
    pub fn new(key: &[u8; 32]) -> Self {
        let cipher_key = Key::<Aes256Gcm>::from_slice(key);
        let cipher = Aes256Gcm::new(cipher_key);
        Self {
            cipher,
            key_bytes: *key,
        }
    }

    /// Load a key from a file (must be exactly 32 bytes).
    pub fn from_key_file(path: &Path) -> Result<Self> {
        let data = std::fs::read(path).map_err(|e| {
            HooverError::Crypto(format!("failed to read key file {}: {e}", path.display()))
        })?;

        if data.len() != 32 {
            return Err(HooverError::Crypto(format!(
                "key file must be exactly 32 bytes, got {}",
                data.len()
            )));
        }

        let mut key = [0u8; 32];
        key.copy_from_slice(&data);
        Ok(Self::new(&key))
    }

    /// Generate a random 12-byte nonce.
    #[must_use]
    pub fn generate_nonce() -> [u8; 12] {
        let mut nonce = [0u8; 12];
        rand::rng().fill_bytes(&mut nonce);
        nonce
    }

    /// Encrypt plaintext, returning nonce + ciphertext (with GCM tag appended).
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, [u8; 12])> {
        let nonce_bytes = Self::generate_nonce();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| HooverError::Crypto(format!("encryption failed: {e}")))?;

        Ok((ciphertext, nonce_bytes))
    }

    /// Decrypt ciphertext given a nonce. Returns plaintext.
    pub fn decrypt(&self, nonce: &[u8; 12], ciphertext: &[u8]) -> Result<Vec<u8>> {
        let nonce = Nonce::from_slice(nonce);

        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| HooverError::Crypto(format!("decryption failed: {e}")))
    }

    /// Update the encryption key (for passphrase negotiation).
    pub fn update_key(&mut self, new_key: &[u8; 32]) {
        let cipher_key = Key::<Aes256Gcm>::from_slice(new_key);
        self.cipher = Aes256Gcm::new(cipher_key);
        self.key_bytes = *new_key;
    }

    /// Get the raw key bytes (for passphrase negotiation).
    #[must_use]
    pub const fn key_bytes(&self) -> &[u8; 32] {
        &self.key_bytes
    }
}

/// Generate a new random 32-byte key and write it to a file.
pub fn generate_key_file(path: &Path) -> Result<()> {
    let mut key = [0u8; 32];
    rand::rng().fill_bytes(&mut key);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(path, key).map_err(|e| {
        HooverError::Crypto(format!("failed to write key file {}: {e}", path.display()))
    })?;

    // Restrict permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_round_trip() {
        let key = [42u8; 32];
        let ctx = CryptoContext::new(&key);

        let plaintext = b"hello, encrypted world!";
        let (ciphertext, nonce) = ctx.encrypt(plaintext).unwrap_or_else(|e| panic!("{e}"));
        let decrypted = ctx
            .decrypt(&nonce, &ciphertext)
            .unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails_decryption() {
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];
        let ctx1 = CryptoContext::new(&key1);
        let ctx2 = CryptoContext::new(&key2);

        let plaintext = b"secret data";
        let (ciphertext, nonce) = ctx1.encrypt(plaintext).unwrap_or_else(|e| panic!("{e}"));
        let result = ctx2.decrypt(&nonce, &ciphertext);

        assert!(result.is_err());
    }

    #[test]
    fn key_file_round_trip() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("test.key");

        generate_key_file(&path).unwrap_or_else(|e| panic!("{e}"));
        let ctx = CryptoContext::from_key_file(&path).unwrap_or_else(|e| panic!("{e}"));

        let plaintext = b"test data";
        let (ciphertext, nonce) = ctx.encrypt(plaintext).unwrap_or_else(|e| panic!("{e}"));
        let decrypted = ctx
            .decrypt(&nonce, &ciphertext)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn key_file_wrong_size_rejected() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("bad.key");
        std::fs::write(&path, &[0u8; 16]).unwrap_or_else(|e| panic!("{e}"));

        let result = CryptoContext::from_key_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn key_update() {
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];
        let mut ctx = CryptoContext::new(&key1);

        let plaintext = b"data";
        let (ciphertext, nonce) = ctx.encrypt(plaintext).unwrap_or_else(|e| panic!("{e}"));

        ctx.update_key(&key2);

        // Old ciphertext should fail with new key
        assert!(ctx.decrypt(&nonce, &ciphertext).is_err());

        // New encryption should work with new key
        let (ciphertext2, nonce2) = ctx.encrypt(plaintext).unwrap_or_else(|e| panic!("{e}"));
        let decrypted = ctx
            .decrypt(&nonce2, &ciphertext2)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(decrypted, plaintext);
    }
}
