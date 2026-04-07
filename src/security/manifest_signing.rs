// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Manifest signing and verification for tool/skill manifests.
//!
//! Provides Ed25519 signing and verification of TOML manifests using the `ring`
//! crate. This ensures manifests haven't been tampered with and come from
//! trusted sources.

use ring::digest::{SHA256, digest};
use ring::rand::SystemRandom;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use serde::{Deserialize, Serialize};

/// A signed manifest with content hash and cryptographic signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedManifest {
    /// The original manifest content (typically TOML).
    pub manifest: String,
    /// SHA-256 hex digest of the manifest content.
    pub content_hash: String,
    /// Ed25519 signature bytes (hex-encoded).
    pub signature: String,
    /// The signer's public key (hex-encoded, 32 bytes).
    pub signer_public_key: String,
    /// Human-readable signer identifier (e.g., "sen-official").
    pub signer_id: String,
}

/// Compute SHA-256 hex digest of content.
pub fn hash_manifest(content: &str) -> String {
    let d = digest(&SHA256, content.as_bytes());
    hex::encode(d.as_ref())
}

/// Errors that can occur during signing or verification.
#[derive(Debug, thiserror::Error)]
pub enum ManifestSignError {
    #[error("failed to generate signing key: {0}")]
    KeyGeneration(String),
    #[error("failed to sign manifest: {0}")]
    SigningFailed(String),
    #[error("signature verification failed")]
    VerificationFailed,
    #[error("content hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("invalid public key format")]
    InvalidPublicKey,
    #[error("invalid signature format")]
    InvalidSignature,
}

/// A signing key pair for manifest signing.
pub struct ManifestSigner {
    key_pair: Ed25519KeyPair,
    signer_id: String,
}

impl ManifestSigner {
    /// Generate a new random signing key pair.
    pub fn generate(signer_id: impl Into<String>) -> Result<Self, ManifestSignError> {
        let rng = SystemRandom::new();
        let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&rng)
            .map_err(|e| ManifestSignError::KeyGeneration(e.to_string()))?;
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8_bytes.as_ref())
            .map_err(|e| ManifestSignError::KeyGeneration(e.to_string()))?;
        Ok(Self {
            key_pair,
            signer_id: signer_id.into(),
        })
    }

    /// Create a signer from existing PKCS#8 DER bytes.
    pub fn from_pkcs8(
        pkcs8_der: &[u8],
        signer_id: impl Into<String>,
    ) -> Result<Self, ManifestSignError> {
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8_der)
            .map_err(|e| ManifestSignError::KeyGeneration(e.to_string()))?;
        Ok(Self {
            key_pair,
            signer_id: signer_id.into(),
        })
    }

    /// Get the public key bytes (32 bytes).
    pub fn public_key_bytes(&self) -> &[u8] {
        self.key_pair.public_key().as_ref()
    }

    /// Get the public key as hex string.
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key_bytes())
    }

    /// Sign a manifest, producing a `SignedManifest`.
    pub fn sign(&self, manifest: &str) -> SignedManifest {
        let content_hash = hash_manifest(manifest);
        let sig = self.key_pair.sign(content_hash.as_bytes());

        SignedManifest {
            manifest: manifest.to_string(),
            content_hash,
            signature: hex::encode(sig.as_ref()),
            signer_public_key: self.public_key_hex(),
            signer_id: self.signer_id.clone(),
        }
    }
}

impl SignedManifest {
    /// Verify the manifest's signature and content hash integrity.
    ///
    /// Returns `Ok(())` if both the content hash matches and the Ed25519
    /// signature is valid for the stated public key.
    pub fn verify(&self) -> Result<(), ManifestSignError> {
        let computed_hash = hash_manifest(&self.manifest);
        if computed_hash != self.content_hash {
            return Err(ManifestSignError::HashMismatch {
                expected: self.content_hash.clone(),
                actual: computed_hash,
            });
        }

        let pub_key_bytes = hex::decode(&self.signer_public_key)
            .map_err(|_| ManifestSignError::InvalidPublicKey)?;
        let sig_bytes =
            hex::decode(&self.signature).map_err(|_| ManifestSignError::InvalidSignature)?;

        let public_key = UnparsedPublicKey::new(&ED25519, &pub_key_bytes);
        public_key
            .verify(self.content_hash.as_bytes(), &sig_bytes)
            .map_err(|_| ManifestSignError::VerificationFailed)?;

        Ok(())
    }

    /// Verify against a specific trusted public key (hex-encoded).
    ///
    /// This is stricter than `verify()` because it also checks the signer
    /// identity matches a known trusted key.
    pub fn verify_with_key(&self, trusted_public_key_hex: &str) -> Result<(), ManifestSignError> {
        if self.signer_public_key != trusted_public_key_hex {
            return Err(ManifestSignError::VerificationFailed);
        }
        self.verify()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_roundtrip() {
        let signer = ManifestSigner::generate("test-signer").unwrap();
        let manifest = "[tool]\nname = \"web_search\"\nversion = \"1.0\"\n";

        let signed = signer.sign(manifest);
        assert_eq!(signed.signer_id, "test-signer");
        assert_eq!(signed.manifest, manifest);

        assert!(signed.verify().is_ok());
    }

    #[test]
    fn tampered_content_detected() {
        let signer = ManifestSigner::generate("test-signer").unwrap();
        let signed = signer.sign("original content");

        let mut tampered = signed.clone();
        tampered.manifest = "modified content".to_string();

        assert!(tampered.verify().is_err());
    }

    #[test]
    fn wrong_key_detected() {
        let signer1 = ManifestSigner::generate("signer-1").unwrap();
        let signer2 = ManifestSigner::generate("signer-2").unwrap();

        let signed = signer1.sign("test manifest");

        let result = signed.verify_with_key(&signer2.public_key_hex());
        assert!(result.is_err());
    }

    #[test]
    fn hash_manifest_deterministic() {
        let h1 = hash_manifest("hello world");
        let h2 = hash_manifest("hello world");
        assert_eq!(h1, h2);
        assert_ne!(h1, hash_manifest("hello world!"));
    }
}
