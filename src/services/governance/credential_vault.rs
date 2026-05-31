// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result};
use base64::Engine as _;
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};
use parking_lot::RwLock;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CredentialKind {
    Username,
    Password,
    Token,
    Url,
    Other,
}

impl CredentialKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CredentialKind::Username => "username",
            CredentialKind::Password => "password",
            CredentialKind::Token => "token",
            CredentialKind::Url => "url",
            CredentialKind::Other => "other",
        }
    }

    pub fn parse(value: &str) -> CredentialKind {
        match value.to_ascii_lowercase().as_str() {
            "username" | "user" => CredentialKind::Username,
            "password" | "pwd" => CredentialKind::Password,
            "token" | "apikey" | "api_key" | "secret" => CredentialKind::Token,
            "url" | "endpoint" => CredentialKind::Url,
            _ => CredentialKind::Other,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CredentialMeta {
    pub name: String,
    pub kind: CredentialKind,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CredentialEntry {
    name: String,
    kind: CredentialKind,
    value: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
struct VaultPayload {
    entries: HashMap<String, CredentialEntry>,
}

#[derive(Serialize, Deserialize)]
struct VaultFile {
    version: u32,
    salt: String,
    nonce: String,
    cipher: String,
}

pub struct CredentialVault {
    data_path: PathBuf,
    salt: Vec<u8>,
    state: RwLock<VaultPayload>,
    placeholder_re: Regex,
    redact_re: Regex,
}

fn current_ts() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn derive_key(salt: &[u8]) -> [u8; 32] {
    let machine_hint = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "senweavercoding-default-host".to_string());
    let mut hasher = Sha256::new();
    hasher.update(b"senweavercoding.credential_vault.v1");
    hasher.update(machine_hint.as_bytes());
    hasher.update(salt);
    let out = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&out);
    key
}

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn restrict_file_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perm = meta.permissions();
            perm.set_mode(0o600);
            let _ = std::fs::set_permissions(path, perm);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

fn preferred_vault_dir(workspace_anchor: &Path) -> PathBuf {
    if let Ok(custom) = std::env::var("SENAGENTOS_VAULT_DIR") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Some(proj) =
        directories::ProjectDirs::from("com", "SenAgentOS", "SenAgentOS")
    {
        return proj.config_dir().to_path_buf();
    }
    workspace_anchor.join(".senagentos")
}

fn migrate_legacy_vault(
    workspace_anchor: &Path,
    target_dir: &Path,
) -> std::io::Result<()> {
    if workspace_anchor == target_dir {
        return Ok(());
    }
    let legacy_data = workspace_anchor.join("credentials.bin");
    let legacy_salt = workspace_anchor.join("credentials.salt");
    if !legacy_data.exists() && !legacy_salt.exists() {
        return Ok(());
    }
    let new_data = target_dir.join("credentials.bin");
    let new_salt = target_dir.join("credentials.salt");
    if new_data.exists() || new_salt.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(target_dir)?;
    if legacy_data.exists() {
        std::fs::rename(&legacy_data, &new_data)
            .or_else(|_| std::fs::copy(&legacy_data, &new_data).map(|_| ()))?;
        restrict_file_permissions(&new_data);
        let _ = std::fs::remove_file(&legacy_data);
    }
    if legacy_salt.exists() {
        std::fs::rename(&legacy_salt, &new_salt)
            .or_else(|_| std::fs::copy(&legacy_salt, &new_salt).map(|_| ()))?;
        restrict_file_permissions(&new_salt);
        let _ = std::fs::remove_file(&legacy_salt);
    }
    Ok(())
}

impl CredentialVault {
    pub fn open(data_dir: &Path) -> Result<Arc<Self>> {
        std::fs::create_dir_all(data_dir).with_context(|| {
            format!(
                "creating credential vault directory {}",
                data_dir.display()
            )
        })?;
        let data_path = data_dir.join("credentials.bin");
        let salt_path = data_dir.join("credentials.salt");

        let salt = if salt_path.exists() {
            std::fs::read(&salt_path).context("reading vault salt")?
        } else {
            let mut salt = vec![0u8; 32];
            OsRng.fill_bytes(&mut salt);
            std::fs::write(&salt_path, &salt).context("writing vault salt")?;
            restrict_file_permissions(&salt_path);
            salt
        };

        let payload = if data_path.exists() {
            let raw = std::fs::read(&data_path).context("reading vault file")?;
            if raw.is_empty() {
                VaultPayload::default()
            } else {
                let file: VaultFile = serde_json::from_slice(&raw).context("parsing vault file")?;
                let key_bytes = derive_key(&salt);
                let cipher = ChaCha20Poly1305::new(Key::from_slice(&key_bytes));
                let nonce_bytes = b64().decode(file.nonce.as_bytes()).context("decoding nonce")?;
                let cipher_bytes = b64()
                    .decode(file.cipher.as_bytes())
                    .context("decoding cipher payload")?;
                if nonce_bytes.len() != 12 {
                    return Err(anyhow::anyhow!("vault nonce malformed"));
                }
                let nonce = Nonce::from_slice(&nonce_bytes);
                let plaintext = cipher
                    .decrypt(nonce, cipher_bytes.as_ref())
                    .map_err(|e| anyhow::anyhow!("vault decrypt failed: {e}"))?;
                serde_json::from_slice::<VaultPayload>(&plaintext)
                    .context("parsing vault payload")?
            }
        } else {
            VaultPayload::default()
        };

        let placeholder_re = Regex::new(r"\$\{cred\.([A-Za-z0-9_-]+)\}")
            .context("compiling credential placeholder regex")?;
        let redact_re = placeholder_re.clone();

        Ok(Arc::new(Self {
            data_path,
            salt,
            state: RwLock::new(payload),
            placeholder_re,
            redact_re,
        }))
    }

    fn write_locked(&self, payload: &VaultPayload) -> Result<()> {
        let plaintext = serde_json::to_vec(payload).context("serializing vault payload")?;
        let key_bytes = derive_key(&self.salt);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key_bytes));
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let cipher_bytes = cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|e| anyhow::anyhow!("vault encrypt failed: {e}"))?;
        let file = VaultFile {
            version: 1,
            salt: b64().encode(&self.salt),
            nonce: b64().encode(nonce_bytes),
            cipher: b64().encode(&cipher_bytes),
        };
        let serialized = serde_json::to_vec_pretty(&file).context("serializing vault file")?;
        if let Some(parent) = self.data_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let tmp = self.data_path.with_extension("bin.tmp");
        std::fs::write(&tmp, &serialized).context("writing vault tmp file")?;
        restrict_file_permissions(&tmp);
        std::fs::rename(&tmp, &self.data_path).context("finalizing vault file")?;
        restrict_file_permissions(&self.data_path);
        Ok(())
    }

    pub fn list(&self) -> Vec<CredentialMeta> {
        let state = self.state.read();
        let mut out: Vec<CredentialMeta> = state
            .entries
            .values()
            .map(|e| CredentialMeta {
                name: e.name.clone(),
                kind: e.kind,
                created_at: e.created_at,
                updated_at: e.updated_at,
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn names(&self) -> Vec<String> {
        let state = self.state.read();
        let mut names: Vec<String> = state.entries.keys().cloned().collect();
        names.sort();
        names
    }

    pub fn get(&self, name: &str) -> Option<String> {
        self.state
            .read()
            .entries
            .get(name)
            .map(|e| e.value.clone())
    }

    pub fn put(&self, name: &str, kind: CredentialKind, value: &str) -> Result<CredentialMeta> {
        validate_name(name)?;
        let mut state = self.state.write();
        let now = current_ts();
        let entry = state
            .entries
            .entry(name.to_string())
            .and_modify(|e| {
                e.kind = kind;
                e.value = value.to_string();
                e.updated_at = now;
            })
            .or_insert_with(|| CredentialEntry {
                name: name.to_string(),
                kind,
                value: value.to_string(),
                created_at: now,
                updated_at: now,
            })
            .clone();
        self.write_locked(&state)?;
        Ok(CredentialMeta {
            name: entry.name,
            kind: entry.kind,
            created_at: entry.created_at,
            updated_at: entry.updated_at,
        })
    }

    pub fn delete(&self, name: &str) -> Result<bool> {
        let mut state = self.state.write();
        let removed = state.entries.remove(name).is_some();
        if removed {
            self.write_locked(&state)?;
        }
        Ok(removed)
    }

    pub fn resolve_placeholders(&self, input: &str) -> String {
        if !input.contains("${cred.") {
            return input.to_string();
        }
        let ephemeral_lookup = current_session_ephemeral_entries();
        let state = self.state.read();
        let re = &self.placeholder_re;
        re.replace_all(input, |caps: &regex::Captures<'_>| {
            let name = &caps[1];
            if let Some(value) = ephemeral_lookup.get(name) {
                return value.clone();
            }
            match state.entries.get(name) {
                Some(e) => e.value.clone(),
                None => caps.get(0).map(|m| m.as_str().to_string()).unwrap_or_default(),
            }
        })
        .into_owned()
    }

    pub fn resolve_json(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => serde_json::Value::String(self.resolve_placeholders(s)),
            serde_json::Value::Array(arr) => serde_json::Value::Array(
                arr.iter().map(|v| self.resolve_json(v)).collect(),
            ),
            serde_json::Value::Object(map) => {
                let mut new_map = serde_json::Map::with_capacity(map.len());
                for (k, v) in map.iter() {
                    new_map.insert(k.clone(), self.resolve_json(v));
                }
                serde_json::Value::Object(new_map)
            }
            other => other.clone(),
        }
    }

    pub fn redact_for_audit(&self, input: &str) -> String {
        if input.is_empty() {
            return String::new();
        }
        let redacted = if input.contains("${cred.") {
            self.redact_re
                .replace_all(input, |caps: &regex::Captures<'_>| {
                    format!("[CRED:{}]", &caps[1])
                })
                .into_owned()
        } else {
            input.to_string()
        };

        let mut patterns: Vec<String> = Vec::new();
        let mut replacements: Vec<String> = Vec::new();
        {
            let state = self.state.read();
            for entry in state.entries.values() {
                if entry.value.is_empty() {
                    continue;
                }
                if matches!(entry.kind, CredentialKind::Username | CredentialKind::Url)
                    && entry.value.len() < 4
                {
                    continue;
                }
                patterns.push(entry.value.clone());
                replacements.push(format!("[CRED:{}]", entry.name));
            }
        }
        for (name, value) in current_session_ephemeral_entries() {
            if value.is_empty() || value.len() < 4 {
                continue;
            }
            patterns.push(value);
            replacements.push(format!("[CRED:{name}]"));
        }

        if patterns.is_empty() {
            return redacted;
        }

        match aho_corasick::AhoCorasick::builder()
            .match_kind(aho_corasick::MatchKind::LeftmostLongest)
            .build(&patterns)
        {
            Ok(ac) => ac.replace_all(&redacted, &replacements),
            Err(_) => {
                let mut out = redacted;
                for (pat, rep) in patterns.iter().zip(replacements.iter()) {
                    if out.contains(pat) {
                        out = out.replace(pat, rep);
                    }
                }
                out
            }
        }
    }

    pub fn redact_args(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => serde_json::Value::String(self.redact_for_audit(s)),
            serde_json::Value::Array(arr) => serde_json::Value::Array(
                arr.iter().map(|v| self.redact_args(v)).collect(),
            ),
            serde_json::Value::Object(map) => {
                let mut new_map = serde_json::Map::with_capacity(map.len());
                for (k, v) in map.iter() {
                    new_map.insert(k.clone(), self.redact_args(v));
                }
                serde_json::Value::Object(new_map)
            }
            other => other.clone(),
        }
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow::anyhow!("credential name must not be empty"));
    }
    if name.len() > 64 {
        return Err(anyhow::anyhow!("credential name too long (max 64)"));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(anyhow::anyhow!(
            "credential name may only contain [A-Za-z0-9_-]"
        ));
    }
    Ok(())
}

