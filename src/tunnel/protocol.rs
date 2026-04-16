//! TunnelProtocol trait and built-in implementations.

use async_trait::async_trait;
use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("encryption failed: {0}")]
    Encrypt(String),
    #[error("decryption failed: {0}")]
    Decrypt(String),
    #[error("invalid key length: expected {expected}, got {got}")]
    InvalidKeyLength { expected: usize, got: usize },
}

/// Trait for tunnel encryption/decryption.
#[async_trait]
pub trait TunnelProtocol: Send + Sync {
    fn name(&self) -> &'static str;
    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>, ProtocolError>;
    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, ProtocolError>;
}

/// ChaCha20-Poly1305 cipher.
///
/// Uses a counter-based nonce: first 4 bytes are a packet counter (incremented
/// per encrypt call), last 8 bytes are a fixed IV shared during key exchange.
pub struct ChaCha20Protocol {
    cipher: ChaCha20Poly1305,
    /// Fixed 8-byte IV — the second half of the 12-byte ChaCha20 nonce.
    fixed_iv: [u8; 8],
    /// Packet counter — incremented atomically for each encrypt call.
    /// Forms the first 4 bytes of the 12-byte ChaCha20 nonce.
    counter: Mutex<u64>,
}

impl ChaCha20Protocol {
    pub fn new(key: &[u8], iv: &[u8]) -> Result<Self, ProtocolError> {
        if key.len() != 32 {
            return Err(ProtocolError::InvalidKeyLength {
                expected: 32,
                got: key.len(),
            });
        }
        if iv.len() != 8 {
            return Err(ProtocolError::InvalidKeyLength {
                expected: 8,
                got: iv.len(),
            });
        }

        let key = chacha20poly1305::Key::from_slice(key);
        let cipher = ChaCha20Poly1305::new(key);
        let mut fixed_iv = [0u8; 8];
        fixed_iv.copy_from_slice(iv);

        Ok(Self {
            cipher,
            fixed_iv,
            counter: Mutex::new(0),
        })
    }

    /// Generate a random 32-byte key and 8-byte IV.
    /// The counter starts at 0 and is managed internally.
    pub fn generate_keypair() -> (Vec<u8>, Vec<u8>) {
        let mut key = vec![0u8; 32];
        let mut iv = vec![0u8; 8];
        OsRng.fill_bytes(&mut key);
        OsRng.fill_bytes(&mut iv);
        (key, iv)
    }

    /// Build a 12-byte ChaCha20 nonce: first 4 bytes = packet counter, last 8 bytes = fixed IV.
    fn build_nonce(&self, counter_val: u64) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        // First 4 bytes: packet counter (u32 big-endian)
        nonce[0..4].copy_from_slice(&(counter_val as u32).to_be_bytes());
        // Last 8 bytes: fixed IV
        nonce[4..12].copy_from_slice(&self.fixed_iv);
        nonce
    }
}

#[async_trait]
impl TunnelProtocol for ChaCha20Protocol {
    fn name(&self) -> &'static str {
        "chacha20-poly1305"
    }

    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        let counter_val = {
            let mut counter = self.counter.lock().unwrap();
            let val = *counter;
            *counter = counter.checked_add(1).ok_or_else(|| {
                ProtocolError::Encrypt("packet counter overflow".into())
            })?;
            val
        };

        let nonce_bytes = self.build_nonce(counter_val);
        let nonce = Nonce::from_slice(&nonce_bytes);
        self.cipher
            .encrypt(nonce, data)
            .map_err(|e| ProtocolError::Encrypt(e.to_string()))
    }

    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        // Advance the counter so encrypt and decrypt stay in sync.
        // Both sides of the connection increment the counter per message.
        let counter_val = {
            let mut counter = self.counter.lock().unwrap();
            let val = *counter;
            *counter = counter.checked_add(1).ok_or_else(|| {
                ProtocolError::Decrypt("packet counter overflow".into())
            })?;
            val
        };

        let nonce_bytes = self.build_nonce(counter_val);
        let nonce = Nonce::from_slice(&nonce_bytes);
        self.cipher
            .decrypt(nonce, data)
            .map_err(|e| ProtocolError::Decrypt(e.to_string()))
    }
}

/// Plaintext (no encryption) — useful for testing.
pub struct PlainProtocol;

#[async_trait]
impl TunnelProtocol for PlainProtocol {
    fn name(&self) -> &'static str {
        "plaintext"
    }

    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        Ok(data.to_vec())
    }

    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        Ok(data.to_vec())
    }
}
