use openssl::symm::{encrypt_aead, decrypt_aead, Cipher};
use openssl::rand::rand_bytes;

/// Encryptor provides authenticated encryption for ORAM objects using AES-128-GCM
/// Each encrypted object has the following structure:
/// - IV (16 bytes): Random initialization vector
/// - Tag (16 bytes): Authentication tag
/// - Ciphertext (same length as the plaintext object): Encrypted data
pub struct Encryptor {
    /// The cipher algorithm (AES-128-GCM)
    cipher: Cipher,
    /// Encryption key (16 bytes)
    key: Vec<u8>,
}

impl Default for Encryptor {
    fn default() -> Self {
        Self::new()
    }
}

impl Encryptor {
    /// Creates a new encryptor instance with a fixed key
    /// Note: In production, the key should be securely generated and managed
    pub fn new() -> Encryptor {
        let cipher = Cipher::aes_128_gcm();
        let key = b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0A\x0B\x0C\x0D\x0E\x0F".to_vec();
        Encryptor { cipher, key }
    }

    /// Encrypts data into a pre-allocated result buffer
    /// Format: [IV (16 bytes)][Tag (16 bytes)][Ciphertext]
    /// - Generates random IV
    /// - Encrypts data with AES-128-GCM
    /// - Writes IV, tag, and ciphertext to result buffer
    pub fn encrypt_object_in(&self, data: &[u8], result: &mut [u8]) {
        let mut iv = vec![0u8; 16];
        rand_bytes(&mut iv).unwrap();
        let mut tag = vec![0u8; 16];
        let ciphertext = encrypt_aead(self.cipher, &self.key, Some(&iv), &[0u8], data, &mut tag).unwrap();
        result[..16].copy_from_slice(&iv);
        result[16..32].copy_from_slice(&tag);
        result[32..].copy_from_slice(&ciphertext);
    }

    /// Decrypts an encrypted object into a pre-allocated result buffer
    /// - Extracts IV and authentication tag
    /// - Verifies authenticity and decrypts
    /// - Writes plaintext to result buffer
    pub fn decrypt_object_in(&self, encrypted: &[u8], result: &mut [u8]){
        let iv = &encrypted[..16];
        let tag = &encrypted[16..32];
        let ciphertext = &encrypted[32..];
        let plaintext = decrypt_aead(self.cipher, &self.key, Some(iv), &[0u8], ciphertext, tag).unwrap();
        result.copy_from_slice(&plaintext);
    }
}