static GLOBAL_VAULT: OnceLock<Arc<CredentialVault>> = OnceLock::new();

pub fn init_credential_vault(workspace_anchor: &Path) -> Result<Arc<CredentialVault>> {
    if let Some(v) = GLOBAL_VAULT.get() {
        return Ok(v.clone());
    }
    let target = preferred_vault_dir(workspace_anchor);
    if let Err(err) = migrate_legacy_vault(workspace_anchor, &target) {
        tracing::warn!(
            error = %err,
            target = %target.display(),
            anchor = %workspace_anchor.display(),
            "credential vault legacy migration failed; using fresh vault at target",
        );
    }
    let vault = CredentialVault::open(&target)?;
    let _ = GLOBAL_VAULT.set(vault.clone());
    Ok(vault)
}

pub fn try_get_credential_vault() -> Option<Arc<CredentialVault>> {
    GLOBAL_VAULT.get().cloned()
}

pub fn redact_for_audit_optional(input: &str) -> String {
    match try_get_credential_vault() {
        Some(v) => v.redact_for_audit(input),
        None => input.to_string(),
    }
}

pub fn redact_args_optional(value: &serde_json::Value) -> serde_json::Value {
    match try_get_credential_vault() {
        Some(v) => v.redact_args(value),
        None => value.clone(),
    }
}

