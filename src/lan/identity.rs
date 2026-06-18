// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::crypto::{random_seed, StaticKeypair};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IdentityFile {
    #[serde(rename = "userId")]
    user_id: String,
    seed: String,
    nickname: String,
    #[serde(default)]
    email: Option<String>,
}

pub struct LanIdentity {
    user_id: String,
    hostname: String,
    keypair: StaticKeypair,
    path: PathBuf,
    mutable: RwLock<MutableProfile>,
}

#[derive(Clone)]
struct MutableProfile {
    nickname: String,
    email: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentitySnapshot {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub hostname: String,
    pub nickname: String,
    pub email: Option<String>,
    #[serde(rename = "localIp")]
    pub local_ip: Option<String>,
    #[serde(rename = "publicKey")]
    pub public_key: String,
}

impl LanIdentity {
    pub fn load_or_create(sen_dir: &Path) -> Result<Arc<Self>> {
        let dir = sen_dir.join("lan");
        std::fs::create_dir_all(&dir).context("creating lan data dir")?;
        let path = dir.join("identity.json");

        if let Ok(body) = std::fs::read_to_string(&path) {
            if let Ok(parsed) = serde_json::from_str::<IdentityFile>(&body) {
                if let Some(seed) = decode_seed(&parsed.seed) {
                    let keypair = StaticKeypair::from_seed(&seed);
                    return Ok(Arc::new(Self {
                        user_id: parsed.user_id,
                        hostname: detect_hostname(),
                        keypair,
                        path,
                        mutable: RwLock::new(MutableProfile {
                            nickname: parsed.nickname,
                            email: parsed.email,
                        }),
                    }));
                }
            }
        }

        let user_id = generate_user_id();
        let seed = random_seed();
        let keypair = StaticKeypair::from_seed(&seed);
        let identity = Self {
            user_id: user_id.clone(),
            hostname: detect_hostname(),
            keypair,
            path,
            mutable: RwLock::new(MutableProfile {
                nickname: user_id,
                email: None,
            }),
        };
        identity.persist(&seed)?;
        Ok(Arc::new(identity))
    }

    fn persist(&self, seed: &[u8; 32]) -> Result<()> {
        let profile = self.mutable.read().clone();
        let file = IdentityFile {
            user_id: self.user_id.clone(),
            seed: hex::encode(seed),
            nickname: profile.nickname,
            email: profile.email,
        };
        let serialized = serde_json::to_string_pretty(&file).context("serializing identity")?;
        crate::util::atomic_write(&self.path, serialized.as_bytes())
            .map_err(|e| anyhow::anyhow!("writing identity: {e}"))?;
        Ok(())
    }

    fn persist_existing(&self) -> Result<()> {
        let body = std::fs::read_to_string(&self.path).context("reading identity for update")?;
        let parsed: IdentityFile = serde_json::from_str(&body).context("parsing identity")?;
        let seed = decode_seed(&parsed.seed).context("decoding identity seed")?;
        self.persist(&seed)
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    pub fn nickname(&self) -> String {
        self.mutable.read().nickname.clone()
    }

    pub fn email(&self) -> Option<String> {
        self.mutable.read().email.clone()
    }

    pub fn keypair(&self) -> &StaticKeypair {
        &self.keypair
    }

    pub fn public_b64(&self) -> String {
        self.keypair.public_b64()
    }

    pub fn set_profile(&self, nickname: Option<String>, email: Option<Option<String>>) -> Result<()> {
        {
            let mut guard = self.mutable.write();
            if let Some(nick) = nickname {
                let trimmed = nick.trim();
                guard.nickname = if trimmed.is_empty() {
                    self.user_id.clone()
                } else {
                    trimmed.to_string()
                };
            }
            if let Some(email) = email {
                guard.email = email
                    .map(|e| e.trim().to_string())
                    .filter(|e| !e.is_empty());
            }
        }
        self.persist_existing()
    }

    pub fn snapshot(&self) -> IdentitySnapshot {
        let profile = self.mutable.read().clone();
        IdentitySnapshot {
            user_id: self.user_id.clone(),
            hostname: self.hostname.clone(),
            nickname: profile.nickname,
            email: profile.email,
            local_ip: detect_local_ip(),
            public_key: self.keypair.public_b64(),
        }
    }
}

pub fn detect_local_ip() -> Option<String> {
    match local_ip_address::local_ip() {
        Ok(IpAddr::V4(ip)) => Some(ip.to_string()),
        Ok(IpAddr::V6(ip)) => Some(ip.to_string()),
        Err(_) => local_ip_address::list_afinet_netifas()
            .ok()
            .and_then(|ifaces| {
                ifaces
                    .into_iter()
                    .find(|(_, ip)| match ip {
                        IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_unspecified(),
                        IpAddr::V6(_) => false,
                    })
                    .map(|(_, ip)| ip.to_string())
            }),
    }
}

fn detect_hostname() -> String {
    hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "device".to_string())
}

fn generate_user_id() -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"senweaver-lan-identity-v1");
    for mac in collect_mac_addresses() {
        hasher.update(mac.as_bytes());
    }
    hasher.update(detect_hostname().as_bytes());
    hasher.update(std::env::consts::OS.as_bytes());
    hasher.update(detect_username().as_bytes());
    let digest = hasher.finalize();
    let hex = hex::encode(digest);
    format!("SEN-{}", hex[..12].to_uppercase())
}

fn collect_mac_addresses() -> Vec<String> {
    let mut macs = Vec::new();
    if let Ok(iter) = mac_address::MacAddressIterator::new() {
        for mac in iter {
            let s = mac.to_string();
            if !s.is_empty() && s != "00:00:00:00:00:00" {
                macs.push(s);
            }
        }
    }
    if macs.is_empty() {
        if let Ok(Some(mac)) = mac_address::get_mac_address() {
            macs.push(mac.to_string());
        }
    }
    macs.sort();
    macs.dedup();
    macs
}

fn detect_username() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "user".to_string())
}

fn decode_seed(value: &str) -> Option<[u8; 32]> {
    let bytes = hex::decode(value.trim()).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);
    Some(seed)
}
