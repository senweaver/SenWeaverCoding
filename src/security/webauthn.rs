// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::security::SecretStore;
use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::rand::SecureRandom;
use ring::signature;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const COSE_ALG_ES256: i64 = -7;

const CHALLENGE_LEN: usize = 32;

const MAX_CREDENTIAL_ID_LEN: usize = 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnConfig {

    pub enabled: bool,

    pub rp_id: String,

    pub rp_origin: String,

    pub rp_name: String,
}

impl Default for WebAuthnConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rp_id: "localhost".into(),
            rp_origin: "http://localhost:42617".into(),
            rp_name: "SenWeaverCoding".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnCredential {

    pub credential_id: String,

    pub public_key: String,

    pub sign_count: u32,

    pub label: String,

    pub registered_at: String,

    pub algorithm: i64,

    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationState {

    pub challenge: String,

    pub user_id: String,

    pub user_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationState {

    pub challenge: String,

    pub user_id: String,

    pub allowed_credentials: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreationChallengeResponse {

    pub challenge: String,

    pub rp: RelyingParty,

    pub user: PublicKeyUser,

    pub pub_key_cred_params: Vec<PubKeyCredParam>,

    pub timeout: u64,

    pub attestation: String,

    pub exclude_credentials: Vec<CredentialDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestChallengeResponse {

    pub challenge: String,

    pub rp_id: String,

    pub allow_credentials: Vec<CredentialDescriptor>,

    pub timeout: u64,

    pub user_verification: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelyingParty {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKeyUser {

    pub id: String,
    pub name: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PubKeyCredParam {
    #[serde(rename = "type")]
    pub type_: String,
    pub alg: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialDescriptor {
    #[serde(rename = "type")]
    pub type_: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterCredentialResponse {

    pub id: String,

    pub attestation_object: String,

    pub client_data_json: String,

    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticateCredentialResponse {

    pub id: String,

    pub authenticator_data: String,

    pub client_data_json: String,

    pub signature: String,
}

pub struct WebAuthnManager {
    config: WebAuthnConfig,
    secret_store: Arc<SecretStore>,
    credentials_path: PathBuf,
    rng: ring::rand::SystemRandom,
}

impl WebAuthnManager {

    pub fn new(config: WebAuthnConfig, secret_store: Arc<SecretStore>, storage_dir: &Path) -> Self {
        Self {
            config,
            secret_store,
            credentials_path: storage_dir.join("webauthn_credentials.json"),
            rng: ring::rand::SystemRandom::new(),
        }
    }

    pub fn start_registration(
        &self,
        user_id: &str,
        user_name: &str,
    ) -> Result<(CreationChallengeResponse, RegistrationState)> {
        let challenge = self.generate_challenge()?;

        let existing = self.load_credentials_for_user(user_id)?;
        let exclude: Vec<CredentialDescriptor> = existing
            .iter()
            .map(|c| CredentialDescriptor {
                type_: "public-key".into(),
                id: c.credential_id.clone(),
            })
            .collect();

        let user_id_b64 = URL_SAFE_NO_PAD.encode(user_id.as_bytes());

        let creation = CreationChallengeResponse {
            challenge: challenge.clone(),
            rp: RelyingParty {
                id: self.config.rp_id.clone(),
                name: self.config.rp_name.clone(),
            },
            user: PublicKeyUser {
                id: user_id_b64,
                name: user_name.into(),
                display_name: user_name.into(),
            },
            pub_key_cred_params: vec![PubKeyCredParam {
                type_: "public-key".into(),
                alg: COSE_ALG_ES256,
            }],
            timeout: 60_000,
            attestation: "none".into(),
            exclude_credentials: exclude,
        };

        let state = RegistrationState {
            challenge,
            user_id: user_id.into(),
            user_name: user_name.into(),
        };

        Ok((creation, state))
    }

    pub fn finish_registration(
        &self,
        reg_state: &RegistrationState,
        response: &RegisterCredentialResponse,
    ) -> Result<WebAuthnCredential> {

        let client_data_bytes = URL_SAFE_NO_PAD
            .decode(&response.client_data_json)
            .context("Invalid base64url in client_data_json")?;
        let client_data: serde_json::Value =
            serde_json::from_slice(&client_data_bytes).context("Invalid client data JSON")?;

        let cd_type = client_data["type"].as_str().unwrap_or_default();
        anyhow::ensure!(
            cd_type == "webauthn.create",
            "Expected type 'webauthn.create', got '{cd_type}'"
        );

        let cd_challenge = client_data["challenge"].as_str().unwrap_or_default();
        anyhow::ensure!(
            cd_challenge == reg_state.challenge,
            "Challenge mismatch in registration response"
        );

        let cd_origin = client_data["origin"].as_str().unwrap_or_default();
        anyhow::ensure!(
            cd_origin == self.config.rp_origin,
            "Origin mismatch: expected '{}', got '{cd_origin}'",
            self.config.rp_origin
        );

        let attestation_bytes = URL_SAFE_NO_PAD
            .decode(&response.attestation_object)
            .context("Invalid base64url in attestation_object")?;

        let (public_key_bytes, sign_count) =
            extract_public_key_from_attestation(&attestation_bytes)?;

        let cred_id_bytes = URL_SAFE_NO_PAD
            .decode(&response.id)
            .context("Invalid base64url in credential ID")?;
        anyhow::ensure!(
            cred_id_bytes.len() <= MAX_CREDENTIAL_ID_LEN,
            "Credential ID too long ({} bytes, max {MAX_CREDENTIAL_ID_LEN})",
            cred_id_bytes.len()
        );

        let now = chrono::Utc::now().to_rfc3339();
        let label = response
            .label
            .clone()
            .unwrap_or_else(|| "Hardware Key".into());

        let credential = WebAuthnCredential {
            credential_id: response.id.clone(),
            public_key: URL_SAFE_NO_PAD.encode(&public_key_bytes),
            sign_count,
            label,
            registered_at: now,
            algorithm: COSE_ALG_ES256,
            user_id: reg_state.user_id.clone(),
        };

        self.store_credential(&credential)?;

        Ok(credential)
    }

    pub fn start_authentication(
        &self,
        user_id: &str,
    ) -> Result<(RequestChallengeResponse, AuthenticationState)> {
        let credentials = self.load_credentials_for_user(user_id)?;
        anyhow::ensure!(
            !credentials.is_empty(),
            "No registered credentials for user '{user_id}'"
        );

        let challenge = self.generate_challenge()?;

        let allow: Vec<CredentialDescriptor> = credentials
            .iter()
            .map(|c| CredentialDescriptor {
                type_: "public-key".into(),
                id: c.credential_id.clone(),
            })
            .collect();

        let allowed_ids: Vec<String> = credentials
            .iter()
            .map(|c| c.credential_id.clone())
            .collect();

        let request = RequestChallengeResponse {
            challenge: challenge.clone(),
            rp_id: self.config.rp_id.clone(),
            allow_credentials: allow,
            timeout: 60_000,
            user_verification: "preferred".into(),
        };

        let state = AuthenticationState {
            challenge,
            user_id: user_id.into(),
            allowed_credentials: allowed_ids,
        };

        Ok((request, state))
    }

    pub fn finish_authentication(
        &self,
        auth_state: &AuthenticationState,
        response: &AuthenticateCredentialResponse,
    ) -> Result<()> {

        anyhow::ensure!(
            auth_state.allowed_credentials.contains(&response.id),
            "Credential ID not in allowed list"
        );

        let mut all_credentials = self.load_all_credentials()?;
        let credential = all_credentials
            .values()
            .flatten()
            .find(|c| c.credential_id == response.id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Credential not found: {}", response.id))?;

        let client_data_bytes = URL_SAFE_NO_PAD
            .decode(&response.client_data_json)
            .context("Invalid base64url in client_data_json")?;
        let client_data: serde_json::Value =
            serde_json::from_slice(&client_data_bytes).context("Invalid client data JSON")?;

        let cd_type = client_data["type"].as_str().unwrap_or_default();
        anyhow::ensure!(
            cd_type == "webauthn.get",
            "Expected type 'webauthn.get', got '{cd_type}'"
        );

        let cd_challenge = client_data["challenge"].as_str().unwrap_or_default();
        anyhow::ensure!(
            cd_challenge == auth_state.challenge,
            "Challenge mismatch in authentication response"
        );

        let cd_origin = client_data["origin"].as_str().unwrap_or_default();
        anyhow::ensure!(
            cd_origin == self.config.rp_origin,
            "Origin mismatch: expected '{}', got '{cd_origin}'",
            self.config.rp_origin
        );

        let auth_data_bytes = URL_SAFE_NO_PAD
            .decode(&response.authenticator_data)
            .context("Invalid base64url in authenticator_data")?;

        let client_data_hash = ring::digest::digest(&ring::digest::SHA256, &client_data_bytes);
        let mut signed_data = auth_data_bytes.clone();
        signed_data.extend_from_slice(client_data_hash.as_ref());

        let public_key_bytes = URL_SAFE_NO_PAD
            .decode(&credential.public_key)
            .context("Invalid base64url in stored public key")?;

        let sig_bytes = URL_SAFE_NO_PAD
            .decode(&response.signature)
            .context("Invalid base64url in signature")?;

        verify_es256_signature(&public_key_bytes, &signed_data, &sig_bytes)?;

        if auth_data_bytes.len() >= 37 {
            let new_count = u32::from_be_bytes([
                auth_data_bytes[33],
                auth_data_bytes[34],
                auth_data_bytes[35],
                auth_data_bytes[36],
            ]);
            if new_count > 0 || credential.sign_count > 0 {
                anyhow::ensure!(
                    new_count > credential.sign_count,
                    "Sign counter did not increase ({new_count} <= {}). Possible cloned authenticator.",
                    credential.sign_count
                );
            }

            if let Some(user_creds) = all_credentials.get_mut(&credential.user_id) {
                if let Some(cred) = user_creds
                    .iter_mut()
                    .find(|c| c.credential_id == response.id)
                {
                    cred.sign_count = new_count;
                }
            }
            self.save_all_credentials(&all_credentials)?;
        }

        Ok(())
    }

    pub fn list_credentials(&self, user_id: &str) -> Result<Vec<WebAuthnCredential>> {
        self.load_credentials_for_user(user_id)
    }

    pub fn remove_credential(&self, user_id: &str, credential_id: &str) -> Result<()> {
        let mut all = self.load_all_credentials()?;
        if let Some(user_creds) = all.get_mut(user_id) {
            let before = user_creds.len();
            user_creds.retain(|c| c.credential_id != credential_id);
            anyhow::ensure!(
                user_creds.len() < before,
                "Credential '{credential_id}' not found for user '{user_id}'"
            );
        } else {
            anyhow::bail!("No credentials found for user '{user_id}'");
        }
        self.save_all_credentials(&all)
    }

    fn generate_challenge(&self) -> Result<String> {
        let mut buf = [0u8; CHALLENGE_LEN];
        self.rng
            .fill(&mut buf)
            .map_err(|_| anyhow::anyhow!("Failed to generate random challenge"))?;
        Ok(URL_SAFE_NO_PAD.encode(buf))
    }

    fn load_credentials_for_user(&self, user_id: &str) -> Result<Vec<WebAuthnCredential>> {
        let all = self.load_all_credentials()?;
        Ok(all.get(user_id).cloned().unwrap_or_default())
    }

    fn store_credential(&self, credential: &WebAuthnCredential) -> Result<()> {
        let mut all = self.load_all_credentials()?;
        all.entry(credential.user_id.clone())
            .or_default()
            .push(credential.clone());
        self.save_all_credentials(&all)
    }

    fn load_all_credentials(&self) -> Result<HashMap<String, Vec<WebAuthnCredential>>> {
        if !self.credentials_path.exists() {
            return Ok(HashMap::new());
        }

        let encrypted = std::fs::read_to_string(&self.credentials_path)
            .context("Failed to read WebAuthn credentials file")?;

        if encrypted.is_empty() {
            return Ok(HashMap::new());
        }

        let json = self
            .secret_store
            .decrypt(&encrypted)
            .context("Failed to decrypt WebAuthn credentials")?;

        serde_json::from_str(&json).context("Failed to parse WebAuthn credentials JSON")
    }

    fn save_all_credentials(
        &self,
        credentials: &HashMap<String, Vec<WebAuthnCredential>>,
    ) -> Result<()> {
        let json = serde_json::to_string(credentials).context("Failed to serialize credentials")?;
        let encrypted = self
            .secret_store
            .encrypt(&json)
            .context("Failed to encrypt WebAuthn credentials")?;

        if let Some(parent) = self.credentials_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.credentials_path, &encrypted)
            .context("Failed to write WebAuthn credentials file")?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                &self.credentials_path,
                std::fs::Permissions::from_mode(0o600),
            )
            .context("Failed to set credentials file permissions")?;
        }

        Ok(())
    }
}

fn extract_public_key_from_attestation(attestation_bytes: &[u8]) -> Result<(Vec<u8>, u32)> {

    if let Ok(att) = serde_json::from_slice::<AttestationObject>(attestation_bytes) {
        let pk = URL_SAFE_NO_PAD
            .decode(&att.public_key)
            .context("Invalid base64url in attestation public key")?;
        return Ok((pk, att.sign_count.unwrap_or(0)));
    }

    if attestation_bytes.len() >= 37 {
        let sign_count = u32::from_be_bytes([
            attestation_bytes[33],
            attestation_bytes[34],
            attestation_bytes[35],
            attestation_bytes[36],
        ]);

        let flags = attestation_bytes[32];
        if flags & 0x40 != 0 && attestation_bytes.len() > 55 {

            let cred_id_len =
                u16::from_be_bytes([attestation_bytes[53], attestation_bytes[54]]) as usize;
            let cose_key_start = 55 + cred_id_len;
            if attestation_bytes.len() > cose_key_start {
                let cose_key = &attestation_bytes[cose_key_start..];
                let pk = extract_p256_from_cose(cose_key)?;
                return Ok((pk, sign_count));
            }
        }
    }

    anyhow::bail!(
        "Unable to extract public key from attestation object ({} bytes)",
        attestation_bytes.len()
    )
}

#[derive(Deserialize)]
struct AttestationObject {

    public_key: String,

    sign_count: Option<u32>,
}

fn extract_p256_from_cose(cose: &[u8]) -> Result<Vec<u8>> {

    if cose.len() >= 65 && cose[0] == 0x04 {
        return Ok(cose[..65].to_vec());
    }

    anyhow::bail!(
        "Unsupported COSE key format (expected uncompressed P-256, got {} bytes starting with 0x{:02x})",
        cose.len(),
        cose.first().copied().unwrap_or(0)
    )
}

fn verify_es256_signature(public_key: &[u8], message: &[u8], sig: &[u8]) -> Result<()> {

    let pk = signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, public_key);

    pk.verify(message, sig)
        .map_err(|_| anyhow::anyhow!("WebAuthn signature verification failed"))
}

fn encode_p256_spki(uncompressed_point: &[u8]) -> Vec<u8> {

    let mut spki = vec![
        0x30, 0x59,
        0x30, 0x13,
        0x06, 0x07,
        0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01,
        0x06, 0x08,
        0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07,
        0x03, 0x42,
        0x00,
    ];
    spki.extend_from_slice(uncompressed_point);
    spki
}
