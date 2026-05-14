// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::json;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::config::Config;
use crate::services::mcp_manager::{
    McpManager, McpServerStatus, McpToolDef as ManagerToolDef, McpTransport as ManagerTransport,
};

#[derive(Debug, Clone)]
struct ServerFingerprint {
    signature: u64,
    enabled: bool,
}

#[derive(Default)]
pub struct LiveMcpReconciler {
    inner: Mutex<HashMap<String, ServerFingerprint>>,
}

impl LiveMcpReconciler {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(HashMap::new()),
        })
    }

    pub fn seed_from_config(&self, cfg: &Config) {
        let mut guard = self.inner.lock();
        guard.clear();
        for server in &cfg.mcp.servers {
            guard.insert(
                server.name.clone(),
                ServerFingerprint {
                    signature: fingerprint_signature(server),
                    enabled: server.enabled && cfg.mcp.enabled,
                },
            );
        }
    }

    pub fn schedule_reconcile(
        self: &Arc<Self>,
        cfg: Arc<Config>,
        event_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
    ) {
        let me = Arc::clone(self);
        crate::runtime::task_manager::spawn_supervised(
            "gateway.mcp_live.reconcile",
            async move {
                me.reconcile(cfg, event_tx).await;
            },
        );
    }

    async fn reconcile(
        &self,
        cfg: Arc<Config>,
        event_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
    ) {
        let mcp_enabled_globally = cfg.mcp.enabled;
        let now: HashMap<String, (ServerFingerprint, crate::config::McpServerConfig)> = cfg
            .mcp
            .servers
            .iter()
            .map(|s| {
                let fp = ServerFingerprint {
                    signature: fingerprint_signature(s),
                    enabled: s.enabled && mcp_enabled_globally,
                };
                (s.name.clone(), (fp, s.clone()))
            })
            .collect();

        let previous = {
            let mut guard = self.inner.lock();
            let prev = std::mem::take(&mut *guard);
            for (name, (fp, _)) in &now {
                guard.insert(name.clone(), fp.clone());
            }
            prev
        };

        let mut added: Vec<crate::config::McpServerConfig> = Vec::new();
        let mut modified: Vec<crate::config::McpServerConfig> = Vec::new();
        let mut removed: Vec<String> = Vec::new();
        let mut toggled_off: Vec<String> = Vec::new();
        let mut toggled_on: Vec<crate::config::McpServerConfig> = Vec::new();

        for (name, (fp_new, server_cfg)) in &now {
            match previous.get(name) {
                None => {
                    if fp_new.enabled {
                        added.push(server_cfg.clone());
                    }
                }
                Some(fp_old) => {
                    if fp_old.signature != fp_new.signature {
                        if fp_new.enabled {
                            modified.push(server_cfg.clone());
                        } else if fp_old.enabled {
                            toggled_off.push(name.clone());
                        }
                    } else if fp_old.enabled != fp_new.enabled {
                        if fp_new.enabled {
                            toggled_on.push(server_cfg.clone());
                        } else {
                            toggled_off.push(name.clone());
                        }
                    }
                }
            }
        }
        for name in previous.keys() {
            if !now.contains_key(name) {
                removed.push(name.clone());
            }
        }

        let svc_mcp_opt: Option<McpManager> =
            crate::services::try_get_services().map(|svc| svc.mcp.clone());

        for name in &removed {
            if let Some(svc) = svc_mcp_opt.as_ref() {
                let _ = svc.remove_server(name).await;
            }
        }
        for name in &toggled_off {
            if let Some(svc) = svc_mcp_opt.as_ref() {
                svc.set_server_status(name, McpServerStatus::Disabled, None)
                    .await;
            }
        }

        for server in added.iter().chain(modified.iter()).chain(toggled_on.iter()) {
            try_reconnect_and_publish(server.clone(), svc_mcp_opt.clone()).await;
        }

        let payload = json!({
            "type": "system_notification",
            "subtype": "mcp_servers_updated",
            "data": {
                "added": added.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
                "modified": modified.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
                "removed": removed,
                "toggledOff": toggled_off,
                "toggledOn": toggled_on
                    .iter()
                    .map(|s| s.name.clone())
                    .collect::<Vec<_>>(),
                "mcpEnabled": mcp_enabled_globally,
                "serverCount": cfg.mcp.servers.len(),
            }
        });
        let touched = !added.is_empty()
            || !modified.is_empty()
            || !removed.is_empty()
            || !toggled_off.is_empty()
            || !toggled_on.is_empty();
        if touched {
            let _ = event_tx.send(payload);
        }
    }
}

