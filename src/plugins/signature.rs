// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature::{self, Ed25519KeyPair, KeyPair};

use super::error::PluginError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureMode {

    Strict,

    Permissive,

    Disabled,
}

impl Default for SignatureMode {
    fn default() -> Self {
        Self::Disabled
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationResult {

    Valid { publisher_key: String },

    Unsigned,

    Untrusted,

    Invalid { reason: String },
}

impl VerificationResult {

    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid { .. })
    }
}

fn b64u_encode(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

fn b64u_decode(s: &str) -> Result<Vec<u8>, PluginError> {
    URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|e| PluginError::SignatureInvalid(format!("base64url decode error: {e}")))
}

fn hex_decode(s: &str) -> Result<Vec<u8>, PluginError> {

    let s = s.trim();
    if s.len() % 2 != 0 {
        return Err(PluginError::SignatureInvalid(
            "hex string must have even length".into(),
        ));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|e| PluginError::SignatureInvalid(format!("hex decode: {e}")))
        })
        .collect()
}

fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn canonical_manifest_bytes(manifest_toml: &str) -> Vec<u8> {
    let mut lines: Vec<&str> = Vec::new();
    for line in manifest_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("signature") && trimmed.contains('=') {
            continue;
        }
        if trimmed.starts_with("publisher_key") && trimmed.contains('=') {
            continue;
        }
        lines.push(line);
    }

    while lines.last().map_or(false, |l| l.trim().is_empty()) {
        lines.pop();
    }
    let canonical = lines.join("\n");
    canonical.into_bytes()
}

pub fn sign_manifest(manifest_toml: &str, pkcs8_der: &[u8]) -> Result<String, PluginError> {
    let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8_der)
        .map_err(|e| PluginError::SignatureInvalid(format!("invalid signing key: {e}")))?;
    let canonical = canonical_manifest_bytes(manifest_toml);
    let sig = key_pair.sign(&canonical);
    Ok(b64u_encode(sig.as_ref()))
}

pub fn public_key_hex(pkcs8_der: &[u8]) -> Result<String, PluginError> {
    let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8_der)
        .map_err(|e| PluginError::SignatureInvalid(format!("invalid signing key: {e}")))?;
    Ok(hex_encode(key_pair.public_key().as_ref()))
}

pub fn verify_manifest(
    manifest_toml: &str,
    signature_b64: &str,
    publisher_key_hex: &str,
    trusted_keys: &[String],
) -> VerificationResult {

    let normalized_key = publisher_key_hex.trim().to_lowercase();
    let is_trusted = trusted_keys
        .iter()
        .any(|k| k.trim().to_lowercase() == normalized_key);

    if !is_trusted {
        return VerificationResult::Untrusted;
    }

    let pub_key_bytes = match hex_decode(publisher_key_hex) {
        Ok(bytes) => bytes,
        Err(e) => {
            return VerificationResult::Invalid {
                reason: format!("invalid publisher key: {e}"),
            };
        }
    };

    let sig_bytes = match b64u_decode(signature_b64) {
        Ok(bytes) => bytes,
        Err(e) => {
            return VerificationResult::Invalid {
                reason: format!("invalid signature encoding: {e}"),
            };
        }
    };

    let canonical = canonical_manifest_bytes(manifest_toml);

    let peer_public_key = signature::UnparsedPublicKey::new(&signature::ED25519, &pub_key_bytes);
    match peer_public_key.verify(&canonical, &sig_bytes) {
        Ok(()) => VerificationResult::Valid {
            publisher_key: normalized_key,
        },
        Err(_) => VerificationResult::Invalid {
            reason: "Ed25519 signature verification failed".into(),
        },
    }
}

pub fn enforce_signature_policy(
    plugin_name: &str,
    manifest_toml: &str,
    signature: Option<&str>,
    publisher_key: Option<&str>,
    trusted_keys: &[String],
    mode: SignatureMode,
) -> Result<VerificationResult, PluginError> {
    if mode == SignatureMode::Disabled {
        return Ok(VerificationResult::Unsigned);
    }

    match (signature, publisher_key) {
        (None, _) | (_, None) => {

            match mode {
                SignatureMode::Strict => Err(PluginError::UnsignedPlugin(plugin_name.to_string())),
                SignatureMode::Permissive => {
                    tracing::warn!(
                        plugin = plugin_name,
                        "plugin is unsigned; loading in permissive mode"
                    );
                    Ok(VerificationResult::Unsigned)
                }
                SignatureMode::Disabled => Ok(VerificationResult::Unsigned),
            }
        }
        (Some(sig), Some(pub_key)) => {
            let result = verify_manifest(manifest_toml, sig, pub_key, trusted_keys);
            match &result {
                VerificationResult::Valid { publisher_key } => {
                    tracing::info!(
                        plugin = plugin_name,
                        publisher_key = publisher_key.as_str(),
                        "plugin signature verified"
                    );
                    Ok(result)
                }
                VerificationResult::Untrusted => match mode {
                    SignatureMode::Strict => Err(PluginError::UntrustedPublisher {
                        plugin: plugin_name.to_string(),
                        publisher_key: pub_key.to_string(),
                    }),
                    SignatureMode::Permissive => {
                        tracing::warn!(
                            plugin = plugin_name,
                            publisher_key = pub_key,
                            "plugin publisher key not trusted; loading in permissive mode"
                        );
                        Ok(result)
                    }
                    SignatureMode::Disabled => Ok(result),
                },
                VerificationResult::Invalid { reason } => match mode {
                    SignatureMode::Strict => Err(PluginError::SignatureInvalid(format!(
                        "plugin '{}': {}",
                        plugin_name, reason
                    ))),
                    SignatureMode::Permissive => {
                        tracing::warn!(
                            plugin = plugin_name,
                            reason = reason.as_str(),
                            "plugin signature invalid; loading in permissive mode"
                        );
                        Ok(result)
                    }
                    SignatureMode::Disabled => Ok(result),
                },
                VerificationResult::Unsigned => Ok(result),
            }
        }
    }
}

pub fn generate_signing_key() -> Result<(Vec<u8>, String), PluginError> {
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng)
        .map_err(|e| PluginError::SignatureInvalid(format!("keygen failed: {e}")))?;
    let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
        .map_err(|e| PluginError::SignatureInvalid(format!("parse pkcs8: {e}")))?;
    let pub_hex = hex_encode(key_pair.public_key().as_ref());
    Ok((pkcs8.as_ref().to_vec(), pub_hex))
}
