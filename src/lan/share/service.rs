// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use anyhow::{anyhow, bail, Result};
use dashmap::DashMap;
use parking_lot::Mutex;
use serde_json::json;

use super::store::{ShareRecord, ShareStore};
use super::types::{ShareInbound, ShareView, ShareWire};
use crate::lan::discovery::PeerRegistry;
use crate::lan::identity::LanIdentity;
use crate::lan::protocol::ControlMessage;
use crate::lan::transport::{LanTransport, ShareReceived};

pub struct ShareService {
    identity: Arc<LanIdentity>,
    store: Arc<ShareStore>,
    registry: Arc<PeerRegistry>,
    peer_shares: DashMap<String, Vec<ShareWire>>,
    transport: Mutex<Weak<LanTransport>>,
}

impl ShareService {
    pub fn new(
        identity: Arc<LanIdentity>,
        store: Arc<ShareStore>,
        registry: Arc<PeerRegistry>,
    ) -> Arc<Self> {
        Arc::new(Self {
            identity,
            store,
            registry,
            peer_shares: DashMap::new(),
            transport: Mutex::new(Weak::new()),
        })
    }

    pub fn attach_transport(&self, transport: &Arc<LanTransport>) {
        *self.transport.lock() = Arc::downgrade(transport);
    }

    fn transport(&self) -> Option<Arc<LanTransport>> {
        self.transport.lock().upgrade()
    }

    fn online_set(&self) -> HashSet<String> {
        self.registry
            .snapshot()
            .into_iter()
            .map(|p| p.user_id)
            .collect()
    }

    fn nick_for(&self, user_id: &str) -> String {
        if user_id == self.identity.user_id() {
            return self.identity.nickname();
        }
        self.registry
            .get(user_id)
            .map(|r| r.nickname)
            .unwrap_or_else(|| user_id.to_string())
    }


    pub fn my_shares(&self) -> Vec<super::types::MyShareView> {
        self.store.my_views()
    }

    pub fn peer_shares(&self) -> Vec<ShareView> {
        let online = self.online_set();
        let mut out: Vec<ShareView> = Vec::new();
        for entry in self.peer_shares.iter() {
            let owner_id = entry.key();
            if !online.contains(owner_id) {
                continue;
            }
            let owner_nickname = self.nick_for(owner_id);
            for wire in entry.value() {
                out.push(ShareView {
                    id: wire.id.clone(),
                    owner_id: owner_id.clone(),
                    owner_nickname: owner_nickname.clone(),
                    name: wire.name.clone(),
                    is_dir: wire.is_dir,
                    size: wire.size,
                    note: wire.note.clone(),
                    online: true,
                    created_at: wire.created_at,
                });
            }
        }
        out.sort_by(|a, b| {
            a.owner_nickname
                .to_lowercase()
                .cmp(&b.owner_nickname.to_lowercase())
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        out
    }


    pub async fn add_share(self: &Arc<Self>, source: &str, note: &str) -> Result<String> {
        let source_path = PathBuf::from(shellexpand::tilde(source).to_string());
        if !source_path.exists() {
            bail!("source path does not exist");
        }
        let name = source_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| anyhow!("invalid source name"))?;
        let probe_path = source_path.clone();
        let (is_dir, size) = tokio::task::spawn_blocking(move || measure_path(&probe_path))
            .await
            .map_err(|e| anyhow!("measure task failed: {e}"))?;
        let record = ShareRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            path: source_path.to_string_lossy().to_string(),
            is_dir,
            size,
            content_hash: String::new(),
            note: note.trim().to_string(),
            created_at: now_ms(),
        };
        self.store.upsert(&record)?;
        let id = record.id.clone();
        self.broadcast_my_shares();
        self.emit_my_shares();
        Ok(id)
    }

    pub fn remove_share(self: &Arc<Self>, id: &str) -> Result<()> {
        if !self.store.remove(id) {
            bail!("share not found");
        }
        self.broadcast_my_shares();
        self.emit_my_shares();
        Ok(())
    }

    pub fn request_download(self: &Arc<Self>, owner_id: &str, share_id: &str) -> Result<()> {
        if owner_id == self.identity.user_id() {
            bail!("cannot download your own share");
        }
        if !self.online_set().contains(owner_id) {
            bail!("owner is offline");
        }
        let Some(transport) = self.transport() else {
            bail!("lan transport is not active");
        };
        let owner = owner_id.to_string();
        let share = share_id.to_string();
        tokio::spawn(async move {
            let msg = ControlMessage::ShareDownloadRequest { share_id: share };
            let _ = transport.send_control_message(&owner, &msg).await;
        });
        Ok(())
    }


