// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Trusted device management — mirrors claude-code-typescript-src`bridge/trustedDevice.ts`.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

/// A device that has been paired with the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedDevice {
    pub device_id: String,
    pub name: Option<String>,
    pub paired_at_epoch_ms: u64,
    pub last_seen_epoch_ms: u64,
    pub fingerprint: Option<String>,
}

/// Manages device pairing and trust.
#[derive(Clone)]
pub struct DeviceManager {
    inner: Arc<RwLock<DeviceManagerInner>>,
    storage_path: PathBuf,
}

struct DeviceManagerInner {
    trusted_devices: Vec<TrustedDevice>,
    active_paircode: Option<String>,
}

impl DeviceManager {
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            inner: Arc::new(RwLock::new(DeviceManagerInner {
                trusted_devices: Vec::new(),
                active_paircode: None,
            })),
            storage_path,
        }
    }

    /// Load trusted devices from disk.
    pub async fn load(&self) -> anyhow::Result<()> {
        let path = self.storage_path.join("trusted_devices.json");
        if path.exists() {
            let data = tokio::fs::read_to_string(&path).await?;
            let devices: Vec<TrustedDevice> = serde_json::from_str(&data)?;
            let mut inner = self.inner.write().await;
            inner.trusted_devices = devices;
        }
        Ok(())
    }

    /// Persist trusted devices to disk.
    pub async fn save(&self) -> anyhow::Result<()> {
        let inner = self.inner.read().await;
        let data = serde_json::to_string_pretty(&inner.trusted_devices)?;
        let path = self.storage_path.join("trusted_devices.json");
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, data).await?;
        Ok(())
    }

    /// Generate a new pairing code (6-digit numeric).
    pub async fn generate_paircode(&self) -> String {
        let code: String = (0..6)
            .map(|_| (rand::random::<u8>() % 10).to_string())
            .collect();
        let mut inner = self.inner.write().await;
        inner.active_paircode = Some(code.clone());
        code
    }

    /// Verify a pairing code and trust the device.
    pub async fn verify_paircode(
        &self,
        code: &str,
        device_id: &str,
        device_name: Option<&str>,
    ) -> bool {
        let mut inner = self.inner.write().await;
        if inner.active_paircode.as_deref() == Some(code) {
            inner.active_paircode = None;
            let now = now_ms();
            inner.trusted_devices.push(TrustedDevice {
                device_id: device_id.to_string(),
                name: device_name.map(|s| s.to_string()),
                paired_at_epoch_ms: now,
                last_seen_epoch_ms: now,
                fingerprint: None,
            });
            true
        } else {
            false
        }
    }

    /// Check whether a device is trusted.
    pub async fn is_trusted(&self, device_id: &str) -> bool {
        let inner = self.inner.read().await;
        inner
            .trusted_devices
            .iter()
            .any(|d| d.device_id == device_id)
    }

    /// Remove a trusted device.
    pub async fn revoke_device(&self, device_id: &str) -> bool {
        let mut inner = self.inner.write().await;
        let before = inner.trusted_devices.len();
        inner.trusted_devices.retain(|d| d.device_id != device_id);
        inner.trusted_devices.len() < before
    }

    /// List all trusted devices.
    pub async fn list_devices(&self) -> Vec<TrustedDevice> {
        let inner = self.inner.read().await;
        inner.trusted_devices.clone()
    }

    /// Update last-seen timestamp.
    pub async fn touch_device(&self, device_id: &str) {
        let mut inner = self.inner.write().await;
        if let Some(device) = inner
            .trusted_devices
            .iter_mut()
            .find(|d| d.device_id == device_id)
        {
            device.last_seen_epoch_ms = now_ms();
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
