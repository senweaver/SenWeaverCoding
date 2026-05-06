// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
//! `LspManager` — reconciler between the persisted `LspConfig` and the
//! running `services::lsp::LspService` fleet.
//!
//! Lifecycle:
//!
//! 1. The gateway constructs a manager during `run_gateway`, pointing
//!    at the workspace dir + the broadcast channel that
//!    `ws_desktop` will replay to UI clients.
//! 2. Whenever a desktop route mutates the LspConfig section (CRUD,
//!    install completion, toggle), it calls `manager.reconcile()` so
//!    servers are spawned/stopped to match.
//! 3. Each running server forwards `publishDiagnostics` notifications
//!    through a [`crate::lsp::events::LspBroadcast`] event so the
//!    desktop editor sees real-time errors / warnings.
//!
//! All reconciliation is intentionally idempotent: calling
//! `reconcile()` repeatedly with the same config yields the same active
//! set without restarting healthy processes.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use parking_lot::RwLock;
use tokio::sync::Mutex;

use crate::config::schema::{Config, LspInstallState, LspServerEntry};
use crate::lsp::events::{
    InstallPhase, LspBroadcast, LspBroadcastEvent, ServerLifecycleStatus,
};
use crate::lsp::installer::{self, InstallProgress, InstallReport};
use crate::services::lsp::{LspService, LspServerConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryFingerprint {

    language_id: String,
    enabled: bool,
    managed: bool,
    command: Option<String>,
    args: Vec<String>,
    env_keys: Vec<String>,
    file_extensions: Vec<String>,
    install_state: String,
}

impl EntryFingerprint {
    fn from_entry(entry: &LspServerEntry) -> Self {
        let mut env_keys: Vec<String> = entry.env.keys().cloned().collect();
        env_keys.sort();
        Self {
            language_id: entry.language_id.clone(),
            enabled: entry.enabled,
            managed: entry.managed,
            command: entry.command.clone(),
            args: entry.args.clone(),
            env_keys,
            file_extensions: entry.file_extensions.clone(),
            install_state: serde_json::to_string(&entry.install_state).unwrap_or_default(),
        }
    }
}

pub struct LspManager {
    service: LspService,
    workspace_root: Arc<RwLock<PathBuf>>,
    broadcast: LspBroadcast,
    inner: Arc<Mutex<ManagerInner>>,

    #[allow(dead_code)]
    diagnostics_listener: Arc<DiagnosticsForwarder>,
}

#[derive(Default)]
struct ManagerInner {

    seen: HashMap<String, EntryFingerprint>,
}

impl LspManager {

    pub async fn new(
        service: LspService,
        workspace_root: PathBuf,
        broadcast: LspBroadcast,
    ) -> Self {
        let workspace_root = Arc::new(RwLock::new(workspace_root));
        let listener = Arc::new(DiagnosticsForwarder {
            broadcast: broadcast.clone(),
            servers: Arc::new(RwLock::new(HashMap::new())),
        });

        #[cfg(feature = "lsp-push-diagnostics")]
        {
            let dyn_listener: Arc<dyn crate::services::lsp::DiagnosticsListener> = listener.clone();
            service.register_diagnostics_listener(dyn_listener).await;
        }

        Self {
            service,
            workspace_root,
            broadcast,
            inner: Arc::new(Mutex::new(ManagerInner::default())),
            diagnostics_listener: listener,
        }
    }

    pub fn set_workspace_root(&self, root: PathBuf) {
        *self.workspace_root.write() = root;
    }

    pub fn broadcast(&self) -> LspBroadcast {
        self.broadcast.clone()
    }

    pub fn service(&self) -> &LspService {
        &self.service
    }

    pub async fn reconcile(&self, config: &Config) {
        let lsp = &config.lsp;
        let mut inner = self.inner.lock().await;

        let mut next_seen: HashMap<String, EntryFingerprint> = HashMap::new();
        let prev_seen: HashSet<String> = inner.seen.keys().cloned().collect();
        let workspace_root = self.workspace_root.read().clone();

        let mut live_mapping: HashMap<String, (String, String)> = HashMap::new();

        for entry in &lsp.servers {
            let id = entry.id.trim().to_string();
            if id.is_empty() {
                continue;
            }
            let fp = EntryFingerprint::from_entry(entry);
            next_seen.insert(id.clone(), fp.clone());

            let want_running = lsp.enabled && entry.enabled && entry.resolved_command().is_some();
            let prev_fp = inner.seen.get(&id).cloned();
            let unchanged = prev_fp.as_ref().is_some_and(|p| *p == fp);

            if !want_running {

                if prev_fp.is_some() {
                    self.stop_entry(entry, &workspace_root).await;
                    self.broadcast.send(LspBroadcastEvent::LspServerStatus {
                        server_id: id.clone(),
                        language_id: entry.language_id.clone(),
                        status: ServerLifecycleStatus::Stopped,
                        reason: None,
                    });
                }
                continue;
            }

            if unchanged {

                if let (Some(language_id), Some(_)) =
                    (Some(entry.language_id.clone()), entry.resolved_command())
                {
                    live_mapping.insert(
                        canonical_uri_prefix(&workspace_root),
                        (id.clone(), language_id),
                    );
                }
                continue;
            }

            if prev_fp.is_some() {
                self.service
                    .shutdown_server(&entry.language_id, &workspace_root)
                    .await;
            }
            self.broadcast.send(LspBroadcastEvent::LspServerStatus {
                server_id: id.clone(),
                language_id: entry.language_id.clone(),
                status: ServerLifecycleStatus::Starting,
                reason: None,
            });

            match self.start_entry(entry, &workspace_root).await {
                Ok(()) => {
                    live_mapping.insert(
                        canonical_uri_prefix(&workspace_root),
                        (id.clone(), entry.language_id.clone()),
                    );
                    self.broadcast.send(LspBroadcastEvent::LspServerStatus {
                        server_id: id.clone(),
                        language_id: entry.language_id.clone(),
                        status: ServerLifecycleStatus::Ready,
                        reason: None,
                    });
                }
                Err(err) => {
                    let reason = format!("{err:#}");
                    tracing::warn!(server_id = %id, error = %reason, "LSP server start failed");
                    self.broadcast.send(LspBroadcastEvent::LspServerStatus {
                        server_id: id.clone(),
                        language_id: entry.language_id.clone(),
                        status: ServerLifecycleStatus::Failed,
                        reason: Some(reason),
                    });
                }
            }
        }

        let next_keys: HashSet<String> = next_seen.keys().cloned().collect();
        for stale_id in prev_seen.difference(&next_keys) {
            let prev_fp = inner.seen.get(stale_id).cloned();
            if let Some(fp) = prev_fp.as_ref() {
                if !fp.language_id.is_empty() {
                    tracing::debug!(
                        target: "lsp.manager",
                        server_id = %stale_id,
                        language = %fp.language_id,
                        "LSP entry removed from config; stopping",
                    );
                    self.service
                        .shutdown_server(&fp.language_id, &workspace_root)
                        .await;
                }
            }
            self.broadcast.send(LspBroadcastEvent::LspServerStatus {
                server_id: stale_id.clone(),
                language_id: prev_fp
                    .as_ref()
                    .map(|f| f.language_id.clone())
                    .unwrap_or_default(),
                status: ServerLifecycleStatus::Stopped,
                reason: Some("removed from config".into()),
            });
        }

        inner.seen = next_seen;
        self.diagnostics_listener.replace_mapping(live_mapping);
    }

    async fn start_entry(&self, entry: &LspServerEntry, workspace_root: &PathBuf) -> Result<()> {
        let cmd = entry
            .resolved_command()
            .ok_or_else(|| anyhow!("server `{}` has no command configured", entry.id))?;
        let server_config = LspServerConfig {
            language_id: entry.language_id.clone(),
            server_command: cmd.to_string(),
            server_args: entry.args.clone(),
            root_path: workspace_root.clone(),
            initialization_options: entry.initialization_options.clone(),
        };
        self.service.register_server(server_config).await;

        self.service
            .ensure_server_started(&entry.language_id, workspace_root)
            .await
    }

    async fn stop_entry(&self, entry: &LspServerEntry, workspace_root: &PathBuf) {
        self.service
            .shutdown_server(&entry.language_id, workspace_root)
            .await;
    }

    pub async fn install(
        self: Arc<Self>,
        config_lock: Arc<parking_lot::Mutex<Config>>,
        live: crate::config::live::LiveConfig,
        server_id: String,
    ) -> Result<InstallReport> {

        {
            let mut cfg = config_lock.lock();
            if let Some(entry) = cfg.lsp.servers.iter_mut().find(|s| s.id == server_id) {
                entry.install_state = LspInstallState::Installing;
            }
        }
        let snapshot_for_install = config_lock.lock().clone();
        if let Err(e) = snapshot_for_install.save().await {
            tracing::warn!(error = %e, "save config (lsp install: enter Installing)");
        }
        live.store(snapshot_for_install);

        let id_for_progress = server_id.clone();
        let broadcast = self.broadcast.clone();
        let progress: InstallProgress = Arc::new(move |phase: InstallPhase| {
            broadcast.send(LspBroadcastEvent::LspInstallProgress {
                server_id: id_for_progress.clone(),
                phase,
            });
        });

        let result = installer::install(&server_id, progress).await;

        match result {
            Ok(report) => {
                {
                    let mut cfg = config_lock.lock();
                    if let Some(entry) = cfg.lsp.servers.iter_mut().find(|s| s.id == server_id) {
                        entry.install_state = LspInstallState::from(&report);
                        entry.command = Some(report.binary_path.to_string_lossy().to_string());
                        if !report.default_args.is_empty() && entry.args.is_empty() {
                            entry.args = report.default_args.clone();
                        }
                    }
                }
                let snapshot = config_lock.lock().clone();
                if let Err(e) = snapshot.save().await {
                    tracing::warn!(error = %e, "save config (lsp install: success)");
                }
                live.store(snapshot.clone());
                self.reconcile(&snapshot).await;
                Ok(report)
            }
            Err(err) => {
                let reason = format!("{err:#}");
                {
                    let mut cfg = config_lock.lock();
                    if let Some(entry) = cfg.lsp.servers.iter_mut().find(|s| s.id == server_id) {
                        entry.install_state = LspInstallState::Failed {
                            reason: reason.clone(),
                        };
                    }
                }
                let snapshot = config_lock.lock().clone();
                if let Err(e) = snapshot.save().await {
                    tracing::warn!(error = %e, "save config (lsp install: failure)");
                }
                live.store(snapshot);
                self.broadcast.send(LspBroadcastEvent::LspInstallProgress {
                    server_id: server_id.clone(),
                    phase: InstallPhase::Failed { reason },
                });
                Err(err)
            }
        }
    }
}

fn canonical_uri_prefix(workspace_root: &PathBuf) -> String {
    crate::services::lsp::path_to_uri(workspace_root)
}

struct DiagnosticsForwarder {
    broadcast: LspBroadcast,

    servers: Arc<RwLock<HashMap<String, (String, String)>>>,
}

impl DiagnosticsForwarder {
    fn replace_mapping(&self, mapping: HashMap<String, (String, String)>) {
        *self.servers.write() = mapping;
    }

    fn lookup(&self, uri: &str) -> Option<(String, String)> {
        let map = self.servers.read();
        for (prefix, ids) in map.iter() {
            if uri.starts_with(prefix) {
                return Some(ids.clone());
            }
        }

        if let Some(path) = uri.strip_prefix("file://") {
            let p = std::path::Path::new(path);
            if let Some(language) = crate::services::lsp::detect_language(p) {
                return Some((language.to_string(), language.to_string()));
            }
        }
        None
    }
}

#[cfg(feature = "lsp-push-diagnostics")]
impl crate::services::lsp::DiagnosticsListener for DiagnosticsForwarder {
    fn on_diagnostics(&self, uri: &str, diagnostics: &[serde_json::Value]) {
        let (server_id, _language_id) = self.lookup(uri).unwrap_or_else(|| {
            (
                "unknown".to_string(),
                "unknown".to_string(),
            )
        });
        self.broadcast.send(LspBroadcastEvent::LspDiagnostics {
            server_id,
            uri: uri.to_string(),
            version: None,
            diagnostics: serde_json::Value::Array(diagnostics.to_vec()),
        });
    }
}
