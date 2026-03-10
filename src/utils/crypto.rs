use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce
};
use rand::RngCore;
use base64::{engine::general_purpose, Engine};

const NONCE_SIZE: usize = 12;

pub fn encrypt_payload(key: &[u8; 32], plaintext: &str) -> Result<String, String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));

    let mut nonce_bytes = [0u8; NONCE_SIZE];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|_| "Encryption failed")?;

    let mut combined = nonce_bytes.to_vec();
    combined.extend(ciphertext);

    Ok(general_purpose::STANDARD.encode(combined))
}

pub fn decrypt_payload(key: &[u8; 32], encrypted: &str) -> Result<String, String> {
    let decoded = general_purpose::STANDARD
        .decode(encrypted)
        .map_err(|_| "Invalid base64")?;

    let (nonce_bytes, ciphertext) = decoded.split_at(NONCE_SIZE);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::from_slice(nonce_bytes);

    let decrypted = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "Decryption failed")?;

    String::from_utf8(decrypted).map_err(|_| "Invalid UTF8".into())
}