#[derive(Default)]
struct EphemeralStore {
    by_session: HashMap<String, HashMap<String, String>>,
}

static EPHEMERAL_VAULT: OnceLock<RwLock<EphemeralStore>> = OnceLock::new();

fn ephemeral_lock() -> &'static RwLock<EphemeralStore> {
    EPHEMERAL_VAULT.get_or_init(|| RwLock::new(EphemeralStore::default()))
}

pub fn put_ephemeral_credential(session_id: &str, name: &str, value: &str) -> Result<()> {
    validate_name(name)?;
    let mut guard = ephemeral_lock().write();
    let session_map = guard
        .by_session
        .entry(session_id.to_string())
        .or_default();
    session_map.insert(name.to_string(), value.to_string());
    Ok(())
}

pub fn purge_session_ephemeral(session_id: &str) -> usize {
    let mut guard = ephemeral_lock().write();
    guard
        .by_session
        .remove(session_id)
        .map(|m| m.len())
        .unwrap_or(0)
}

pub fn list_ephemeral_names(session_id: &str) -> Vec<String> {
    let guard = ephemeral_lock().read();
    let mut names: Vec<String> = guard
        .by_session
        .get(session_id)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    names
}

pub fn count_ephemeral(session_id: &str) -> usize {
    let guard = ephemeral_lock().read();
    guard
        .by_session
        .get(session_id)
        .map(|m| m.len())
        .unwrap_or(0)
}

fn current_session_ephemeral_entries() -> HashMap<String, String> {
    let Some(ctx) = crate::session::current_session_context() else {
        return HashMap::new();
    };
    let guard = ephemeral_lock().read();
    guard
        .by_session
        .get(&ctx.session_id)
        .cloned()
        .unwrap_or_default()
}
