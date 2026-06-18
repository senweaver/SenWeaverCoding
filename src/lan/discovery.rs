// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use anyhow::{Context, Result};
use dashmap::DashMap;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;

pub const LAN_PROTOCOL: &str = "senweaver-lan-v1";

#[derive(Debug, Clone)]
pub struct PeerRecord {
    pub user_id: String,
    pub nickname: String,
    pub public_key: String,
    pub addr: SocketAddr,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerView {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub nickname: String,
    #[serde(rename = "publicKey")]
    pub public_key: String,
    pub ip: String,
    pub port: u16,
    pub online: bool,
}

#[derive(Default)]
pub struct PeerRegistry {
    peers: DashMap<String, PeerRecord>,
}

impl PeerRegistry {
    pub fn new() -> Self {
        Self {
            peers: DashMap::new(),
        }
    }

    pub fn upsert(&self, record: PeerRecord) -> bool {
        let changed = match self.peers.get(&record.user_id) {
            Some(existing) => {
                existing.addr != record.addr
                    || existing.nickname != record.nickname
                    || existing.public_key != record.public_key
            }
            None => true,
        };
        self.peers.insert(record.user_id.clone(), record);
        changed
    }

    pub fn remove(&self, user_id: &str) -> bool {
        self.peers.remove(user_id).is_some()
    }

    pub fn get(&self, user_id: &str) -> Option<PeerRecord> {
        self.peers.get(user_id).map(|r| r.clone())
    }

    pub fn snapshot(&self) -> Vec<PeerView> {
        let mut views: Vec<PeerView> = self
            .peers
            .iter()
            .map(|entry| {
                let r = entry.value();
                PeerView {
                    user_id: r.user_id.clone(),
                    nickname: r.nickname.clone(),
                    public_key: r.public_key.clone(),
                    ip: r.addr.ip().to_string(),
                    port: r.addr.port(),
                    online: true,
                }
            })
            .collect();
        views.sort_by(|a, b| a.nickname.to_lowercase().cmp(&b.nickname.to_lowercase()));
        views
    }

    pub fn clear(&self) {
        self.peers.clear();
    }
}

pub struct Discovery {
    daemon: ServiceDaemon,
}

pub struct DiscoveryParams {
    pub service_type: String,
    pub user_id: String,
    pub nickname: String,
    pub public_key: String,
    pub port: u16,
}

impl Discovery {
    pub fn start(
        params: DiscoveryParams,
        registry: Arc<PeerRegistry>,
        on_change: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let daemon = ServiceDaemon::new().context("creating mdns daemon")?;

        let mut properties: HashMap<String, String> = HashMap::new();
        properties.insert("uid".to_string(), params.user_id.clone());
        properties.insert("nick".to_string(), params.nickname.clone());
        properties.insert("pk".to_string(), params.public_key.clone());
        properties.insert("proto".to_string(), LAN_PROTOCOL.to_string());

        let instance = sanitize_instance(&params.user_id);
        let host_name = format!("{instance}.local.");
        let service = ServiceInfo::new(
            &params.service_type,
            &instance,
            &host_name,
            "",
            params.port,
            properties,
        )
        .context("building mdns service info")?
        .enable_addr_auto();

        daemon.register(service).context("registering mdns service")?;

        let receiver = daemon
            .browse(&params.service_type)
            .context("starting mdns browse")?;

        let self_user_id = params.user_id.clone();
        let service_type = params.service_type.clone();
        std::thread::Builder::new()
            .name("lan-mdns-browse".to_string())
            .spawn(move || {
                while let Ok(event) = receiver.recv() {
                    match event {
                        ServiceEvent::ServiceResolved(info) => {
                            if let Some(record) = resolve_peer(&info, &self_user_id) {
                                if registry.upsert(record) {
                                    on_change();
                                }
                            }
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => {
                            if let Some(uid) = user_id_from_fullname(&fullname, &service_type) {
                                if uid != self_user_id && registry.remove(&uid) {
                                    on_change();
                                }
                            }
                        }
                        _ => {}
                    }
                }
            })
            .context("spawning mdns browse thread")?;

        Ok(Self { daemon })
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}

fn resolve_peer(info: &ServiceInfo, self_user_id: &str) -> Option<PeerRecord> {
    let uid = info.get_property_val_str("uid")?.to_string();
    if uid == self_user_id {
        return None;
    }
    let nickname = info
        .get_property_val_str("nick")
        .map(str::to_string)
        .unwrap_or_else(|| uid.clone());
    let public_key = info.get_property_val_str("pk")?.to_string();
    let port = info.get_port();
    let ip = pick_address(info)?;
    Some(PeerRecord {
        user_id: uid,
        nickname,
        public_key,
        addr: SocketAddr::new(ip, port),
    })
}

fn pick_address(info: &ServiceInfo) -> Option<IpAddr> {
    let mut fallback: Option<IpAddr> = None;
    for addr in info.get_addresses() {
        match addr {
            IpAddr::V4(v4) => {
                if !v4.is_loopback() && !v4.is_unspecified() {
                    return Some(IpAddr::V4(*v4));
                }
            }
            IpAddr::V6(_) => {
                if fallback.is_none() {
                    fallback = Some(*addr);
                }
            }
        }
    }
    fallback
}

fn sanitize_instance(user_id: &str) -> String {
    user_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect()
}

fn user_id_from_fullname(fullname: &str, service_type: &str) -> Option<String> {
    let suffix = format!(".{service_type}");
    let instance = fullname.strip_suffix(&suffix).or_else(|| {
        fullname.strip_suffix(service_type).map(|s| s.trim_end_matches('.'))
    })?;
    Some(instance.to_string())
}
