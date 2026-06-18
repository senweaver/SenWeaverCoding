// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use dashmap::DashMap;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use super::crypto::{public_from_b64, SessionCipher};
use super::discovery::PeerRegistry;
use super::group::op::GroupInbound;
use super::identity::LanIdentity;
use super::share::types::ShareInbound;
use super::protocol::{
    decode_frame, encode_control, encode_file_chunk, read_frame, write_frame, ControlMessage,
    DecodedFrame, Hello,
};

const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(150);

#[derive(Debug, Clone)]
pub struct TransferUpdate {
    pub transfer_id: String,
    pub peer_id: String,
    pub direction: String,
    pub name: String,
    pub path: Option<String>,
    pub size: i64,
    pub transferred: i64,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct GroupDocReceived {
    pub group_id: String,
    pub doc_id: String,
    pub peer_id: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: i64,
}

#[derive(Debug, Clone)]
pub struct ShareReceived {
    pub share_id: String,
    pub peer_id: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: i64,
    pub name: String,
}

pub trait LanEvents: Send + Sync {
    fn on_incoming_chat(&self, peer_id: &str, msg_id: &str, ts_ms: i64, body: &str);
    fn on_transfer_update(&self, update: TransferUpdate);
    fn on_connection_change(&self);
    fn on_peer_connected(&self, _peer_id: &str) {}
    fn on_group_control(&self, _peer_id: &str, _msg: GroupInbound) {}
    fn on_group_doc_received(&self, _info: GroupDocReceived) {}
    fn on_share_control(&self, _peer_id: &str, _msg: ShareInbound) {}
    fn on_share_received(&self, _info: ShareReceived) {}
}

struct PeerLink {
    streams: Vec<mpsc::Sender<Vec<u8>>>,
    cursor: AtomicUsize,
}

impl PeerLink {
    async fn send_control(&self, plain: Vec<u8>) -> Result<()> {
        self.streams[0]
            .send(plain)
            .await
            .map_err(|_| anyhow!("lan connection closed"))
    }

    async fn send_chunk(&self, plain: Vec<u8>) -> Result<()> {
        let n = self.streams.len();
        let idx = self.cursor.fetch_add(1, Ordering::Relaxed) % n;
        self.streams[idx]
            .send(plain)
            .await
            .map_err(|_| anyhow!("lan connection closed"))
    }
}

enum InboundOp {
    Offer {
        name: String,
        is_dir: bool,
        display_total: u64,
        group_id: String,
        doc_id: String,
        share_id: String,
    },
    Chunk {
        offset: u64,
        data: Vec<u8>,
    },
    Complete {
        total: u64,
    },
}

struct InboundHandle {
    peer_id: String,
    tx: mpsc::Sender<InboundOp>,
}

pub struct LanTransport {
    identity: Arc<LanIdentity>,
    registry: Arc<PeerRegistry>,
    events: Arc<dyn LanEvents>,
    downloads_dir: PathBuf,
    chunk_size: usize,
    max_frame: usize,
    num_streams: usize,
    links: Arc<DashMap<String, Arc<PeerLink>>>,
    inbound: Arc<DashMap<String, InboundHandle>>,
    finished: Arc<DashMap<String, ()>>,
    listen_port: AtomicU16,
}

impl LanTransport {
    pub fn new(
        identity: Arc<LanIdentity>,
        registry: Arc<PeerRegistry>,
        events: Arc<dyn LanEvents>,
        downloads_dir: PathBuf,
        chunk_size: usize,
        max_frame: usize,
        num_streams: usize,
    ) -> Self {
        Self {
            identity,
            registry,
            events,
            downloads_dir,
            chunk_size,
            max_frame,
            num_streams: num_streams.clamp(1, 16),
            links: Arc::new(DashMap::new()),
            inbound: Arc::new(DashMap::new()),
            finished: Arc::new(DashMap::new()),
            listen_port: AtomicU16::new(0),
        }
    }

