// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde_json::json;

use super::discovery::{Discovery, DiscoveryParams, PeerRegistry};
use super::group::op::GroupInbound;
use super::group::store::GroupStore;
use super::group::GroupService;
use super::identity::LanIdentity;
use super::share::store::ShareStore;
use super::share::types::ShareInbound;
use super::share::ShareService;
use super::store::{LanStore, NewMessage};
use super::transport::{
    GroupDocReceived, LanEvents, LanTransport, ShareReceived, TransferUpdate,
};

const FILE_MARKER: &str = "__lan_file__:";

pub struct LanService {
    identity: Arc<LanIdentity>,
    store: Arc<LanStore>,
    registry: Arc<PeerRegistry>,
    transport: Arc<LanTransport>,
    group: Arc<GroupService>,
    share: Arc<ShareService>,
    discovery: Mutex<Option<Discovery>>,
    running: AtomicBool,
    service_type: String,
    configured_port: u16,
    lan_root: PathBuf,
    downloads_dir: PathBuf,
}

struct LanEventSink {
    store: Arc<LanStore>,
    registry: Arc<PeerRegistry>,
    group: Arc<GroupService>,
    share: Arc<ShareService>,
}

impl LanService {
    pub fn new(
        sen_dir: &std::path::Path,
        shared_config: &Arc<crate::config::hot_reload::SharedConfig>,
    ) -> Result<Arc<Self>> {
        let lan_cfg = shared_config.load().lan.clone();
        let identity = LanIdentity::load_or_create(sen_dir)?;
        let store = Arc::new(LanStore::open(sen_dir)?);
        let registry = Arc::new(PeerRegistry::new());

        let lan_root = sen_dir.join("lan");
        let downloads_dir = match lan_cfg.download_dir.as_ref().filter(|d| !d.trim().is_empty()) {
            Some(dir) => PathBuf::from(shellexpand::tilde(dir).to_string()),
            None => lan_root.join("downloads"),
        };

        let group_store = Arc::new(GroupStore::open(sen_dir, identity.user_id())?);
        let group_docs_root = sen_dir.join("lan").join("groups");
        let group = GroupService::new(
            Arc::clone(&identity),
            group_store,
            Arc::clone(&registry),
            group_docs_root,
        );

        let share_store = Arc::new(ShareStore::open(sen_dir)?);
        let share = ShareService::new(
            Arc::clone(&identity),
            share_store,
            Arc::clone(&registry),
        );

        let sink: Arc<dyn LanEvents> = Arc::new(LanEventSink {
            store: Arc::clone(&store),
            registry: Arc::clone(&registry),
            group: Arc::clone(&group),
            share: Arc::clone(&share),
        });

        let transport = Arc::new(LanTransport::new(
            Arc::clone(&identity),
            Arc::clone(&registry),
            sink,
            downloads_dir.clone(),
            lan_cfg.chunk_size.max(4096),
            lan_cfg.max_frame_bytes.max(lan_cfg.chunk_size + 4096),
            lan_cfg.num_streams,
        ));
        group.attach_transport(&transport);
        share.attach_transport(&transport);

        Ok(Arc::new(Self {
            identity,
            store,
            registry,
            transport,
            group,
            share,
            discovery: Mutex::new(None),
            running: AtomicBool::new(false),
            service_type: lan_cfg.service_name,
            configured_port: lan_cfg.port,
            lan_root,
            downloads_dir,
        }))
    }

    pub fn group(&self) -> Arc<GroupService> {
        Arc::clone(&self.group)
    }

