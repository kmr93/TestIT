use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use sha2::{Digest, Sha256};

pub fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

pub fn encrypt_secret(key: &[u8; 32], plaintext: &str) -> Result<String, anyhow::Error> {
    use aes_gcm::aead::rand_core::RngCore;

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| anyhow::anyhow!("Invalid encryption key length: {}", e))?;

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    // Prepend nonce to ciphertext: [12-byte nonce][ciphertext] -> hex string
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(hex::encode(combined))
}

pub fn decrypt_secret(key: &[u8; 32], encoded: &str) -> Result<String, anyhow::Error> {
    let combined = hex::decode(encoded)?;
    if combined.len() < 12 {
        anyhow::bail!("Ciphertext payload too short to contain nonce");
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| anyhow::anyhow!("Invalid encryption key length: {}", e))?;
    let nonce = Nonce::from_slice(nonce_bytes);

    let decrypted = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

    let plaintext = String::from_utf8(decrypted)?;
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_encryption_roundtrip() {
        let key = [42u8; 32];
        let secret = "super-secret-api-token-12345";
        let encrypted = encrypt_secret(&key, secret).expect("encrypt");
        assert_ne!(secret, encrypted);

        let decrypted = decrypt_secret(&key, &encrypted).expect("decrypt");
        assert_eq!(secret, decrypted);
    }

    #[test]
    fn test_sha256() {
        let hash = compute_sha256(b"hello world");
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }
}