    pub fn listen_port(&self) -> u16 {
        self.listen_port.load(Ordering::Relaxed)
    }

    pub async fn bind_listener(self: &Arc<Self>, port: u16) -> Result<u16> {
        let listener = TcpListener::bind(("0.0.0.0", port))
            .await
            .context("binding lan tcp listener")?;
        let actual = listener.local_addr()?.port();
        self.listen_port.store(actual, Ordering::Relaxed);

        let this = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _addr)) => {
                        let inner = Arc::clone(&this);
                        tokio::spawn(async move {
                            if let Err(err) = inner.accept_connection(stream).await {
                                tracing::debug!(error = %err, "lan inbound connection failed");
                            }
                        });
                    }
                    Err(err) => {
                        tracing::debug!(error = %err, "lan accept error");
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                }
            }
        });
        Ok(actual)
    }

    pub fn shutdown(&self) {
        self.links.clear();
        self.inbound.clear();
        self.finished.clear();
        self.listen_port.store(0, Ordering::Relaxed);
    }

    async fn accept_connection(self: &Arc<Self>, stream: TcpStream) -> Result<()> {
        stream.set_nodelay(true).ok();
        let (mut read_half, mut write_half) = stream.into_split();

        let hello_bytes = read_frame(&mut read_half, 64 * 1024).await?;
        let peer_hello: Hello =
            serde_json::from_slice(&hello_bytes).context("parsing peer hello")?;

        let our_hello = self.build_hello();
        write_frame(&mut write_half, &serde_json::to_vec(&our_hello)?).await?;

        let peer_pub = public_from_b64(&peer_hello.public_key)
            .ok_or_else(|| anyhow!("invalid peer public key"))?;
        let session_key = self.identity.keypair().session_key(&peer_pub);
        let cipher = SessionCipher::new(&session_key);

        self.register_peer_seen(&peer_hello);
        self.events.on_connection_change();

        let peer_id = peer_hello.user_id;
        self.events.on_peer_connected(&peer_id);
        self.spawn_reader(peer_id, read_half, cipher);
        Ok(())
    }

    fn spawn_reader(self: &Arc<Self>, peer_id: String, mut read_half: OwnedReadHalf, cipher: SessionCipher) {
        let this = Arc::clone(self);
        let max_frame = self.max_frame;
        tokio::spawn(async move {
            loop {
                let frame = match read_frame(&mut read_half, max_frame).await {
                    Ok(frame) => frame,
                    Err(_) => break,
                };
                let plaintext = match cipher.open(&frame) {
                    Some(plaintext) => plaintext,
                    None => break,
                };
                match decode_frame(&plaintext) {
                    Ok(decoded) => this.handle_decoded(&peer_id, decoded).await,
                    Err(err) => {
                        tracing::debug!(error = %err, "lan frame decode error");
                    }
                }
            }
            this.fail_inbound_for_peer(&peer_id).await;
            this.events.on_connection_change();
        });
    }

    async fn ensure_link(self: &Arc<Self>, peer_id: &str) -> Result<Arc<PeerLink>> {
        if let Some(existing) = self.links.get(peer_id) {
            return Ok(Arc::clone(existing.value()));
        }
        let record = self
            .registry
            .get(peer_id)
            .ok_or_else(|| anyhow!("peer not discovered: {peer_id}"))?;

        let mut streams = Vec::with_capacity(self.num_streams);
        for _ in 0..self.num_streams {
            let (write_half, key) = self.dial_stream(record.addr).await?;
            let (tx, rx) = mpsc::channel::<Vec<u8>>(8);
            spawn_stream_writer(write_half, rx, SessionCipher::new(&key));
            streams.push(tx);
        }
        let link = Arc::new(PeerLink {
            streams,
            cursor: AtomicUsize::new(0),
        });
        self.links.insert(peer_id.to_string(), Arc::clone(&link));
        self.events.on_connection_change();
        self.events.on_peer_connected(peer_id);
        Ok(link)
    }

    async fn dial_stream(self: &Arc<Self>, addr: std::net::SocketAddr) -> Result<(OwnedWriteHalf, [u8; 32])> {
        let stream = TcpStream::connect(addr)
            .await
            .with_context(|| format!("connecting to {addr}"))?;
        stream.set_nodelay(true).ok();
        let (mut read_half, mut write_half) = stream.into_split();

        let our_hello = self.build_hello();
        write_frame(&mut write_half, &serde_json::to_vec(&our_hello)?).await?;
        let hello_bytes = read_frame(&mut read_half, 64 * 1024).await?;
        let peer_hello: Hello =
            serde_json::from_slice(&hello_bytes).context("parsing peer hello")?;

        let peer_pub = public_from_b64(&peer_hello.public_key)
            .ok_or_else(|| anyhow!("invalid peer public key"))?;
        let key = self.identity.keypair().session_key(&peer_pub);
        self.register_peer_seen(&peer_hello);
        Ok((write_half, key))
    }

    async fn handle_decoded(self: &Arc<Self>, peer_id: &str, decoded: DecodedFrame) {
        match decoded {
            DecodedFrame::Control(ControlMessage::Chat { id, ts_ms, body }) => {
                self.events.on_incoming_chat(peer_id, &id, ts_ms, &body);
            }
            DecodedFrame::Control(ControlMessage::FileOffer {
                transfer_id,
                name,
                is_dir,
                total_size,
                group_id,
                doc_id,
                share_id,
            }) => {
                if let Some(tx) = self.inbound_sender(peer_id, &transfer_id) {
                    let _ = tx
                        .send(InboundOp::Offer {
                            name,
                            is_dir,
                            display_total: total_size,
                            group_id,
                            doc_id,
                            share_id,
                        })
                        .await;
                }
            }
            DecodedFrame::Control(ControlMessage::FileComplete {
                transfer_id,
                total_size,
            }) => {
                if let Some(tx) = self.inbound_sender(peer_id, &transfer_id) {
                    let _ = tx.send(InboundOp::Complete { total: total_size }).await;
                }
            }
            DecodedFrame::Control(ControlMessage::FileAbort { transfer_id, .. }) => {
                self.inbound.remove(&transfer_id);
            }
            DecodedFrame::Control(ControlMessage::Ack { .. }) => {}
            DecodedFrame::Control(ControlMessage::GroupGossip { group_id, ops }) => {
                self.events
                    .on_group_control(peer_id, GroupInbound::Gossip { group_id, ops });
            }
            DecodedFrame::Control(ControlMessage::GroupSyncRequest { groups }) => {
                self.events
                    .on_group_control(peer_id, GroupInbound::SyncRequest { groups });
            }
            DecodedFrame::Control(ControlMessage::GroupSyncResponse { groups }) => {
                self.events
                    .on_group_control(peer_id, GroupInbound::SyncResponse { groups });
            }
            DecodedFrame::Control(ControlMessage::GroupInvite { group_id, ops }) => {
                self.events
                    .on_group_control(peer_id, GroupInbound::Invite { group_id, ops });
            }
            DecodedFrame::Control(ControlMessage::GroupDocRequest { group_id, doc_id }) => {
                self.events
                    .on_group_control(peer_id, GroupInbound::DocRequest { group_id, doc_id });
            }
            DecodedFrame::Control(ControlMessage::ShareListRequest) => {
                self.events
                    .on_share_control(peer_id, ShareInbound::ListRequest);
            }
            DecodedFrame::Control(ControlMessage::ShareListResponse { shares }) => {
                self.events
                    .on_share_control(peer_id, ShareInbound::ListResponse { shares });
            }
            DecodedFrame::Control(ControlMessage::ShareDownloadRequest { share_id }) => {
                self.events
                    .on_share_control(peer_id, ShareInbound::DownloadRequest { share_id });
            }
            DecodedFrame::FileChunk {
                transfer_id,
                offset,
                data,
            } => {
                let id = transfer_id.to_string();
                if let Some(tx) = self.inbound_sender(peer_id, &id) {
                    let _ = tx.send(InboundOp::Chunk { offset, data }).await;
                }
            }
        }
    }

    fn inbound_sender(self: &Arc<Self>, peer_id: &str, transfer_id: &str) -> Option<mpsc::Sender<InboundOp>> {
        if self.finished.contains_key(transfer_id) {
            return None;
        }
        let entry = self
            .inbound
            .entry(transfer_id.to_string())
            .or_insert_with(|| self.spawn_inbound_writer(peer_id.to_string(), transfer_id.to_string()));
        Some(entry.tx.clone())
    }

    fn spawn_inbound_writer(self: &Arc<Self>, peer_id: String, transfer_id: String) -> InboundHandle {
        let (tx, mut rx) = mpsc::channel::<InboundOp>(64);
        let this = Arc::clone(self);
        let task_peer = peer_id.clone();
        tokio::spawn(async move {
            let peer_dir = this.downloads_dir.join(sanitize_component(&task_peer));
            if tokio::fs::create_dir_all(&peer_dir).await.is_err() {
                return;
            }
            let part_path = peer_dir.join(format!(".incoming-{transfer_id}.part"));
            let mut file = match File::create(&part_path).await {
                Ok(file) => file,
                Err(_) => return,
            };

            let mut cur_pos: u64 = 0;
            let mut written: u64 = 0;
            let mut name: Option<String> = None;
            let mut is_dir = false;
            let mut display_total: u64 = 0;
            let mut complete_total: Option<u64> = None;
            let mut group_id = String::new();
            let mut doc_id = String::new();
            let mut share_id = String::new();
            let mut last_emit = Instant::now();

            while let Some(op) = rx.recv().await {
                match op {
                    InboundOp::Offer {
                        name: offered_name,
                        is_dir: offered_is_dir,
                        display_total: offered_total,
                        group_id: offered_group,
                        doc_id: offered_doc,
                        share_id: offered_share,
                    } => {
                        name = Some(offered_name);
                        is_dir = offered_is_dir;
                        display_total = offered_total;
                        group_id = offered_group;
                        doc_id = offered_doc;
                        share_id = offered_share;
                        let nm = name.clone().unwrap_or_default();
                        this.emit_inbound(&peer_dir, &transfer_id, &task_peer, &nm, display_total, written, "active");
                    }
                    InboundOp::Chunk { offset, data } => {
                        if cur_pos != offset {
                            if file.seek(std::io::SeekFrom::Start(offset)).await.is_err() {
                                break;
                            }
                            cur_pos = offset;
                        }
                        if file.write_all(&data).await.is_err() {
                            break;
                        }
                        let len = data.len() as u64;
                        cur_pos += len;
                        written += len;
                        if last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
                            last_emit = Instant::now();
                            let nm = name.clone().unwrap_or_else(|| transfer_id.clone());
                            this.emit_inbound(&peer_dir, &transfer_id, &task_peer, &nm, display_total, written, "active");
                        }
                    }
                    InboundOp::Complete { total } => {
                        complete_total = Some(total);
                    }
                }

                if name.is_some() && complete_total.map(|t| written >= t).unwrap_or(false) {
                    let _ = file.flush().await;
                    drop(file);
                    this.finalize_inbound(
                        &peer_dir,
                        &part_path,
                        &transfer_id,
                        &task_peer,
                        name.clone().unwrap_or_default(),
                        is_dir,
                        written,
                        &group_id,
                        &doc_id,
                        &share_id,
                    )
                    .await;
                    this.inbound.remove(&transfer_id);
                    this.finished.insert(transfer_id.clone(), ());
                    return;
                }
            }

            drop(file);
            let _ = tokio::fs::remove_file(&part_path).await;
            if !this.finished.contains_key(&transfer_id) {
                this.inbound.remove(&transfer_id);
                let nm = name.unwrap_or_else(|| transfer_id.clone());
                this.events.on_transfer_update(TransferUpdate {
                    transfer_id: transfer_id.clone(),
                    peer_id: task_peer.clone(),
                    direction: "in".to_string(),
                    name: nm,
                    path: None,
                    size: i64::try_from(display_total).unwrap_or(0),
                    transferred: i64::try_from(written).unwrap_or(0),
                    status: "failed".to_string(),
                });
            }
        });
        InboundHandle { peer_id, tx }
    }

    fn emit_inbound(
        &self,
        peer_dir: &Path,
        transfer_id: &str,
        peer_id: &str,
        name: &str,
        size: u64,
        written: u64,
        status: &str,
    ) {
        self.events.on_transfer_update(TransferUpdate {
            transfer_id: transfer_id.to_string(),
            peer_id: peer_id.to_string(),
            direction: "in".to_string(),
            name: name.to_string(),
            path: Some(peer_dir.join(name).to_string_lossy().to_string()),
            size: i64::try_from(size).unwrap_or(0),
            transferred: i64::try_from(written).unwrap_or(0),
            status: status.to_string(),
        });
    }

    #[allow(clippy::too_many_arguments)]
    async fn finalize_inbound(
        &self,
        peer_dir: &Path,
        part_path: &Path,
        transfer_id: &str,
        peer_id: &str,
        name: String,
        is_dir: bool,
        written: u64,
        group_id: &str,
        doc_id: &str,
        share_id: &str,
    ) {
        let mut status = "completed".to_string();
        let final_path: PathBuf;

        if is_dir {
            let folder_name = if name.is_empty() {
                "folder".to_string()
            } else {
                name.clone()
            };
            let staging = peer_dir.join(format!(".stage-{transfer_id}"));
            let _ = tokio::fs::remove_dir_all(&staging).await;
            let target = unique_path(peer_dir, &folder_name);
            match extract_store_tar(part_path.to_path_buf(), staging.clone()).await {
                Ok(()) => {
                    let _ = tokio::fs::remove_file(part_path).await;
                    let extracted_root = staging.join(&folder_name);
                    let source_dir = if extracted_root.is_dir() {
                        extracted_root
                    } else {
                        staging.clone()
                    };
                    if tokio::fs::rename(&source_dir, &target).await.is_err() {
                        status = "failed".to_string();
                    }
                    let _ = tokio::fs::remove_dir_all(&staging).await;
                }
                Err(err) => {
                    tracing::debug!(error = %err, "lan store tar extract failed");
                    let _ = tokio::fs::remove_dir_all(&staging).await;
                    status = "failed".to_string();
                }
            }
            final_path = target;
        } else {
            let file_name = if name.is_empty() {
                "file".to_string()
            } else {
                name.clone()
            };
            let target = unique_path(peer_dir, &file_name);
            if tokio::fs::rename(part_path, &target).await.is_err() {
                status = "failed".to_string();
            }
            final_path = target;
        }

        self.events.on_transfer_update(TransferUpdate {
            transfer_id: transfer_id.to_string(),
            peer_id: peer_id.to_string(),
            direction: "in".to_string(),
            name: name.clone(),
            path: Some(final_path.to_string_lossy().to_string()),
            size: i64::try_from(written).unwrap_or(0),
            transferred: i64::try_from(written).unwrap_or(0),
            status: status.clone(),
        });

        if status == "completed" {
            if !group_id.is_empty() && !doc_id.is_empty() {
                self.events.on_group_doc_received(GroupDocReceived {
                    group_id: group_id.to_string(),
                    doc_id: doc_id.to_string(),
                    peer_id: peer_id.to_string(),
                    path: final_path.clone(),
                    is_dir,
                    size: i64::try_from(written).unwrap_or(0),
                });
            } else if !share_id.is_empty() {
                self.events.on_share_received(ShareReceived {
                    share_id: share_id.to_string(),
                    peer_id: peer_id.to_string(),
                    path: final_path.clone(),
                    is_dir,
                    size: i64::try_from(written).unwrap_or(0),
                    name: name.clone(),
                });
            } else {
                let body = final_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or(name);
                self.events.on_incoming_chat(
                    peer_id,
                    &format!("file-{transfer_id}"),
                    now_ms(),
                    &format!("__lan_file__:{body}:{}", final_path.to_string_lossy()),
                );
            }
        }
    }

    async fn fail_inbound_for_peer(&self, peer_id: &str) {
        let ids: Vec<String> = self
            .inbound
            .iter()
            .filter(|entry| entry.value().peer_id == peer_id)
            .map(|entry| entry.key().clone())
            .collect();
        for id in ids {
            self.inbound.remove(&id);
        }
    }

    pub async fn send_text(
        self: &Arc<Self>,
        peer_id: &str,
        msg_id: &str,
        ts_ms: i64,
        body: &str,
    ) -> Result<()> {
        let link = self.ensure_link(peer_id).await?;
        let frame = encode_control(&ControlMessage::Chat {
            id: msg_id.to_string(),
            ts_ms,
            body: body.to_string(),
        })?;
        if let Err(err) = link.send_control(frame).await {
            self.links.remove(peer_id);
            return Err(err);
        }
        Ok(())
    }

    pub async fn send_path(
        self: &Arc<Self>,
        peer_id: &str,
        transfer_id: &str,
        source: &Path,
    ) -> Result<()> {
        self.send_path_inner(peer_id, transfer_id, source, "", "", "")
            .await
    }

    pub async fn send_group_doc(
        self: &Arc<Self>,
        peer_id: &str,
        transfer_id: &str,
        group_id: &str,
        doc_id: &str,
        source: &Path,
    ) -> Result<()> {
        self.send_path_inner(peer_id, transfer_id, source, group_id, doc_id, "")
            .await
    }

    pub async fn send_share(
        self: &Arc<Self>,
        peer_id: &str,
        transfer_id: &str,
        share_id: &str,
        source: &Path,
    ) -> Result<()> {
        self.send_path_inner(peer_id, transfer_id, source, "", "", share_id)
            .await
    }

    pub async fn send_control_message(
        self: &Arc<Self>,
        peer_id: &str,
        message: &ControlMessage,
    ) -> Result<()> {
        let link = self.ensure_link(peer_id).await?;
        let frame = encode_control(message)?;
        if let Err(err) = link.send_control(frame).await {
            self.links.remove(peer_id);
            return Err(err);
        }
        Ok(())
    }

    async fn send_path_inner(
        self: &Arc<Self>,
        peer_id: &str,
        transfer_id: &str,
        source: &Path,
        group_id: &str,
        doc_id: &str,
        share_id: &str,
    ) -> Result<()> {
        let link = self.ensure_link(peer_id).await?;
        let transfer_uuid =
            uuid::Uuid::parse_str(transfer_id).unwrap_or_else(|_| uuid::Uuid::new_v4());

        let metadata = tokio::fs::metadata(source)
            .await
            .with_context(|| format!("reading metadata for {}", source.display()))?;

        let result = if metadata.is_dir() {
            let name = source
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "folder".to_string());
            self.stream_dir(
                &link, peer_id, &transfer_uuid, source, &name, group_id, doc_id, share_id,
            )
            .await
        } else {
            let name = source
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".to_string());
            self.stream_file(
                &link,
                peer_id,
                &transfer_uuid,
                source,
                &name,
                metadata.len(),
                group_id,
                doc_id,
                share_id,
            )
            .await
        };

        if result.is_err() {
            self.links.remove(peer_id);
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn stream_file(
        self: &Arc<Self>,
        link: &Arc<PeerLink>,
        peer_id: &str,
        transfer_uuid: &uuid::Uuid,
        path: &Path,
        name: &str,
        total: u64,
        group_id: &str,
        doc_id: &str,
        share_id: &str,
    ) -> Result<()> {
        let transfer_id = transfer_uuid.to_string();
        link.send_control(encode_control(&ControlMessage::FileOffer {
            transfer_id: transfer_id.clone(),
            name: name.to_string(),
            is_dir: false,
            total_size: total,
            group_id: group_id.to_string(),
            doc_id: doc_id.to_string(),
            share_id: share_id.to_string(),
        })?)
        .await?;
        self.emit_out(&transfer_id, peer_id, name, Some(path), total, 0, "active");

        let mut file = File::open(path)
            .await
            .with_context(|| format!("opening {} for send", path.display()))?;
        let mut offset: u64 = 0;
        let mut buf = vec![0u8; self.chunk_size];
        let mut last_emit = Instant::now();
        loop {
            let read = read_full(&mut file, &mut buf).await?;
            if read == 0 {
                break;
            }
            link.send_chunk(encode_file_chunk(transfer_uuid, offset, &buf[..read]))
                .await?;
            offset += read as u64;
            if last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
                last_emit = Instant::now();
                self.emit_out(&transfer_id, peer_id, name, Some(path), total, offset, "active");
            }
        }

        link.send_control(encode_control(&ControlMessage::FileComplete {
            transfer_id: transfer_id.clone(),
            total_size: offset,
        })?)
        .await?;
        self.emit_out(&transfer_id, peer_id, name, Some(path), total, offset, "completed");
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn stream_dir(
        self: &Arc<Self>,
        link: &Arc<PeerLink>,
        peer_id: &str,
        transfer_uuid: &uuid::Uuid,
        source: &Path,
        name: &str,
        group_id: &str,
        doc_id: &str,
        share_id: &str,
    ) -> Result<()> {
        let transfer_id = transfer_uuid.to_string();
        let walk_source = source.to_path_buf();
        let display_total = tokio::task::spawn_blocking(move || dir_total_size(&walk_source))
            .await
            .unwrap_or(0);

        link.send_control(encode_control(&ControlMessage::FileOffer {
            transfer_id: transfer_id.clone(),
            name: name.to_string(),
            is_dir: true,
            total_size: display_total,
            group_id: group_id.to_string(),
            doc_id: doc_id.to_string(),
            share_id: share_id.to_string(),
        })?)
        .await?;
        self.emit_out(&transfer_id, peer_id, name, Some(source), display_total, 0, "active");

        let mut rx = spawn_tar_stream(source.to_path_buf(), name.to_string());
        let mut carry: Vec<u8> = Vec::with_capacity(self.chunk_size * 2);
        let mut offset: u64 = 0;
        let mut last_emit = Instant::now();
        while let Some(bytes) = rx.recv().await {
            carry.extend_from_slice(&bytes);
            while carry.len() >= self.chunk_size {
                let chunk: Vec<u8> = carry.drain(..self.chunk_size).collect();
                let len = chunk.len() as u64;
                link.send_chunk(encode_file_chunk(transfer_uuid, offset, &chunk))
                    .await?;
                offset += len;
                if last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
                    last_emit = Instant::now();
                    self.emit_out(&transfer_id, peer_id, name, Some(source), display_total, offset, "active");
                }
            }
        }
        if !carry.is_empty() {
            let len = carry.len() as u64;
            link.send_chunk(encode_file_chunk(transfer_uuid, offset, &carry))
                .await?;
            offset += len;
        }

        link.send_control(encode_control(&ControlMessage::FileComplete {
            transfer_id: transfer_id.clone(),
            total_size: offset,
        })?)
        .await?;
        self.emit_out(&transfer_id, peer_id, name, Some(source), offset, offset, "completed");
        Ok(())
    }

    fn emit_out(
        &self,
        transfer_id: &str,
        peer_id: &str,
        name: &str,
        path: Option<&Path>,
        size: u64,
        transferred: u64,
        status: &str,
    ) {
        self.events.on_transfer_update(TransferUpdate {
            transfer_id: transfer_id.to_string(),
            peer_id: peer_id.to_string(),
            direction: "out".to_string(),
            name: name.to_string(),
            path: path.map(|p| p.to_string_lossy().to_string()),
            size: i64::try_from(size).unwrap_or(0),
            transferred: i64::try_from(transferred).unwrap_or(0),
            status: status.to_string(),
        });
    }

    fn build_hello(&self) -> Hello {
        Hello {
            user_id: self.identity.user_id().to_string(),
            nickname: self.identity.nickname(),
            public_key: self.identity.public_b64(),
            protocol: super::discovery::LAN_PROTOCOL.to_string(),
        }
    }

    fn register_peer_seen(&self, hello: &Hello) {
        if let Some(record) = self.registry.get(&hello.user_id) {
            let mut updated = record;
            updated.nickname = hello.nickname.clone();
            updated.public_key = hello.public_key.clone();
            self.registry.upsert(updated);
        }
    }
}