    pub fn share(&self) -> Arc<ShareService> {
        Arc::clone(&self.share)
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub async fn start(self: &Arc<Self>) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }
        let port = self.transport.bind_listener(self.configured_port).await?;
        self.spawn_discovery(port)?;
        self.running.store(true, Ordering::Relaxed);
        self.emit_status();
        self.emit_peers();
        Ok(())
    }

    pub fn stop(&self) {
        if !self.running.swap(false, Ordering::Relaxed) {
            return;
        }
        *self.discovery.lock() = None;
        self.transport.shutdown();
        self.registry.clear();
        self.emit_status();
        self.emit_peers();
    }

    fn spawn_discovery(&self, port: u16) -> Result<()> {
        let registry = Arc::clone(&self.registry);
        let runtime = tokio::runtime::Handle::current();
        let on_change: Arc<dyn Fn() + Send + Sync> = {
            let reg = Arc::clone(&self.registry);
            let group = Arc::clone(&self.group);
            let share = Arc::clone(&self.share);
            let runtime = runtime.clone();
            Arc::new(move || {
                let snapshot = reg.snapshot();
                emit_lan("lan_peers", json!({ "peers": snapshot }));
                let group = Arc::clone(&group);
                let share = Arc::clone(&share);
                runtime.spawn(async move {
                    group.on_peers_changed();
                    share.on_peers_changed();
                });
            })
        };
        let discovery = Discovery::start(
            DiscoveryParams {
                service_type: self.service_type.clone(),
                user_id: self.identity.user_id().to_string(),
                nickname: self.identity.nickname(),
                public_key: self.identity.public_b64(),
                port,
            },
            registry,
            on_change,
        )?;
        *self.discovery.lock() = Some(discovery);
        Ok(())
    }

    pub fn identity_snapshot(&self) -> serde_json::Value {
        let snapshot = self.identity.snapshot();
        let mut value = serde_json::to_value(&snapshot).unwrap_or_else(|_| json!({}));
        if let Some(obj) = value.as_object_mut() {
            obj.insert("running".to_string(), json!(self.is_running()));
            obj.insert("port".to_string(), json!(self.transport.listen_port()));
        }
        value
    }

    pub fn set_profile(
        self: &Arc<Self>,
        nickname: Option<String>,
        email: Option<Option<String>>,
    ) -> Result<()> {
        self.identity.set_profile(nickname, email)?;
        if self.is_running() {
            let port = self.transport.listen_port();
            let _ = self.spawn_discovery(port);
        }
        emit_lan("lan_identity", self.identity_snapshot());
        Ok(())
    }

    pub fn peers(&self) -> Vec<super::discovery::PeerView> {
        self.registry.snapshot()
    }

    pub fn history(&self, peer_id: &str, limit: i64) -> Result<Vec<super::store::MessageView>> {
        self.store.list_messages(peer_id, limit)
    }

    pub fn conversations(&self) -> Result<Vec<super::store::ConversationView>> {
        self.store.conversations()
    }

    pub fn transfers(&self) -> Result<Vec<super::store::TransferView>> {
        self.store.list_transfers(200)
    }

    pub fn unread_total(&self) -> i64 {
        self.store.unread_total()
    }

    pub fn mark_read(&self, peer_id: &str) -> Result<()> {
        self.store.mark_read(peer_id)?;
        emit_lan("lan_unread", json!({ "unread": self.store.unread_total() }));
        Ok(())
    }

    pub async fn send_text(self: &Arc<Self>, peer_id: &str, body: &str) -> Result<String> {
        let msg_id = uuid::Uuid::new_v4().to_string();
        let ts_ms = now_ms();
        let nickname = self
            .registry
            .get(peer_id)
            .map(|r| r.nickname)
            .unwrap_or_else(|| peer_id.to_string());
        let public_key = self.registry.get(peer_id).map(|r| r.public_key);
        let ip = self.registry.get(peer_id).map(|r| r.addr.ip().to_string());
        self.store.upsert_peer(
            peer_id,
            &nickname,
            None,
            ip.as_deref(),
            public_key.as_deref(),
            ts_ms,
        )?;
        self.store.record_message(&NewMessage {
            id: msg_id.clone(),
            peer_id: peer_id.to_string(),
            direction: "out".to_string(),
            kind: "text".to_string(),
            body: body.to_string(),
            file_name: None,
            file_path: None,
            file_size: None,
            created_at: ts_ms,
            read: true,
        })?;
        self.transport.send_text(peer_id, &msg_id, ts_ms, body).await?;
        emit_lan(
            "lan_message",
            json!({
                "message": {
                    "id": msg_id,
                    "peerId": peer_id,
                    "direction": "out",
                    "kind": "text",
                    "body": body,
                    "createdAt": ts_ms,
                    "read": true,
                }
            }),
        );
        Ok(msg_id)
    }

    pub async fn save_received(&self, source: &str, dest_dir: &str) -> Result<String> {
        let source_path = PathBuf::from(shellexpand::tilde(source).to_string());
        let dest_root = PathBuf::from(shellexpand::tilde(dest_dir).to_string());
        if !source_path.exists() {
            anyhow::bail!("source path no longer exists");
        }
        tokio::fs::create_dir_all(&dest_root)
            .await
            .with_context(|| format!("creating destination {}", dest_root.display()))?;
        let file_name = source_path
            .file_name()
            .map(|n| n.to_os_string())
            .ok_or_else(|| anyhow::anyhow!("invalid source name"))?;
        let target = unique_dest(&dest_root, &file_name);
        let src = source_path.clone();
        let dst = target.clone();
        tokio::task::spawn_blocking(move || copy_path(&src, &dst))
            .await
            .map_err(|e| anyhow::anyhow!("copy task failed: {e}"))??;
        Ok(target.to_string_lossy().to_string())
    }

    pub fn outbox_dir(&self) -> PathBuf {
        self.lan_root.join("outbox")
    }

    pub async fn send_image(
        self: &Arc<Self>,
        peer_id: &str,
        file_name: &str,
        bytes: Vec<u8>,
    ) -> Result<String> {
        if bytes.is_empty() {
            anyhow::bail!("image payload is empty");
        }
        let dir = self.outbox_dir();
        tokio::fs::create_dir_all(&dir)
            .await
            .with_context(|| format!("creating outbox {}", dir.display()))?;
        let safe = sanitize_outbox_name(file_name);
        let unique = format!("{}-{}", uuid::Uuid::new_v4(), safe);
        let path = dir.join(unique);
        tokio::fs::write(&path, &bytes)
            .await
            .with_context(|| format!("writing image to {}", path.display()))?;
        Ok(self.send_path(peer_id, &path.to_string_lossy()))
    }

    pub fn read_shared_file(&self, raw_path: &str) -> Result<(Vec<u8>, String)> {
        let requested = PathBuf::from(shellexpand::tilde(raw_path).to_string());
        let canonical = std::fs::canonicalize(&requested)
            .with_context(|| format!("resolving {}", requested.display()))?;
        let allowed = [
            std::fs::canonicalize(&self.lan_root).ok(),
            std::fs::canonicalize(&self.downloads_dir).ok(),
        ];
        let permitted = allowed
            .iter()
            .flatten()
            .any(|root| canonical.starts_with(root));
        if !permitted {
            anyhow::bail!("path is not within a shared directory");
        }
        if !canonical.is_file() {
            anyhow::bail!("not a file");
        }
        let bytes = std::fs::read(&canonical)
            .with_context(|| format!("reading {}", canonical.display()))?;
        let mime = super::guess_mime(&canonical.to_string_lossy());
        Ok((bytes, mime))
    }

    pub fn send_path(self: &Arc<Self>, peer_id: &str, path: &str) -> String {
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let this = Arc::clone(self);
        let peer = peer_id.to_string();
        let source = PathBuf::from(shellexpand::tilde(path).to_string());
        let transfer = transfer_id.clone();
        tokio::spawn(async move {
            match this.transport.send_path(&peer, &transfer, &source).await {
                Ok(()) => {
                    let name = source
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "file".to_string());
                    let ts = now_ms();
                    let _ = this.store.record_message(&NewMessage {
                        id: format!("file-out-{transfer}"),
                        peer_id: peer.clone(),
                        direction: "out".to_string(),
                        kind: "file".to_string(),
                        body: name.clone(),
                        file_name: Some(name.clone()),
                        file_path: Some(source.to_string_lossy().to_string()),
                        file_size: None,
                        created_at: ts,
                        read: true,
                    });
                    emit_lan(
                        "lan_message",
                        json!({
                            "message": {
                                "id": format!("file-out-{transfer}"),
                                "peerId": peer,
                                "direction": "out",
                                "kind": "file",
                                "body": name,
                                "fileName": name,
                                "filePath": source.to_string_lossy(),
                                "createdAt": ts,
                                "read": true,
                            }
                        }),
                    );
                }
                Err(err) => {
                    tracing::debug!(error = %err, "lan send_path failed");
                    emit_lan(
                        "lan_transfer",
                        json!({
                            "transfer": {
                                "id": transfer,
                                "peerId": peer,
                                "direction": "out",
                                "status": "failed",
                            }
                        }),
                    );
                }
            }
        });
        transfer_id
    }
}

