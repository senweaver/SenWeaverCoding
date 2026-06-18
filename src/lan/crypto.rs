// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use base64::Engine as _;
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::{
    aead::{Aead, AeadInPlace, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce, Tag,
};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

const SESSION_KEY_CONTEXT: &[u8] = b"senweaver-lan-session-v1";

pub struct StaticKeypair {
    secret: StaticSecret,
    public: PublicKey,
}

impl StaticKeypair {
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let secret = StaticSecret::from(*seed);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    pub fn public_bytes(&self) -> [u8; 32] {
        *self.public.as_bytes()
    }

    pub fn public_b64(&self) -> String {
        b64().encode(self.public.as_bytes())
    }

    pub fn session_key(&self, peer_public: &PublicKey) -> [u8; 32] {
        let shared = self.secret.diffie_hellman(peer_public);
        let mut hasher = Sha256::new();
        hasher.update(SESSION_KEY_CONTEXT);
        hasher.update(shared.as_bytes());
        let mut both = [self.public.as_bytes().to_vec(), peer_public.as_bytes().to_vec()];
        both.sort();
        hasher.update(&both[0]);
        hasher.update(&both[1]);
        let digest = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&digest);
        key
    }
}

pub fn public_from_b64(value: &str) -> Option<PublicKey> {
    let bytes = b64().decode(value.trim()).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Some(PublicKey::from(arr))
}

pub fn random_seed() -> [u8; 32] {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    seed
}

pub fn seal(key: &[u8; 32], plaintext: &[u8]) -> Option<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plaintext).ok()?;
    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Some(out)
}

pub fn open(key: &[u8; 32], frame: &[u8]) -> Option<Vec<u8>> {
    if frame.len() < 12 {
        return None;
    }
    let (nonce_bytes, ciphertext) = frame.split_at(12);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher.decrypt(nonce, ciphertext).ok()
}

pub struct SessionCipher {
    cipher: ChaCha20Poly1305,
}

impl SessionCipher {
    pub fn new(key: &[u8; 32]) -> Self {
        Self {
            cipher: ChaCha20Poly1305::new(Key::from_slice(key)),
        }
    }

    pub fn seal_with_len_prefix(&self, plaintext: &[u8]) -> Vec<u8> {
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let mut frame = Vec::with_capacity(4 + 12 + plaintext.len() + 16);
        frame.extend_from_slice(&[0u8; 4]);
        frame.extend_from_slice(&nonce_bytes);
        let body_start = frame.len();
        frame.extend_from_slice(plaintext);
        let tag = self
            .cipher
            .encrypt_in_place_detached(nonce, b"", &mut frame[body_start..])
            .expect("chacha20poly1305 encryption never fails");
        frame.extend_from_slice(tag.as_slice());
        let body_len = (frame.len() - 4) as u32;
        frame[0..4].copy_from_slice(&body_len.to_be_bytes());
        frame
    }

    pub fn open(&self, body: &[u8]) -> Option<Vec<u8>> {
        if body.len() < 12 + 16 {
            return None;
        }
        let (nonce_bytes, rest) = body.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        let tag_start = rest.len() - 16;
        let tag = Tag::clone_from_slice(&rest[tag_start..]);
        let mut buf = rest[..tag_start].to_vec();
        self.cipher
            .decrypt_in_place_detached(nonce, b"", &mut buf, &tag)
            .ok()?;
        Some(buf)
    }
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}