async fn try_reconnect_and_publish(
    server_cfg: crate::config::McpServerConfig,
    svc_mcp_opt: Option<McpManager>,
) {
    let name = server_cfg.name.clone();
    if let Some(svc) = svc_mcp_opt.as_ref() {
        let transport = manager_transport_from_config(&server_cfg);
        svc.add_server(&name, transport).await;
        svc.set_server_status(&name, McpServerStatus::Connecting, None)
            .await;
    }
    match crate::tools::mcp_client::McpServer::connect(server_cfg.clone()).await {
        Ok(server) => {
            if let Some(svc) = svc_mcp_opt.as_ref() {
                let tools = server.tools().await;
                let manager_tools: Vec<ManagerToolDef> = tools
                    .into_iter()
                    .map(|t| ManagerToolDef {
                        name: t.name,
                        description: t.description,
                        input_schema: t.input_schema,
                        server_name: name.clone(),
                    })
                    .collect();
                svc.set_server_tools(&name, manager_tools).await;
                svc.set_server_status(&name, McpServerStatus::Connected, None)
                    .await;
            }
            tracing::info!(
                target: "gateway.mcp_live",
                server = %name,
                "live MCP reconcile: server connected"
            );
        }
        Err(err) => {
            if let Some(svc) = svc_mcp_opt.as_ref() {
                svc.set_server_status(
                    &name,
                    McpServerStatus::Error,
                    Some(format!("{err:#}")),
                )
                .await;
            }
            tracing::warn!(
                target: "gateway.mcp_live",
                server = %name,
                error = %err,
                "live MCP reconcile: connect attempt failed (status set to error; previous registry retained)"
            );
        }
    }
}

fn manager_transport_from_config(
    server_cfg: &crate::config::McpServerConfig,
) -> ManagerTransport {
    use crate::config::McpTransport as Cfg;
    match server_cfg.transport {
        Cfg::Stdio => ManagerTransport::Stdio {
            command: server_cfg.command.clone(),
            args: server_cfg.args.clone(),
            env: server_cfg.env.clone(),
        },
        Cfg::Sse => ManagerTransport::Sse {
            url: server_cfg.url.clone().unwrap_or_default(),
            headers: server_cfg.headers.clone(),
        },
        Cfg::Http => ManagerTransport::Streamable {
            url: server_cfg.url.clone().unwrap_or_default(),
            headers: server_cfg.headers.clone(),
        },
    }
}

fn transport_tag(t: &crate::config::McpTransport) -> &'static str {
    use crate::config::McpTransport as Cfg;
    match t {
        Cfg::Stdio => "stdio",
        Cfg::Http => "http",
        Cfg::Sse => "sse",
    }
}

fn fingerprint_signature(server: &crate::config::McpServerConfig) -> u64 {
    let mut hasher = DefaultHasher::new();
    server.name.hash(&mut hasher);
    transport_tag(&server.transport).hash(&mut hasher);
    server.url.hash(&mut hasher);
    server.command.hash(&mut hasher);
    for arg in &server.args {
        arg.hash(&mut hasher);
    }
    let mut env_pairs: Vec<(&String, &String)> = server.env.iter().collect();
    env_pairs.sort_unstable();
    for (k, v) in env_pairs {
        k.hash(&mut hasher);
        v.hash(&mut hasher);
    }
    let mut hdr_pairs: Vec<(&String, &String)> = server.headers.iter().collect();
    hdr_pairs.sort_unstable();
    for (k, v) in hdr_pairs {
        k.hash(&mut hasher);
        v.hash(&mut hasher);
    }
    server.tool_timeout_secs.hash(&mut hasher);
    server.enabled.hash(&mut hasher);
    hasher.finish()
}