fn spawn_stream_writer(
    mut write_half: OwnedWriteHalf,
    mut rx: mpsc::Receiver<Vec<u8>>,
    cipher: SessionCipher,
) {
    tokio::spawn(async move {
        while let Some(plain) = rx.recv().await {
            let frame = cipher.seal_with_len_prefix(&plain);
            if write_half.write_all(&frame).await.is_err() {
                break;
            }
            if write_half.flush().await.is_err() {
                break;
            }
        }
    });
}

async fn read_full<R>(reader: &mut R, buf: &mut [u8]) -> Result<usize>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut filled = 0;
    while filled < buf.len() {
        let n = reader.read(&mut buf[filled..]).await?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

fn spawn_tar_stream(source: PathBuf, dir_name: String) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel::<Vec<u8>>(8);
    tokio::task::spawn_blocking(move || {
        struct Bridge {
            tx: mpsc::Sender<Vec<u8>>,
        }
        impl std::io::Write for Bridge {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.tx
                    .blocking_send(buf.to_vec())
                    .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "lan tar receiver dropped"))?;
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let writer = std::io::BufWriter::with_capacity(1 << 20, Bridge { tx });
        let mut builder = tar::Builder::new(writer);
        builder.follow_symlinks(false);
        if let Err(err) = builder.append_dir_all(&dir_name, &source) {
            tracing::debug!(error = %err, "lan tar pack failed");
            return;
        }
        match builder.into_inner() {
            Ok(mut buffered) => {
                use std::io::Write;
                let _ = buffered.flush();
            }
            Err(err) => {
                tracing::debug!(error = %err, "lan tar finish failed");
            }
        }
    });
    rx
}

async fn extract_store_tar(archive: PathBuf, dest: PathBuf) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        std::fs::create_dir_all(&dest)?;
        let file = std::fs::File::open(&archive)?;
        let reader = std::io::BufReader::with_capacity(1 << 20, file);
        let mut archive = tar::Archive::new(reader);
        archive.unpack(&dest)?;
        Ok(())
    })
    .await
    .map_err(|e| anyhow!("extract task failed: {e}"))??;
    Ok(())
}

fn dir_total_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(read_dir) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in read_dir.flatten() {
        let child = entry.path();
        match entry.metadata() {
            Ok(meta) if meta.is_dir() => total += dir_total_size(&child),
            Ok(meta) => total += meta.len(),
            Err(_) => {}
        }
    }
    total
}

fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let safe = sanitize_file_name(name);
    let candidate = dir.join(&safe);
    if !candidate.exists() {
        return candidate;
    }
    let stem = Path::new(&safe)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let ext = Path::new(&safe)
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

fn sanitize_file_name(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let cleaned: String = base
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    if cleaned.trim().is_empty() {
        "file".to_string()
    } else {
        cleaned
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
