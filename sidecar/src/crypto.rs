//! ChaCha20-Poly1305 snapshot envelope helpers.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use zeroize::Zeroizing;

use crate::sessions::{namespace_bytes, StoreError};

pub const MAGIC: &[u8] = b"gaze-hermes-session-v1\n";
pub const NONCE_LEN: usize = 12;
pub const KEY_LEN: usize = 32;

pub fn load_or_create_master_key(
    path: &std::path::Path,
) -> Result<Zeroizing<[u8; KEY_LEN]>, StoreError> {
    match std::fs::read(path) {
        Ok(bytes) => {
            if bytes.len() != KEY_LEN {
                return Err(StoreError::Integrity);
            }
            let mut key = Zeroizing::new([0u8; KEY_LEN]);
            key.copy_from_slice(&bytes);
            Ok(key)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let key = random_master_key();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, key.as_slice())?;
            Ok(key)
        }
        Err(err) => Err(StoreError::Io(err)),
    }
}

pub fn random_master_key() -> Zeroizing<[u8; KEY_LEN]> {
    Zeroizing::new(rand::random::<[u8; KEY_LEN]>())
}

pub fn encrypt_snapshot(
    key: &[u8; KEY_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, StoreError> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce_bytes = rand::random::<[u8; NONCE_LEN]>();
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| StoreError::Encrypt)?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn decrypt_snapshot(
    key: &[u8; KEY_LEN],
    aad: &[u8],
    envelope: &[u8],
) -> Result<Vec<u8>, StoreError> {
    if envelope.len() < MAGIC.len() + NONCE_LEN || &envelope[..MAGIC.len()] != MAGIC {
        return Err(StoreError::Integrity);
    }
    let nonce_bytes = &envelope[MAGIC.len()..MAGIC.len() + NONCE_LEN];
    let ciphertext = &envelope[MAGIC.len() + NONCE_LEN..];
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            Nonce::from_slice(nonce_bytes),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| StoreError::Decrypt)
}

pub fn encrypt_session_payload(
    key: &[u8; KEY_LEN],
    profile_id: &str,
    session_id: &str,
    plaintext: &[u8],
) -> Result<Vec<u8>, StoreError> {
    let aad = namespace_bytes(profile_id, session_id);
    encrypt_snapshot(key, &aad, plaintext)
}

pub fn decrypt_session_payload(
    key: &[u8; KEY_LEN],
    profile_id: &str,
    session_id: &str,
    envelope: &[u8],
) -> Result<Vec<u8>, StoreError> {
    let aad = namespace_bytes(profile_id, session_id);
    decrypt_snapshot(key, &aad, envelope)
}