impl LanEvents for LanEventSink {
    fn on_incoming_chat(&self, peer_id: &str, msg_id: &str, ts_ms: i64, body: &str) {
        let nickname = self
            .registry
            .get(peer_id)
            .map(|r| r.nickname)
            .or_else(|| self.store.peer_nickname(peer_id))
            .unwrap_or_else(|| peer_id.to_string());
        let public_key = self.registry.get(peer_id).map(|r| r.public_key);
        let ip = self.registry.get(peer_id).map(|r| r.addr.ip().to_string());
        let _ = self.store.upsert_peer(
            peer_id,
            &nickname,
            None,
            ip.as_deref(),
            public_key.as_deref(),
            ts_ms,
        );

        let (kind, body_text, file_name, file_path) = parse_incoming(body);
        let message = NewMessage {
            id: msg_id.to_string(),
            peer_id: peer_id.to_string(),
            direction: "in".to_string(),
            kind: kind.to_string(),
            body: body_text.clone(),
            file_name: file_name.clone(),
            file_path: file_path.clone(),
            file_size: None,
            created_at: ts_ms,
            read: false,
        };
        let _ = self.store.record_message(&message);

        emit_lan(
            "lan_message",
            json!({
                "message": {
                    "id": msg_id,
                    "peerId": peer_id,
                    "direction": "in",
                    "kind": kind,
                    "body": body_text,
                    "fileName": file_name,
                    "filePath": file_path,
                    "createdAt": ts_ms,
                    "read": false,
                }
            }),
        );
        emit_lan("lan_unread", json!({ "unread": self.store.unread_total() }));
    }

