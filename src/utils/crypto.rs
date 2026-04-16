use aes_gcm::{
    aead::{Aead, KeyInit as AesKeyInit},
    Aes256Gcm, Key, Nonce
};
use hmac::{Hmac, Mac};
use hmac::digest::KeyInit as HmacKeyInit;
use sha2::Sha256;
use rand::RngCore;
use base64::{engine::general_purpose, Engine};
use serde::{Deserialize, Serialize};

type HmacSha256 = Hmac<Sha256>;

const NONCE_SIZE: usize = 12;

#[derive(Serialize, Deserialize, Debug)]
pub struct SecureEnvelope {
    pub preview: String,
    pub payload: String,
    pub signature: String,
}

pub fn encrypt_and_sign(key: &[u8; 32], plaintext: &str, preview: &str) -> Result<SecureEnvelope, String> {
    // 1. Encrypt
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|_| "Encryption failed")?;

    let mut combined = nonce_bytes.to_vec();
    combined.extend(ciphertext);
    let payload_b64 = general_purpose::STANDARD.encode(combined);

    // 2. Sign the payload string
    let mut mac: HmacSha256 = HmacKeyInit::new_from_slice(key)
        .map_err(|_| "Failed to initialize HMAC")?;
    mac.update(payload_b64.as_bytes());
    let signature = general_purpose::STANDARD.encode(mac.finalize().into_bytes());

    Ok(SecureEnvelope {
        preview: preview.to_string(),
        payload: payload_b64,
        signature,
    })
}

pub fn decrypt_payload(key: &[u8; 32], encrypted: &str) -> Result<String, String> {
    // Check if it's already plain-text (graceful degradation)
    if !encrypted.contains('=') && encrypted.len() < 24 {
         // This is a rough heuristic, but basically if it doesn't look like base64-encrypted data
         // (which usually has a nonce + some overhead), return as is.
         // Actually better: try to decode base64 first.
    }

    let decoded = match general_purpose::STANDARD.decode(encrypted) {
        Ok(d) => d,
        Err(_) => return Ok(encrypted.to_string()), // Fallback to plain text
    };

    if decoded.len() < NONCE_SIZE {
        return Ok(encrypted.to_string()); // Fallback
    }

    let (nonce_bytes, ciphertext) = decoded.split_at(NONCE_SIZE);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::from_slice(nonce_bytes);

    match cipher.decrypt(nonce, ciphertext) {
        Ok(decrypted) => String::from_utf8(decrypted).map_err(|_| "Invalid UTF8".into()),
        Err(_) => Ok(encrypted.to_string()), // Fallback
    }
}

pub fn verify_signature(key: &[u8; 32], payload: &str, signature: &str) -> Result<(), String> {
    let sig_bytes = general_purpose::STANDARD
        .decode(signature)
        .map_err(|_| "Invalid signature base64".to_string())?;

    let mut mac: HmacSha256 = HmacKeyInit::new_from_slice(key)
        .map_err(|_| "Failed to initialize HMAC".to_string())?;
    mac.update(payload.as_bytes());
    
    mac.verify_slice(&sig_bytes)
        .map_err(|_| "Signature verification failed".to_string())
}

/// Simplified encryption for database fields (nonce + ciphertext)
pub fn encrypt_db_field(key: &[u8; 32], plaintext: &str) -> String {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    if let Ok(ciphertext) = cipher.encrypt(nonce, plaintext.as_bytes()) {
        let mut combined = nonce_bytes.to_vec();
        combined.extend(ciphertext);
        general_purpose::STANDARD.encode(combined)
    } else {
        plaintext.to_string() // Fallback
    }
}

/// Simplified decryption for database fields
pub fn decrypt_db_field(key: &[u8; 32], encrypted: &str) -> String {
    decrypt_payload(key, encrypted).unwrap_or_else(|_| encrypted.to_string())
}