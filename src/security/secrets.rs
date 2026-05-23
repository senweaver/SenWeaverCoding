// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Context, Result};
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;

const KEY_LEN: usize = 32;

const NONCE_LEN: usize = 12;

static DISABLED_ENCRYPTION_WARN_ONCE: Once = Once::new();

#[derive(Debug, Clone)]
pub struct SecretStore {

    key_path: PathBuf,

    enabled: bool,
}

impl SecretStore {

    pub fn mask_secret(secret: &str) -> String {
        let len = secret.len();
        if len <= 8 {
            return "*".repeat(len);
        }
        let prefix = &secret[..4];
        let suffix = &secret[len - 4..];
        format!("{prefix}...{suffix}")
    }

    pub fn new(sen_dir: &Path, enabled: bool) -> Self {
        Self {
            key_path: sen_dir.join(".secret_key"),
            enabled,
        }
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        if plaintext.is_empty() {
            return Ok(plaintext.to_string());
        }
        if !self.enabled {
            DISABLED_ENCRYPTION_WARN_ONCE.call_once(|| {
                tracing::warn!(
                    "secrets.encrypt is disabled in config; storing secrets as plaintext. \
                     Set [secrets].encrypt = true to enable ChaCha20-Poly1305 encryption."
                );
            });
            return Ok(plaintext.to_string());
        }

        let key_bytes = self.load_or_create_key()?;
        let key = Key::from_slice(&key_bytes);
        let cipher = ChaCha20Poly1305::new(key);

        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|e| anyhow::anyhow!("Encryption failed: {e}"))?;