    fn on_transfer_update(&self, update: TransferUpdate) {
        let now = now_ms();
        let _ = self.store.upsert_transfer(
            &update.transfer_id,
            &update.peer_id,
            &update.direction,
            &update.name,
            update.path.as_deref(),
            update.size,
            update.transferred,
            &update.status,
            now,
        );
        emit_lan(
            "lan_transfer",
            json!({
                "transfer": {
                    "id": update.transfer_id,
                    "peerId": update.peer_id,
                    "direction": update.direction,
                    "name": update.name,
                    "path": update.path,
                    "size": update.size,
                    "transferred": update.transferred,
                    "status": update.status,
                }
            }),
        );
    }

    fn on_connection_change(&self) {}

    fn on_peer_connected(&self, peer_id: &str) {
        let group = Arc::clone(&self.group);
        let share = Arc::clone(&self.share);
        let peer = peer_id.to_string();
        tokio::spawn(async move {
            group.handle_peer_connected(&peer).await;
            share.handle_peer_connected(&peer).await;
        });
    }

    fn on_group_control(&self, peer_id: &str, msg: GroupInbound) {
        let group = Arc::clone(&self.group);
        let peer = peer_id.to_string();
        tokio::spawn(async move {
            group.handle_inbound(&peer, msg).await;
        });
    }

    fn on_group_doc_received(&self, info: GroupDocReceived) {
        let group = Arc::clone(&self.group);
        tokio::spawn(async move {
            group.handle_doc_received(info).await;
        });
    }

    fn on_share_control(&self, peer_id: &str, msg: ShareInbound) {
        let share = Arc::clone(&self.share);
        let peer = peer_id.to_string();
        tokio::spawn(async move {
            share.handle_inbound(&peer, msg).await;
        });
    }

    fn on_share_received(&self, info: ShareReceived) {
        let share = Arc::clone(&self.share);
        tokio::spawn(async move {
            share.handle_share_received(info).await;
        });
    }
}

impl LanService {
    fn emit_status(&self) {
        emit_lan(
            "lan_status",
            json!({
                "running": self.is_running(),
                "port": self.transport.listen_port(),
            }),
        );
    }

    fn emit_peers(&self) {
        emit_lan("lan_peers", json!({ "peers": self.registry.snapshot() }));
    }
}

fn parse_incoming(body: &str) -> (&'static str, String, Option<String>, Option<String>) {
    if let Some(rest) = body.strip_prefix(FILE_MARKER) {
        let mut parts = rest.splitn(2, ':');
        let name = parts.next().unwrap_or("file").to_string();
        let path = parts.next().map(str::to_string);
        return ("file", name.clone(), Some(name), path);
    }
    ("text", body.to_string(), None, None)
}

fn sanitize_outbox_name(name: &str) -> String {
    let base = std::path::Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "image.png".to_string());
    let cleaned: String = base
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    if cleaned.trim().is_empty() {
        "image.png".to_string()
    } else {
        cleaned
    }
}

fn emit_lan(kind: &str, data: serde_json::Value) {
    crate::gateway::emit_gateway_event(json!({
        "type": "lan_event",
        "kind": kind,
        "data": data,
    }));
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn unique_dest(dir: &std::path::Path, name: &std::ffi::OsStr) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let name_str = name.to_string_lossy();
    let path = std::path::Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name_str.to_string());
    let ext = path
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    for i in 1..10_000 {
        let candidate = dir.join(format!("{stem} ({i}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}{ext}", uuid::Uuid::new_v4()))
}

fn copy_path(source: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(source)
        .with_context(|| format!("reading metadata for {}", source.display()))?;
    if meta.is_dir() {
        std::fs::create_dir_all(dest)
            .with_context(|| format!("creating dir {}", dest.display()))?;
        for entry in std::fs::read_dir(source)
            .with_context(|| format!("reading dir {}", source.display()))?
        {
            let entry = entry?;
            let child_dest = dest.join(entry.file_name());
            copy_path(&entry.path(), &child_dest)?;
        }
    } else {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating parent {}", parent.display()))?;
        }
        std::fs::copy(source, dest)
            .with_context(|| format!("copying {} -> {}", source.display(), dest.display()))?;
    }
    Ok(())
}
