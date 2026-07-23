use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::{Aead, KeyInit};
use crate::errors::{Result, XhjobError};

/// Get the encryption key from env var XHJOB_ENCRYPTION_KEY (hex-encoded
/// 32-byte / 64-char key). Returns None if not set (plaintext mode).
pub fn encryption_key() -> Option<[u8; 32]> {
    let hex = std::env::var("XHJOB_ENCRYPTION_KEY").ok()?;
    if hex.is_empty() { return None; }
    if hex.len() != 64 { return None; }
    let mut key = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let byte = u8::from_str_radix(
            std::str::from_utf8(chunk).ok()?,
            16,
        ).ok()?;
        key[i] = byte;
    }
    Some(key)
}

/// Encrypt a plaintext string. Returns a base64-encoded ciphertext with
/// a random 12-byte nonce prepended. Format: base64(nonce || ciphertext).
pub fn encrypt(plaintext: &str) -> Result<String> {
    let key_bytes = encryption_key()
        .ok_or_else(|| XhjobError::store("encryption key not set".to_string()))?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    // P0-22 fix: generate the 12-byte nonce from the OS CSPRNG (OsRng)
    // instead of timestamp+counter. For AES-GCM, nonce reuse under the
    // same key is catastrophic (leaks the plaintext via XOR and lets an
    // attacker forge messages). The previous timestamp+counter scheme was
    // unique within a single process, but if multiple daemons share the
    // same XHJOB_ENCRYPTION_KEY and start near-simultaneously (counter
    // resets to 0), the nonces can collide. OsRng pulls from
    // /dev/urandom (Linux) / getentropy (macOS) / BCryptGenRandom
    // (Windows), giving a 96-bit nonce space with negligible collision
    // probability even across processes/machines.
    let nonce_bytes = {
        use rand::rngs::OsRng;
        use rand::RngCore;
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        nonce
    };
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| XhjobError::store(format!("encrypt: {}", e)))?;
    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(base64_encode(&combined))
}

/// Decrypt a base64-encoded ciphertext (nonce || ciphertext).
pub fn decrypt(b64: &str) -> Result<String> {
    let key_bytes = encryption_key()
        .ok_or_else(|| XhjobError::store("encryption key not set".to_string()))?;
    let combined = base64_decode(b64)
        .map_err(|e| XhjobError::store(format!("decrypt base64: {}", e)))?;
    if combined.len() < 12 {
        return Err(XhjobError::store("decrypt: ciphertext too short".to_string()));
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher.decrypt(nonce, ciphertext)
        .map_err(|e| XhjobError::store(format!("decrypt: {}", e)))?;
    String::from_utf8(plaintext)
        .map_err(|e| XhjobError::store(format!("decrypt utf8: {}", e)))
}

fn base64_encode(data: &[u8]) -> String {
    // Simple base64 encoding without external crate
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((n >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((n >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

fn base64_decode(s: &str) -> std::result::Result<Vec<u8>, String> {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let s: Vec<u8> = s.bytes().filter(|&b| b != b'\n' && b != b'\r' && b != b' ').collect();
    let s: Vec<u8> = s.iter().filter(|&&b| b != b'=').cloned().collect();
    let mut result = Vec::new();
    for chunk in s.chunks(4) {
        let mut n: u32 = 0;
        let mut valid = 0;
        for &b in chunk {
            if let Some(pos) = CHARS.iter().position(|&c| c == b) {
                n = (n << 6) | (pos as u32);
                valid += 1;
            }
        }
        n <<= (4 - valid) * 6;
        if valid >= 2 { result.push((n >> 16) as u8); }
        if valid >= 3 { result.push((n >> 8) as u8); }
        if valid >= 4 { result.push(n as u8); }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        std::env::set_var("XHJOB_ENCRYPTION_KEY", "a".repeat(64));
        let plaintext = "hello world {\"cmd\":\"echo hi\"}";
        let ct = encrypt(plaintext).unwrap();
        assert_ne!(ct, plaintext);
        let pt = decrypt(&ct).unwrap();
        assert_eq!(pt, plaintext);
        std::env::remove_var("XHJOB_ENCRYPTION_KEY");
    }

    #[test]
    fn test_no_key_returns_none() {
        std::env::remove_var("XHJOB_ENCRYPTION_KEY");
        assert!(encryption_key().is_none());
    }
}