        let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ciphertext);

        Ok(format!("enc2:{}", hex_encode(&blob)))
    }

    pub fn decrypt(&self, value: &str) -> Result<String> {
        if let Some(hex_str) = value.strip_prefix("enc2:") {
            self.decrypt_chacha20(hex_str)
        } else if let Some(hex_str) = value.strip_prefix("enc:") {
            self.decrypt_legacy_xor(hex_str)
        } else {
            Ok(value.to_string())
        }
    }

    pub fn decrypt_and_migrate(&self, value: &str) -> Result<(String, Option<String>)> {
        if let Some(hex_str) = value.strip_prefix("enc2:") {

            let plaintext = self.decrypt_chacha20(hex_str)?;
            Ok((plaintext, None))
        } else if let Some(hex_str) = value.strip_prefix("enc:") {

            tracing::warn!(
                "Decrypting legacy XOR-encrypted secret (enc: prefix). \
                 This format is insecure and will be removed in a future release. \
                 The secret will be automatically migrated to enc2: (ChaCha20-Poly1305)."
            );
            let plaintext = self.decrypt_legacy_xor(hex_str)?;
            let migrated = self.encrypt(&plaintext)?;
            Ok((plaintext, Some(migrated)))
        } else {

            Ok((value.to_string(), None))
        }
    }

    pub fn needs_migration(value: &str) -> bool {
        value.starts_with("enc:")
    }

    fn decrypt_chacha20(&self, hex_str: &str) -> Result<String> {
        let blob =
            hex_decode(hex_str).context("Failed to decode encrypted secret (corrupt hex)")?;
        anyhow::ensure!(
            blob.len() > NONCE_LEN,
            "Encrypted value too short (missing nonce)"
        );

        let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
        let nonce = Nonce::from_slice(nonce_bytes);
        let key_bytes = self.load_or_create_key()?;
        let key = Key::from_slice(&key_bytes);
        let cipher = ChaCha20Poly1305::new(key);

        let plaintext_bytes = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| anyhow::anyhow!("Decryption failed — wrong key or tampered data"))?;

        String::from_utf8(plaintext_bytes)
            .context("Decrypted secret is not valid UTF-8 — corrupt data")
    }

    fn decrypt_legacy_xor(&self, hex_str: &str) -> Result<String> {
        let ciphertext = hex_decode(hex_str)
            .context("Failed to decode legacy encrypted secret (corrupt hex)")?;
        let master_key = self.load_or_create_key()?;

        use hmac::Mac;
        type HmacSha256 = hmac::Hmac<sha2::Sha256>;

        let mut mac = <HmacSha256 as Mac>::new_from_slice(&master_key)
            .map_err(|e| anyhow::anyhow!("HMAC key error: {e}"))?;
        mac.update(b"sen-legacy-xor-key-derivation");
        let derived_key = mac.finalize().into_bytes().to_vec();

        let plaintext_bytes = xor_cipher(&ciphertext, &derived_key);
        String::from_utf8(plaintext_bytes)
            .context("Decrypted legacy secret is not valid UTF-8 — wrong key or corrupt data")
    }

    pub fn is_encrypted(value: &str) -> bool {
        value.starts_with("enc2:") || value.starts_with("enc:")
    }

    pub fn is_secure_encrypted(value: &str) -> bool {
        value.starts_with("enc2:")
    }

    fn load_or_create_key(&self) -> Result<Vec<u8>> {
        if self.key_path.exists() {
            let hex_key =
                fs::read_to_string(&self.key_path).context("Failed to read secret key file")?;
            let key_bytes = hex_decode(hex_key.trim()).context("Secret key file is corrupt")?;
            if key_bytes.len() != 32 {
                anyhow::bail!(
                    "Secret key file is corrupt: expected 32 bytes, got {}",
                    key_bytes.len()
                );
            }
            Ok(key_bytes)
        } else {
            let key = generate_random_key();
            if let Some(parent) = self.key_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&self.key_path, hex_encode(&key))
                .context("Failed to write secret key file")?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&self.key_path, fs::Permissions::from_mode(0o600))
                    .context("Failed to set key file permissions")?;
            }
            #[cfg(windows)]
            {

                let username = crate::util::hidden_sync_command("whoami")
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_else(|| std::env::var("USERNAME").unwrap_or_default());
                let Some(grant_arg) = build_windows_icacls_grant_arg(&username) else {
                    tracing::warn!(
                        "USERNAME environment variable is empty; \
                         cannot restrict key file permissions via icacls"
                    );
                    return Ok(key);
                };

                match crate::util::hidden_sync_command("takeown")
                    .arg("/F")
                    .arg(&self.key_path)
                    .output()
                {
                    Ok(o) if !o.status.success() => {
                        tracing::warn!(
                            "Failed to take ownership of key file via takeown (exit code {:?})",
                            o.status.code()
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Could not take ownership of key file: {e}");
                    }
                    _ => {
                        tracing::debug!("Key file ownership set to current user via takeown");
                    }
                }

                match crate::util::hidden_sync_command("icacls")
                    .arg(&self.key_path)
                    .args(["/inheritance:r", "/grant:r"])
                    .arg(grant_arg)
                    .output()
                {
                    Ok(o) if !o.status.success() => {
                        tracing::warn!(
                            "Failed to set key file permissions via icacls (exit code {:?})",
                            o.status.code()
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Could not set key file permissions: {e}");
                    }
                    _ => {
                        tracing::debug!("Key file permissions restricted via icacls");
                    }
                }
            }

            Ok(key)
        }
    }
}

fn xor_cipher(data: &[u8], key: &[u8]) -> Vec<u8> {
    if key.is_empty() {
        return data.to_vec();
    }
    data.iter()
        .enumerate()
        .map(|(i, &b)| b ^ key[i % key.len()])
        .collect()
}

fn generate_random_key() -> Vec<u8> {
    ChaCha20Poly1305::generate_key(&mut OsRng).to_vec()
}

fn hex_encode(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn build_windows_icacls_grant_arg(username: &str) -> Option<String> {
    let normalized = username.trim();
    if normalized.is_empty() {
        return None;
    }
    Some(format!("{normalized}:F"))
}

#[allow(clippy::manual_is_multiple_of)]
fn hex_decode(hex: &str) -> Result<Vec<u8>> {
    if (hex.len() & 1) != 0 {
        anyhow::bail!("Hex string has odd length");
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|e| anyhow::anyhow!("Invalid hex at position {i}: {e}"))
        })
        .collect()
}
