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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedManifest {

    pub manifest: String,

    pub content_hash: String,

    pub signature: String,

    pub signer_public_key: String,

    pub signer_id: String,
}

pub fn hash_manifest(content: &str) -> String {
    let d = digest(&SHA256, content.as_bytes());
    hex::encode(d.as_ref())
}

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

pub struct ManifestSigner {
    key_pair: Ed25519KeyPair,
    signer_id: String,
}

impl ManifestSigner {

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

    pub fn public_key_bytes(&self) -> &[u8] {
        self.key_pair.public_key().as_ref()
    }

    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key_bytes())
    }

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

    pub fn verify_with_key(&self, trusted_public_key_hex: &str) -> Result<(), ManifestSignError> {
        if self.signer_public_key != trusted_public_key_hex {
            return Err(ManifestSignError::VerificationFailed);
        }
        self.verify()
    }
}