    pub async fn handle_inbound(self: &Arc<Self>, peer_id: &str, msg: ShareInbound) {
        match msg {
            ShareInbound::ListRequest => {
                if let Some(transport) = self.transport() {
                    let response = ControlMessage::ShareListResponse {
                        shares: self.store.wire_views(),
                    };
                    let _ = transport.send_control_message(peer_id, &response).await;
                }
            }
            ShareInbound::ListResponse { shares } => {
                self.peer_shares.insert(peer_id.to_string(), shares);
                self.emit_peer_shares();
            }
            ShareInbound::DownloadRequest { share_id } => {
                self.handle_download_request(peer_id, &share_id).await;
            }
        }
    }

    pub async fn handle_peer_connected(self: &Arc<Self>, peer_id: &str) {
        let Some(transport) = self.transport() else {
            return;
        };
        let push = ControlMessage::ShareListResponse {
            shares: self.store.wire_views(),
        };
        let _ = transport.send_control_message(peer_id, &push).await;
        let _ = transport
            .send_control_message(peer_id, &ControlMessage::ShareListRequest)
            .await;
    }

    pub fn on_peers_changed(self: &Arc<Self>) {
        let online = self.online_set();
        let stale: Vec<String> = self
            .peer_shares
            .iter()
            .map(|e| e.key().clone())
            .filter(|id| !online.contains(id))
            .collect();
        let mut changed = false;
        for id in stale {
            if self.peer_shares.remove(&id).is_some() {
                changed = true;
            }
        }
        if changed {
            self.emit_peer_shares();
        }
    }

    pub async fn handle_share_received(self: &Arc<Self>, info: ShareReceived) {
        self.emit_share_downloaded(&info);
    }

    async fn handle_download_request(self: &Arc<Self>, peer_id: &str, share_id: &str) {
        let Some(record) = self.store.get(share_id) else {
            return;
        };
        let path = PathBuf::from(shellexpand::tilde(&record.path).to_string());
        if !path.exists() {
            return;
        }
        let Some(transport) = self.transport() else {
            return;
        };
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let _ = transport
            .send_share(peer_id, &transfer_id, share_id, &path)
            .await;
    }


    fn broadcast_my_shares(self: &Arc<Self>) {
        let Some(transport) = self.transport() else {
            return;
        };
        let online = self.online_set();
        let me = self.identity.user_id().to_string();
        let shares = self.store.wire_views();
        tokio::spawn(async move {
            let msg = ControlMessage::ShareListResponse { shares };
            for peer in online {
                if peer == me {
                    continue;
                }
                let _ = transport.send_control_message(&peer, &msg).await;
            }
        });
    }

    fn emit_my_shares(&self) {
        emit_share("lan_shares", json!({ "shares": self.store.my_views() }));
    }

    fn emit_peer_shares(&self) {
        emit_share("lan_share_peers", json!({ "shares": self.peer_shares() }));
    }

    fn emit_share_downloaded(&self, info: &ShareReceived) {
        emit_share(
            "lan_share_downloaded",
            json!({
                "peerId": info.peer_id,
                "ownerNickname": self.nick_for(&info.peer_id),
                "shareId": info.share_id,
                "name": info.name,
                "path": info.path.to_string_lossy().to_string(),
                "isDir": info.is_dir,
                "size": info.size,
            }),
        );
    }
}

fn measure_path(path: &Path) -> (bool, i64) {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => (true, i64::try_from(dir_size(path)).unwrap_or(i64::MAX)),
        Ok(meta) => (false, i64::try_from(meta.len()).unwrap_or(i64::MAX)),
        Err(_) => (false, 0),
    }
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(read_dir) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in read_dir.flatten() {
        match entry.metadata() {
            Ok(meta) if meta.is_dir() => total += dir_size(&entry.path()),
            Ok(meta) => total += meta.len(),
            Err(_) => {}
        }
    }
    total
}

fn emit_share(kind: &str, data: serde_json::Value) {
    crate::gateway::emit_gateway_event(json!({
        "type": "lan_event",
        "kind": kind,
        "data": data,
    }));
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
