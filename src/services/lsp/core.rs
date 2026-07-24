// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const INIT_TIMEOUT: Duration = Duration::from_secs(60);
const INCOMING_CHANNEL_CAPACITY: usize = 256;

fn clamp_doc_version(version: i64) -> Option<i32> {
    if version > 0 && version <= i64::from(i32::MAX) {
        Some(version as i32)
    } else {
        None
    }
}

#[derive(Debug)]
enum LspRequestFailure {
    Timeout { method: String },
    Protocol { method: String, code: i64, message: String },
}

impl std::fmt::Display for LspRequestFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout { method } => write!(f, "LSP request `{method}` timed out"),
            Self::Protocol {
                method,
                code,
                message,
            } => write!(f, "LSP error for `{method}` ({code}): {message}"),
        }
    }
}

impl std::error::Error for LspRequestFailure {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspRange {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDiagnostic {
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub range: LspRange,
    pub source: Option<String>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: String,
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
    pub container: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    pub language_id: String,
    pub server_command: String,
    pub server_args: Vec<String>,
    pub root_path: PathBuf,
    pub initialization_options: Option<serde_json::Value>,
}

#[cfg(feature = "lsp-push-diagnostics")]
pub trait DiagnosticsListener: Send + Sync {

    fn on_diagnostics(&self, uri: &str, diagnostics: &[serde_json::Value]);
}

#[cfg(feature = "lsp-push-diagnostics")]
struct PushDiagnosticsCache {
    store: Arc<std::sync::RwLock<HashMap<PathBuf, Vec<LspDiagnostic>>>>,
}

#[cfg(feature = "lsp-push-diagnostics")]
impl DiagnosticsListener for PushDiagnosticsCache {
    fn on_diagnostics(&self, uri: &str, diagnostics: &[serde_json::Value]) {
        let path = PathBuf::from(uri_to_path_string(uri));
        let wrapped = json!({ "diagnostics": diagnostics });
        let parsed = parse_lsp_diagnostics(&wrapped);
        if let Ok(mut guard) = self.store.write() {
            if parsed.is_empty() {
                guard.remove(&path);
            } else {
                guard.insert(path, parsed);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerKey {
    pub language_id: String,
    pub workspace_root: PathBuf,
}

impl ServerKey {
    pub fn new(language_id: impl Into<String>, workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            language_id: language_id.into(),
            workspace_root: workspace_root.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub language_id: String,
    pub workspace_root: PathBuf,
    pub status: String,
    pub open_files: usize,
}

struct LspServerHandle {
    _process: Child,
    stdin: ChildStdin,
    incoming: tokio::sync::mpsc::Receiver<serde_json::Value>,
    reader_task: tokio::task::JoinHandle<()>,
    next_id: u64,
    initialized: bool,
    opened_files: HashSet<String>,
    doc_versions: HashMap<String, i32>,
    doc_texts: HashMap<String, String>,
    did_change_support: super::incremental::DidChangeSupport,

    #[cfg(feature = "lsp-push-diagnostics")]
    notification_listeners: Arc<std::sync::RwLock<Vec<Arc<dyn DiagnosticsListener>>>>,

    #[cfg(feature = "lsp-push-diagnostics")]
    notifications: Option<tokio::sync::mpsc::Receiver<serde_json::Value>>,
}

impl Drop for LspServerHandle {
    fn drop(&mut self) {
        self.reader_task.abort();
    }
}

impl LspServerHandle {
    fn next_doc_version(&mut self, uri: &str) -> i32 {
        let counter = self.doc_versions.entry(uri.to_string()).or_insert(1);
        *counter = counter.saturating_add(1);
        *counter
    }

    fn build_did_change_params(
        &self,
        uri: &str,
        version: i32,
        new_text: &str,
    ) -> serde_json::Value {
        if self.did_change_support == super::incremental::DidChangeSupport::Incremental {
            if let Some(old) = self.doc_texts.get(uri) {
                return super::incremental::build_incremental_change(uri, version, old, new_text);
            }
        }
        super::incremental::build_full_change(uri, version, new_text)
    }

    fn cache_doc_text(&mut self, uri: &str, text: &str) {
        self.doc_texts.insert(uri.to_string(), text.to_string());
    }
}

struct StdoutReader {
    incoming: tokio::sync::mpsc::Receiver<serde_json::Value>,
    task: tokio::task::JoinHandle<()>,
    #[cfg(feature = "lsp-push-diagnostics")]
    notifications: tokio::sync::mpsc::Receiver<serde_json::Value>,
}

fn spawn_stdout_reader(stdout: ChildStdout) -> StdoutReader {
    let (tx, rx) = tokio::sync::mpsc::channel(INCOMING_CHANNEL_CAPACITY);
    #[cfg(feature = "lsp-push-diagnostics")]
    let (notif_tx, notif_rx) =
        tokio::sync::mpsc::channel::<serde_json::Value>(INCOMING_CHANNEL_CAPACITY);
    let task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        loop {
            let msg = match read_message(&mut reader).await {
                Ok(msg) => msg,
                Err(_) => break,
            };
            if msg.get("id").is_some() {
                if tx.send(msg).await.is_err() {
                    break;
                }
                continue;
            }
            #[cfg(feature = "lsp-push-diagnostics")]
            if msg.get("method").and_then(|m| m.as_str())
                == Some("textDocument/publishDiagnostics")
            {
                if notif_tx.send(msg).await.is_err() {
                    break;
                }
            }
        }
    });
    StdoutReader {
        incoming: rx,
        task,
        #[cfg(feature = "lsp-push-diagnostics")]
        notifications: notif_rx,
    }
}

struct LspServiceInner {

    servers: HashMap<ServerKey, Arc<Mutex<LspServerHandle>>>,

    starting: HashMap<ServerKey, Arc<Mutex<()>>>,

    server_configs: HashMap<ServerKey, LspServerConfig>,

    diagnostics: HashMap<PathBuf, Vec<LspDiagnostic>>,

    #[cfg(feature = "lsp-push-diagnostics")]
    diagnostics_listeners: Arc<std::sync::RwLock<Vec<Arc<dyn DiagnosticsListener>>>>,

    #[cfg(feature = "lsp-push-diagnostics")]
    push_diagnostics: Arc<std::sync::RwLock<HashMap<PathBuf, Vec<LspDiagnostic>>>>,
}

#[derive(Clone)]
pub struct LspService {
    inner: Arc<Mutex<LspServiceInner>>,
}

impl LspService {
    pub fn new() -> Self {
        #[cfg(feature = "lsp-push-diagnostics")]
        let push_store: Arc<std::sync::RwLock<HashMap<PathBuf, Vec<LspDiagnostic>>>> =
            Arc::new(std::sync::RwLock::new(HashMap::new()));
        #[cfg(feature = "lsp-push-diagnostics")]
        let builtin_listener: Arc<dyn DiagnosticsListener> = Arc::new(PushDiagnosticsCache {
            store: Arc::clone(&push_store),
        });
        Self {
            inner: Arc::new(Mutex::new(LspServiceInner {
                servers: HashMap::new(),
                starting: HashMap::new(),
                server_configs: HashMap::new(),
                diagnostics: HashMap::new(),
                #[cfg(feature = "lsp-push-diagnostics")]
                diagnostics_listeners: Arc::new(std::sync::RwLock::new(vec![builtin_listener])),
                #[cfg(feature = "lsp-push-diagnostics")]
                push_diagnostics: push_store,
            })),
        }
    }

    pub async fn register_server(&self, config: LspServerConfig) {
        let mut inner = self.inner.lock().await;
        let key = ServerKey {
            language_id: config.language_id.clone(),
            workspace_root: config.root_path.clone(),
        };
        inner.server_configs.insert(key, config);
    }

    async fn existing_handle(&self, key: &ServerKey) -> Option<Arc<Mutex<LspServerHandle>>> {
        let inner = self.inner.lock().await;
        inner.servers.get(key).map(Arc::clone)
    }

    async fn server_handle(
        &self,
        key: &ServerKey,
        workspace_root: &Path,
    ) -> Result<Arc<Mutex<LspServerHandle>>> {
        if let Some(handle) = self.existing_handle(key).await {
            return Ok(handle);
        }

        let start_lock = {
            let mut inner = self.inner.lock().await;
            if let Some(handle) = inner.servers.get(key) {
                return Ok(Arc::clone(handle));
            }
            Arc::clone(
                inner
                    .starting
                    .entry(key.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };

        let _start_guard = start_lock.lock().await;

        if let Some(handle) = self.existing_handle(key).await {
            return Ok(handle);
        }

        let config = {
            let inner = self.inner.lock().await;
            inner.server_configs.get(key).cloned()
        };

        let start_result = if let Some(config) = config {
            LspServerHandle::start_with_config(config).await
        } else {
            LspServerHandle::start(&key.language_id, workspace_root).await
        };
        #[cfg_attr(not(feature = "lsp-push-diagnostics"), allow(unused_mut))]
        let mut handle = match start_result {
            Ok(h) => h,
            Err(e) => {
                let mut inner = self.inner.lock().await;
                inner.starting.remove(key);
                return Err(e);
            }
        };

        #[cfg(feature = "lsp-push-diagnostics")]
        {
            let listeners = {
                let inner = self.inner.lock().await;
                Arc::clone(&inner.diagnostics_listeners)
            };
            handle.notification_listeners = Arc::clone(&listeners);
            if let Some(mut notifications) = handle.notifications.take() {
                crate::runtime::spawn_supervised("lsp.diagnostics_pump", async move {
                    while let Some(msg) = notifications.recv().await {
                        LspServerHandle::dispatch_push_notification(&msg, &listeners);
                    }
                });
            }
        }

        let arc = Arc::new(Mutex::new(handle));
        {
            let mut inner = self.inner.lock().await;
            inner.servers.insert(key.clone(), Arc::clone(&arc));
            inner.starting.remove(key);
        }
        Ok(arc)
    }

    async fn drop_server(&self, key: &ServerKey) {
        let mut inner = self.inner.lock().await;
        inner.servers.remove(key);
    }

    pub async fn request(
        &self,
        language: &str,
        workspace_root: &Path,
        file_path: Option<&Path>,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let key = ServerKey::new(language, workspace_root);
        let handle = self.server_handle(&key, workspace_root).await?;
        let result = {
            let mut h = handle.lock().await;
            h.execute_with_open(file_path, &key.language_id, method, params)
                .await
        };
        match result {
            Ok(val) => Ok(val),
            Err(e) => {
                if e.downcast_ref::<LspRequestFailure>().is_some() {
                    return Err(e);
                }
                self.drop_server(&key).await;
                Err(e.context("LSP server error; the server will be restarted on next attempt"))
            }
        }
    }

    pub async fn notify(
        &self,
        language: &str,
        workspace_root: &Path,
        method: &str,
        params: serde_json::Value,
    ) -> Result<()> {
        let key = ServerKey::new(language, workspace_root);
        let handle = self.server_handle(&key, workspace_root).await?;
        let mut h = handle.lock().await;
        h.send_notification(method, params).await
    }

    pub async fn notify_file_changed(&self, path: &Path, contents: &str) -> Result<()> {
        let language = match lsp_language_id_from_path(path) {
            Some(l) => l,
            None => return Ok(()),
        };
        let workspace_root = infer_workspace_root(path).unwrap_or_else(|| path.to_path_buf());
        let key = ServerKey::new(&language, &workspace_root);
        let Some(handle) = self.existing_handle(&key).await else {
            return Ok(());
        };
        let uri = path_to_uri(path);
        let mut h = handle.lock().await;
        if !h.opened_files.contains(&uri) {
            return Ok(());
        }
        let version = h.next_doc_version(&uri);
        let params = h.build_did_change_params(&uri, version, contents);
        h.cache_doc_text(&uri, contents);
        h.send_notification("textDocument/didChange", params).await
    }

    pub async fn ensure_server_started(
        &self,
        language_id: &str,
        workspace_root: &Path,
    ) -> Result<()> {
        let key = ServerKey::new(language_id, workspace_root);
        self.server_handle(&key, workspace_root).await?;
        Ok(())
    }

    pub async fn open_text_document(
        &self,
        path: &Path,
        language_id: &str,
        text: &str,
        version: i64,
    ) -> Result<()> {
        let workspace_root =
            infer_workspace_root(path).unwrap_or_else(|| path.parent().unwrap_or(path).to_path_buf());
        let key = ServerKey::new(language_id, &workspace_root);
        let handle = self.server_handle(&key, &workspace_root).await?;
        let uri = path_to_uri(path);
        let mut h = handle.lock().await;
        if h.opened_files.contains(&uri) {
            return Ok(());
        }
        let version = clamp_doc_version(version).unwrap_or(1);
        h.doc_versions.insert(uri.clone(), version);
        h.send_notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": &uri,
                    "languageId": language_id,
                    "version": version,
                    "text": text,
                }
            }),
        )
        .await?;
        h.cache_doc_text(&uri, text);
        h.opened_files.insert(uri);
        Ok(())
    }

    pub async fn change_text_document(
        &self,
        path: &Path,
        language_id: &str,
        text: &str,
        version: i64,
    ) -> Result<()> {
        let workspace_root =
            infer_workspace_root(path).unwrap_or_else(|| path.parent().unwrap_or(path).to_path_buf());
        let key = ServerKey::new(language_id, &workspace_root);
        let handle = self.server_handle(&key, &workspace_root).await?;
        let uri = path_to_uri(path);
        let mut h = handle.lock().await;
        if !h.opened_files.contains(&uri) {
            let open_version = clamp_doc_version(version).unwrap_or(1);
            h.doc_versions.insert(uri.clone(), open_version);
            h.send_notification(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": &uri,
                        "languageId": language_id,
                        "version": open_version,
                        "text": text,
                    }
                }),
            )
            .await?;
            h.cache_doc_text(&uri, text);
            h.opened_files.insert(uri.clone());
        }
        let change_version = match clamp_doc_version(version) {
            Some(v) => {
                h.doc_versions.insert(uri.clone(), v);
                v
            }
            None => h.next_doc_version(&uri),
        };
        let params = h.build_did_change_params(&uri, change_version, text);
        h.cache_doc_text(&uri, text);
        h.send_notification("textDocument/didChange", params).await
    }

    pub async fn notify_external_change_if_open(&self, path: &Path) {
        let Some(language) = lsp_language_id_from_path(path) else {
            return;
        };
        let workspace_root = infer_workspace_root(path)
            .unwrap_or_else(|| path.parent().unwrap_or(path).to_path_buf());
        let key = ServerKey::new(&language, &workspace_root);
        let Some(handle) = self.existing_handle(&key).await else {
            return;
        };
        let uri = path_to_uri(path);
        let text = match tokio::fs::read_to_string(path).await {
            Ok(t) => t,
            Err(_) => return,
        };
        let mut h = handle.lock().await;
        if !h.opened_files.contains(&uri) {
            return;
        }
        let change_version = h.next_doc_version(&uri);
        let params = h.build_did_change_params(&uri, change_version, &text);
        h.cache_doc_text(&uri, &text);
        let _ = h.send_notification("textDocument/didChange", params).await;
    }

    pub async fn save_text_document(
        &self,
        path: &Path,
        language_id: &str,
        text: Option<&str>,
    ) -> Result<()> {
        let workspace_root =
            infer_workspace_root(path).unwrap_or_else(|| path.parent().unwrap_or(path).to_path_buf());
        let key = ServerKey::new(language_id, &workspace_root);
        let Some(handle) = self.existing_handle(&key).await else {
            return Ok(());
        };
        let uri = path_to_uri(path);
        let mut h = handle.lock().await;
        if !h.opened_files.contains(&uri) {
            return Ok(());
        }
        let mut params = json!({
            "textDocument": { "uri": uri }
        });
        if let Some(t) = text {
            params["text"] = serde_json::Value::String(t.to_string());
        }
        h.send_notification("textDocument/didSave", params).await
    }

    pub async fn close_text_document(&self, path: &Path, language_id: &str) -> Result<()> {
        let workspace_root =
            infer_workspace_root(path).unwrap_or_else(|| path.parent().unwrap_or(path).to_path_buf());
        let key = ServerKey::new(language_id, &workspace_root);
        let Some(handle) = self.existing_handle(&key).await else {
            return Ok(());
        };
        let uri = path_to_uri(path);
        let mut h = handle.lock().await;
        if !h.opened_files.remove(&uri) {
            return Ok(());
        }
        h.doc_versions.remove(&uri);
        h.doc_texts.remove(&uri);
        h.send_notification(
            "textDocument/didClose",
            json!({
                "textDocument": { "uri": uri }
            }),
        )
        .await
    }

    pub async fn is_server_running(&self, language: &str, workspace_root: &Path) -> bool {
        let inner = self.inner.lock().await;
        let key = ServerKey::new(language, workspace_root);
        inner.servers.contains_key(&key)
    }

    pub async fn list_servers(&self) -> Vec<ServerInfo> {
        let entries: Vec<(ServerKey, Arc<Mutex<LspServerHandle>>)> = {
            let inner = self.inner.lock().await;
            inner
                .servers
                .iter()
                .map(|(key, handle)| (key.clone(), Arc::clone(handle)))
                .collect()
        };
        let mut out = Vec::with_capacity(entries.len());
        for (key, handle) in entries {
            let open_files = match handle.try_lock() {
                Ok(h) => h.opened_files.len(),
                Err(_) => 0,
            };
            out.push(ServerInfo {
                language_id: key.language_id.clone(),
                workspace_root: key.workspace_root.clone(),
                status: "Running".to_string(),
                open_files,
            });
        }
        out
    }

    pub async fn shutdown_all(&self) {
        let handles: Vec<(ServerKey, Arc<Mutex<LspServerHandle>>)> = {
            let mut inner = self.inner.lock().await;
            let entries: Vec<(ServerKey, Arc<Mutex<LspServerHandle>>)> =
                inner.servers.drain().collect();
            inner.server_configs.clear();
            entries
        };
        for (_key, handle) in handles {
            let mut h = handle.lock().await;
            let _ = h.shutdown().await;
        }
    }

    pub async fn shutdown_server(&self, language: &str, workspace_root: &Path) {
        let key = ServerKey::new(language, workspace_root);
        let handle = {
            let mut inner = self.inner.lock().await;
            inner.server_configs.remove(&key);
            inner.servers.remove(&key)
        };
        if let Some(handle) = handle {
            let mut h = handle.lock().await;
            let _ = h.shutdown().await;
        }
    }

    pub async fn restart_server(&self, language: &str, workspace_root: &Path) -> Result<()> {
        let key = ServerKey::new(language, workspace_root);
        let handle = {
            let mut inner = self.inner.lock().await;
            inner.servers.remove(&key)
        };
        if let Some(handle) = handle {
            let mut h = handle.lock().await;
            let _ = h.shutdown().await;
        }
        self.server_handle(&key, workspace_root).await?;
        Ok(())
    }

    pub async fn get_diagnostics(&self, file: &PathBuf) -> Vec<LspDiagnostic> {
        let canonical = canonical_diag_key(file);
        let inner = self.inner.lock().await;
        #[cfg(feature = "lsp-push-diagnostics")]
        if let Ok(push) = inner.push_diagnostics.read() {
            if let Some(diags) = push.get(&canonical).or_else(|| push.get(file)) {
                return diags.clone();
            }
        }
        inner
            .diagnostics
            .get(&canonical)
            .or_else(|| inner.diagnostics.get(file))
            .cloned()
            .unwrap_or_default()
    }

    pub async fn get_all_diagnostics(&self) -> HashMap<PathBuf, Vec<LspDiagnostic>> {
        let inner = self.inner.lock().await;
        #[cfg_attr(not(feature = "lsp-push-diagnostics"), allow(unused_mut))]
        let mut merged = inner.diagnostics.clone();
        #[cfg(feature = "lsp-push-diagnostics")]
        if let Ok(push) = inner.push_diagnostics.read() {
            for (path, diags) in push.iter() {
                merged.insert(path.clone(), diags.clone());
            }
        }
        merged
    }

    pub async fn update_diagnostics(&self, file: PathBuf, diagnostics: Vec<LspDiagnostic>) {
        let mut inner = self.inner.lock().await;
        if diagnostics.is_empty() {
            inner.diagnostics.remove(&file);
        } else {
            inner.diagnostics.insert(file, diagnostics);
        }
    }

    #[cfg(feature = "lsp-push-diagnostics")]
    pub async fn register_diagnostics_listener(&self, listener: Arc<dyn DiagnosticsListener>) {
        let inner = self.inner.lock().await;
        if let Ok(mut guard) = inner.diagnostics_listeners.write() {
            guard.push(listener);
        }
    }

    pub async fn hover(
        &self,
        path: impl AsRef<Path>,
        line: u32,
        character: u32,
    ) -> Option<String> {
        let path = path.as_ref();
        let language = lsp_language_id_from_path(path)?;
        let workspace_root =
            infer_workspace_root(path).unwrap_or_else(|| path.parent().unwrap_or(path).to_path_buf());
        let file_uri = path_to_uri(path);
        let params = json!({
            "textDocument": { "uri": file_uri },
            "position": { "line": line, "character": character },
        });
        match self
            .request(&language, &workspace_root, Some(path), "textDocument/hover", params)
            .await
        {
            Ok(result) => {
                let text = format_hover(&result);
                if text == "No hover information available." {
                    None
                } else {
                    Some(text)
                }
            }
            Err(_) => None,
        }
    }

    pub async fn hover_if_running(
        &self,
        path: impl AsRef<Path>,
        line: u32,
        character: u32,
    ) -> Option<String> {
        let path = path.as_ref();
        let language = lsp_language_id_from_path(path)?;
        let workspace_root =
            infer_workspace_root(path).unwrap_or_else(|| path.parent().unwrap_or(path).to_path_buf());
        let key = ServerKey::new(&language, &workspace_root);
        let handle = self.existing_handle(&key).await?;
        let file_uri = path_to_uri(path);
        let params = json!({
            "textDocument": { "uri": file_uri },
            "position": { "line": line, "character": character },
        });
        let result = {
            let mut h = handle.lock().await;
            h.execute_with_open(Some(path), &key.language_id, "textDocument/hover", params)
                .await
        };
        match result {
            Ok(value) => {
                let text = format_hover(&value);
                if text == "No hover information available." {
                    None
                } else {
                    Some(text)
                }
            }
            Err(_) => None,
        }
    }

    pub async fn refresh_diagnostics(
        &self,
        file: &Path,
        language: &str,
        workspace_root: &Path,
    ) -> Result<Vec<LspDiagnostic>> {
        let file_uri = path_to_uri(file);
        let params = json!({
            "textDocument": {
                "uri": file_uri
            }
        });

        let result = self
            .request(
                language,
                workspace_root,
                Some(file),
                "textDocument/diagnostic",
                params,
            )
            .await;

        match result {
            Ok(response) => {
                let diags = parse_lsp_diagnostics(&response);
                let path = canonical_diag_key(file);
                let mut inner = self.inner.lock().await;
                inner.diagnostics.insert(path, diags.clone());
                Ok(diags)
            }
            Err(e) => {
                tracing::debug!(error = %e, "textDocument/diagnostic not supported, trying alternative");
                let inner = self.inner.lock().await;
                Ok(inner
                    .diagnostics
                    .get(&canonical_diag_key(file))
                    .cloned()
                    .unwrap_or_default())
            }
        }
    }
}

impl Default for LspService {
    fn default() -> Self {
        Self::new()
    }
}

impl LspServerHandle {
    async fn start(language: &str, workspace_root: &Path) -> Result<Self> {
        let (binary, args) = detect_server(language)?;

        let mut cmd = crate::util::hidden_async_command(&binary);
        cmd.args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);

        if language.eq_ignore_ascii_case("python") {
            for (k, v) in crate::python_env::activation_env(workspace_root) {
                cmd.env(k, v);
            }
            cmd.env_remove("PYTHONHOME");
        }

        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x0800_0000);

        let mut process = cmd.spawn().with_context(|| {
            format!("Failed to start language server '{binary}' for '{language}'")
        })?;

        let stdin = process
            .stdin
            .take()
            .context("Failed to capture server stdin")?;
        let stdout = process
            .stdout
            .take()
            .context("Failed to capture server stdout")?;

        let reader = spawn_stdout_reader(stdout);
        let mut handle = Self {
            _process: process,
            stdin,
            incoming: reader.incoming,
            reader_task: reader.task,
            next_id: 1,
            initialized: false,
            opened_files: HashSet::new(),
            doc_versions: HashMap::new(),
            doc_texts: HashMap::new(),
            did_change_support: super::incremental::DidChangeSupport::Full,
            #[cfg(feature = "lsp-push-diagnostics")]
            notification_listeners: Arc::new(std::sync::RwLock::new(Vec::new())),
            #[cfg(feature = "lsp-push-diagnostics")]
            notifications: Some(reader.notifications),
        };

        handle.initialize(workspace_root).await?;
        Ok(handle)
    }

    async fn start_with_config(config: LspServerConfig) -> Result<Self> {
        let mut cmd = crate::util::hidden_async_command(&config.server_command);
        cmd.args(&config.server_args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);

        if config.language_id.eq_ignore_ascii_case("python") {
            for (k, v) in crate::python_env::activation_env(&config.root_path) {
                cmd.env(k, v);
            }
            cmd.env_remove("PYTHONHOME");
        }

        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x0800_0000);

        let mut process = cmd.spawn().with_context(|| {
            format!(
                "Failed to start language server '{}' for '{}'",
                config.server_command, config.language_id
            )
        })?;

        let stdin = process
            .stdin
            .take()
            .context("Failed to capture server stdin")?;
        let stdout = process
            .stdout
            .take()
            .context("Failed to capture server stdout")?;

        let reader = spawn_stdout_reader(stdout);
        let mut handle = Self {
            _process: process,
            stdin,
            incoming: reader.incoming,
            reader_task: reader.task,
            next_id: 1,
            initialized: false,
            opened_files: HashSet::new(),
            doc_versions: HashMap::new(),
            doc_texts: HashMap::new(),
            did_change_support: super::incremental::DidChangeSupport::Full,
            #[cfg(feature = "lsp-push-diagnostics")]
            notification_listeners: Arc::new(std::sync::RwLock::new(Vec::new())),
            #[cfg(feature = "lsp-push-diagnostics")]
            notifications: Some(reader.notifications),
        };

        handle.initialize(&config.root_path).await?;
        Ok(handle)
    }

    async fn initialize(&mut self, workspace_root: &Path) -> Result<()> {
        let root_uri = path_to_uri(workspace_root);
        let ws_name = workspace_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".to_string());

        let params = json!({
            "processId": std::process::id(),
            "capabilities": {
                "textDocument": {
                    "synchronization": {
                        "dynamicRegistration": false,
                        "willSave": true,
                        "willSaveWaitUntil": false,
                        "didSave": true
                    },
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "definition": { "linkSupport": true },
                    "typeDefinition": { "linkSupport": true },
                    "implementation": { "linkSupport": true },
                    "declaration": { "linkSupport": true },
                    "references": { "dynamicRegistration": false },
                    "documentSymbol": {
                        "hierarchicalDocumentSymbolSupport": true,
                        "symbolKind": {
                            "valueSet": [
                                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13,
                                14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26
                            ]
                        },
                        "tagSupport": { "valueSet": [1] }
                    },
                    "publishDiagnostics": {
                        "relatedInformation": true,
                        "versionSupport": true,
                        "codeDescriptionSupport": true,
                        "dataSupport": true,
                        "tagSupport": { "valueSet": [1, 2] }
                    },
                    "diagnostic": {
                        "dynamicRegistration": false,
                        "relatedDocumentSupport": true
                    },
                    "callHierarchy": {},
                    "completion": {
                        "completionItem": {
                            "snippetSupport": true,
                            "commitCharactersSupport": false,
                            "documentationFormat": ["markdown", "plaintext"],
                            "deprecatedSupport": true,
                            "preselectSupport": true,
                            "tagSupport": { "valueSet": [1] },
                            "insertReplaceSupport": true,
                            "resolveSupport": {
                                "properties": [
                                    "documentation",
                                    "detail",
                                    "additionalTextEdits",
                                    "command"
                                ]
                            },
                            "insertTextModeSupport": { "valueSet": [1, 2] },
                            "labelDetailsSupport": true
                        },
                        "completionItemKind": {
                            "valueSet": [
                                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13,
                                14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25
                            ]
                        },
                        "contextSupport": true,
                        "insertTextMode": 2
                    },
                    "signatureHelp": {
                        "signatureInformation": {
                            "documentationFormat": ["markdown", "plaintext"],
                            "parameterInformation": {
                                "labelOffsetSupport": true
                            },
                            "activeParameterSupport": true
                        },
                        "contextSupport": true
                    },
                    "inlayHint": {
                        "dynamicRegistration": false,
                        "resolveSupport": {
                            "properties": [
                                "tooltip",
                                "textEdits",
                                "label.tooltip",
                                "label.location",
                                "label.command"
                            ]
                        }
                    },
                    "codeAction": {
                        "dynamicRegistration": false,
                        "isPreferredSupport": true,
                        "disabledSupport": true,
                        "dataSupport": true,
                        "honorsChangeAnnotations": false,
                        "resolveSupport": {
                            "properties": ["edit", "command"]
                        },
                        "codeActionLiteralSupport": {
                            "codeActionKind": {
                                "valueSet": [
                                    "",
                                    "quickfix",
                                    "refactor",
                                    "refactor.extract",
                                    "refactor.inline",
                                    "refactor.rewrite",
                                    "source",
                                    "source.organizeImports",
                                    "source.fixAll"
                                ]
                            }
                        }
                    },
                    "formatting": { "dynamicRegistration": false },
                    "rangeFormatting": { "dynamicRegistration": false },
                    "onTypeFormatting": { "dynamicRegistration": false },
                    "rename": {
                        "dynamicRegistration": false,
                        "prepareSupport": true,
                        "prepareSupportDefaultBehavior": 1,
                        "honorsChangeAnnotations": false
                    },
                    "documentHighlight": { "dynamicRegistration": false },
                    "documentLink": {
                        "dynamicRegistration": false,
                        "tooltipSupport": true
                    },
                    "foldingRange": {
                        "dynamicRegistration": false,
                        "rangeLimit": 5000,
                        "lineFoldingOnly": true,
                        "foldingRangeKind": {
                            "valueSet": ["comment", "imports", "region"]
                        },
                        "foldingRange": { "collapsedText": false }
                    },
                    "selectionRange": { "dynamicRegistration": false },
                    "semanticTokens": {
                        "dynamicRegistration": false,
                        "requests": {
                            "range": true,
                            "full": { "delta": true }
                        },
                        "tokenTypes": [
                            "namespace", "type", "class", "enum", "interface",
                            "struct", "typeParameter", "parameter", "variable",
                            "property", "enumMember", "event", "function",
                            "method", "macro", "keyword", "modifier", "comment",
                            "string", "number", "regexp", "operator", "decorator"
                        ],
                        "tokenModifiers": [
                            "declaration", "definition", "readonly", "static",
                            "deprecated", "abstract", "async", "modification",
                            "documentation", "defaultLibrary"
                        ],
                        "formats": ["relative"],
                        "overlappingTokenSupport": false,
                        "multilineTokenSupport": false,
                        "serverCancelSupport": true,
                        "augmentsSyntaxTokens": true
                    }
                },
                "workspace": {
                    "applyEdit": true,
                    "workspaceEdit": {
                        "documentChanges": true,
                        "resourceOperations": ["create", "rename", "delete"],
                        "failureHandling": "textOnlyTransactional",
                        "normalizesLineEndings": true,
                        "changeAnnotationSupport": { "groupsOnLabel": false }
                    },
                    "symbol": {
                        "dynamicRegistration": false,
                        "symbolKind": {
                            "valueSet": [
                                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13,
                                14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26
                            ]
                        },
                        "tagSupport": { "valueSet": [1] },
                        "resolveSupport": { "properties": ["location.range"] }
                    },
                    "executeCommand": { "dynamicRegistration": false },
                    "configuration": true,
                    "didChangeConfiguration": { "dynamicRegistration": false },
                    "didChangeWatchedFiles": { "dynamicRegistration": false },
                    "workspaceFolders": true,
                    "semanticTokens": { "refreshSupport": true },
                    "inlayHint": { "refreshSupport": true },
                    "diagnostics": { "refreshSupport": true }
                },
                "window": {
                    "workDoneProgress": true,
                    "showMessage": {
                        "messageActionItem": { "additionalPropertiesSupport": false }
                    }
                },
                "general": {
                    "positionEncodings": ["utf-16"]
                }
            },
            "rootUri": &root_uri,
            "workspaceFolders": [{
                "uri": &root_uri,
                "name": ws_name
            }]
        });

        let init_result = self
            .send_request_with_timeout("initialize", params, INIT_TIMEOUT)
            .await
            .context("Language server initialization failed")?;

        let sync = init_result
            .get("capabilities")
            .and_then(|c| c.get("textDocumentSync"));
        let change_kind = match sync {
            Some(serde_json::Value::Number(n)) => n.as_i64(),
            Some(serde_json::Value::Object(o)) => o.get("change").and_then(|v| v.as_i64()),
            _ => None,
        };
        if let Some(kind) = change_kind {
            self.did_change_support = super::incremental::DidChangeSupport::from_capability(kind);
        }

        self.send_notification("initialized", json!({})).await?;
        self.initialized = true;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        if self.initialized {
            let _ = self.send_request("shutdown", json!(null)).await;
            let _ = self.send_notification("exit", json!(null)).await;
        }
        Ok(())
    }

    async fn ensure_file_open(&mut self, path: &Path, language: &str) -> Result<()> {
        let uri = path_to_uri(path);
        if self.opened_files.contains(&uri) {
            return Ok(());
        }
        let text = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read file: {}", path.display()))?;

        self.send_notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": &uri,
                    "languageId": language,
                    "version": 1,
                    "text": text,
                }
            }),
        )
        .await?;

        self.cache_doc_text(&uri, &text);
        self.opened_files.insert(uri);
        Ok(())
    }

    async fn execute_with_open(
        &mut self,
        file_path: Option<&Path>,
        language: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        if let Some(path) = file_path {
            self.ensure_file_open(path, language).await?;
        }
        self.send_request(method, params).await
    }

    async fn send_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.send_request_with_timeout(method, params, REQUEST_TIMEOUT)
            .await
    }

    async fn send_request_with_timeout(
        &mut self,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;

        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        write_message(&mut self.stdin, &msg).await?;

        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let resp = match tokio::time::timeout_at(deadline, self.incoming.recv()).await {
                Ok(Some(resp)) => resp,
                Ok(None) => {
                    anyhow::bail!("Language server closed its stdout unexpectedly");
                }
                Err(_) => {
                    return Err(anyhow::Error::new(LspRequestFailure::Timeout {
                        method: method.to_string(),
                    }));
                }
            };

            #[cfg(feature = "lsp-push-diagnostics")]
            Self::dispatch_push_notification(&resp, &self.notification_listeners);

            if resp.get("id").is_some() && resp.get("method").is_some() {
                let req_id = resp.get("id").cloned().unwrap_or(json!(null));
                let req_method = resp
                    .get("method")
                    .and_then(|m| m.as_str())
                    .unwrap_or_default()
                    .to_string();
                let reply = match req_method.as_str() {
                    "workspace/configuration" => {
                        let count = resp
                            .pointer("/params/items")
                            .and_then(|items| items.as_array())
                            .map_or(0, Vec::len);
                        json!({
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "result": vec![serde_json::Value::Null; count],
                        })
                    }
                    "window/workDoneProgress/create"
                    | "client/registerCapability"
                    | "client/unregisterCapability" => {
                        json!({ "jsonrpc": "2.0", "id": req_id, "result": null })
                    }
                    _ => json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "error": {
                            "code": -32601,
                            "message": format!(
                                "method not supported by this client: {req_method}"
                            ),
                        },
                    }),
                };
                if let Err(e) = write_message(&mut self.stdin, &reply).await {
                    tracing::debug!(
                        method = %req_method,
                        error = %e,
                        "failed to answer server-initiated LSP request"
                    );
                }
                continue;
            }

            let resp_id = resp.get("id").and_then(|v| v.as_u64());
            if resp_id == Some(id) && resp.get("method").is_none() {
                if let Some(error) = resp.get("error") {
                    let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
                    let message = error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown error")
                        .to_string();
                    return Err(anyhow::Error::new(LspRequestFailure::Protocol {
                        method: method.to_string(),
                        code,
                        message,
                    }));
                }
                return Ok(resp.get("result").cloned().unwrap_or(json!(null)));
            }
        }
    }

    async fn send_notification(&mut self, method: &str, params: serde_json::Value) -> Result<()> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        write_message(&mut self.stdin, &msg).await
    }

    #[cfg(feature = "lsp-push-diagnostics")]
    fn dispatch_push_notification(
        msg: &serde_json::Value,
        listeners: &std::sync::RwLock<Vec<Arc<dyn DiagnosticsListener>>>,
    ) {
        if msg
            .get("method")
            .and_then(|m| m.as_str())
            .map_or(true, |m| m != "textDocument/publishDiagnostics")
        {
            return;
        }
        let params = msg.get("params").unwrap_or(&serde_json::Value::Null);
        let uri = params
            .get("uri")
            .and_then(|u| u.as_str())
            .unwrap_or("");
        let empty: Vec<serde_json::Value> = Vec::new();
        let diags: &[serde_json::Value] = params
            .get("diagnostics")
            .and_then(|d| d.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(empty.as_slice());
        if let Ok(guard) = listeners.read() {
            for listener in guard.iter() {
                listener.on_diagnostics(uri, diags);
            }
        }
    }
}

const WRITE_TIMEOUT: Duration = Duration::from_secs(15);

async fn write_message(stdin: &mut ChildStdin, msg: &serde_json::Value) -> Result<()> {
    let body = serde_json::to_string(msg)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    tokio::time::timeout(WRITE_TIMEOUT, async {
        stdin.write_all(header.as_bytes()).await?;
        stdin.write_all(body.as_bytes()).await?;
        stdin.flush().await?;
        Ok::<(), std::io::Error>(())
    })
    .await
    .context("LSP stdin write timed out; language server is not draining its input")??;
    Ok(())
}

async fn read_message(reader: &mut BufReader<ChildStdout>) -> Result<serde_json::Value> {
    let mut content_length: Option<usize> = None;

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            anyhow::bail!("Language server closed its stdout unexpectedly");
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(len_str) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(len_str.trim().parse().context("Invalid Content-Length")?);
        }
    }

    let length = content_length.context("Missing Content-Length header from server")?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).await?;
    serde_json::from_slice(&body).context("Failed to parse JSON-RPC message from server")
}

pub fn detect_language(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "rs" => Some("rust"),
        "ts" | "tsx" | "mts" | "cts" => Some("typescript"),
        "js" | "jsx" | "mjs" | "cjs" => Some("javascript"),
        "py" | "pyi" => Some("python"),
        "go" => Some("go"),
        "c" | "h" => Some("c"),
        "cpp" | "cxx" | "cc" | "hpp" | "hxx" | "hh" => Some("cpp"),
        "java" => Some("java"),
        "zig" => Some("zig"),
        "lua" => Some("lua"),
        "rb" => Some("ruby"),
        "swift" => Some("swift"),
        "kt" | "kts" => Some("kotlin"),
        "cs" => Some("csharp"),
        "css" => Some("css"),
        "scss" | "sass" => Some("scss"),
        "less" => Some("less"),
        "html" | "htm" => Some("html"),
        "json" | "json5" | "jsonc" => Some("json"),
        "yaml" | "yml" => Some("yaml"),
        "toml" => Some("toml"),
        "sh" | "bash" | "zsh" | "ksh" | "fish" => Some("shell"),
        _ => None,
    }
}

fn server_candidates(language: &str) -> &'static [(&'static str, &'static [&'static str])] {
    match language {
        "rust" => &[("rust-analyzer", &[])],
        "typescript" | "javascript" => &[("typescript-language-server", &["--stdio"])],
        "python" => &[("pylsp", &[]), ("pyright-langserver", &["--stdio"])],
        "go" => &[("gopls", &[])],
        "c" | "cpp" => &[("clangd", &[])],
        "java" => &[("jdtls", &[])],
        "csharp" => &[("omnisharp", &["-lsp"]), ("OmniSharp", &["-lsp"])],
        "zig" => &[("zls", &[])],
        "lua" => &[("lua-language-server", &[])],
        "ruby" => &[("solargraph", &["stdio"])],
        "swift" => &[("sourcekit-lsp", &[])],
        "kotlin" => &[("kotlin-language-server", &[])],
        "css" | "scss" | "less" => &[
            ("vscode-css-language-server", &["--stdio"]),
            ("css-languageserver", &["--stdio"]),
        ],
        "html" | "htm" => &[
            ("vscode-html-language-server", &["--stdio"]),
            ("html-languageserver", &["--stdio"]),
        ],
        "json" | "jsonc" => &[
            ("vscode-json-language-server", &["--stdio"]),
            ("json-languageserver", &["--stdio"]),
        ],
        "yaml" | "yml" => &[("yaml-language-server", &["--stdio"])],
        "toml" => &[("taplo", &["lsp", "stdio"])],
        "shell" | "bash" => &[("bash-language-server", &["start"])],
        _ => &[],
    }
}

fn detect_server(language: &str) -> Result<(String, Vec<String>)> {
    let candidates = server_candidates(language);
    if candidates.is_empty() {
        anyhow::bail!("No known language server for '{language}'");
    }

    for (cmd, args) in candidates {
        if which::which(cmd).is_ok() {
            return Ok((
                cmd.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ));
        }
    }

    let names: Vec<_> = candidates.iter().map(|(name, _)| *name).collect();
    anyhow::bail!(
        "No language server found for '{language}'. Tried: {}. \
         Install one of these and ensure it is on your PATH.",
        names.join(", ")
    )
}

pub fn path_to_uri(path: &Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let s = normalize_drive_case(&abs.to_string_lossy().replace('\\', "/"));
    let encoded = percent_encode_uri_path(&s);
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

fn percent_encode_uri_path(path: &str) -> String {
    const KEEP: &[u8] = b"/:-._~!$&'()*+,;=@";
    let mut out = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        let b = *byte;
        if b.is_ascii_alphanumeric() || KEEP.contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &input[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn normalize_drive_case(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() >= 2
        && bytes[0].is_ascii_lowercase()
        && bytes[1] == b':'
    {
        let mut s = String::with_capacity(path.len());
        s.push(bytes[0].to_ascii_uppercase() as char);
        s.push_str(&path[1..]);
        return s;
    }
    if bytes.len() >= 3
        && bytes[0] == b'/'
        && bytes[1].is_ascii_lowercase()
        && bytes[2] == b':'
    {
        let mut s = String::with_capacity(path.len());
        s.push('/');
        s.push(bytes[1].to_ascii_uppercase() as char);
        s.push_str(&path[2..]);
        return s;
    }
    path.to_string()
}

fn uri_to_path_string(uri: &str) -> String {
    let stripped = uri.strip_prefix("file://").unwrap_or(uri);
    let decoded = percent_decode(stripped);
    let bytes = decoded.as_bytes();
    let trimmed = if bytes.len() >= 3
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
    {
        decoded[1..].to_string()
    } else {
        decoded
    };
    normalize_drive_case(&trimmed)
}

pub fn canonical_diag_key(path: &Path) -> PathBuf {
    let s = path.to_string_lossy().replace('\\', "/");
    PathBuf::from(normalize_drive_case(&s))
}

fn symbol_kind_name(kind: u64) -> &'static str {
    match kind {
        1 => "File",
        2 => "Module",
        3 => "Namespace",
        4 => "Package",
        5 => "Class",
        6 => "Method",
        7 => "Property",
        8 => "Field",
        9 => "Constructor",
        10 => "Enum",
        11 => "Interface",
        12 => "Function",
        13 => "Variable",
        14 => "Constant",
        15 => "String",
        16 => "Number",
        17 => "Boolean",
        18 => "Array",
        19 => "Object",
        20 => "Key",
        21 => "Null",
        22 => "EnumMember",
        23 => "Struct",
        24 => "Event",
        25 => "Operator",
        26 => "TypeParameter",
        _ => "Unknown",
    }
}

fn severity_label(sev: u64) -> &'static str {
    match sev {
        1 => "Error",
        2 => "Warning",
        3 => "Information",
        4 => "Hint",
        _ => "Unknown",
    }
}

fn parse_lsp_diagnostics(response: &serde_json::Value) -> Vec<LspDiagnostic> {
    let items = response
        .get("items")
        .or_else(|| response.get("diagnostics"))
        .and_then(|v| v.as_array());

    match items {
        Some(arr) => arr
            .iter()
            .filter_map(|d| {
                let message = d.get("message")?.as_str()?.to_string();
                let severity = d.get("severity").and_then(|s| s.as_u64()).unwrap_or(1);
                let range = d.get("range")?;
                let start_line = range.get("start")?.get("line")?.as_u64()? as u32;
                let start_char = range.get("start")?.get("character")?.as_u64().unwrap_or(0) as u32;
                Some(LspDiagnostic {
                    message,
                    severity: match severity {
                        1 => DiagnosticSeverity::Error,
                        2 => DiagnosticSeverity::Warning,
                        3 => DiagnosticSeverity::Information,
                        _ => DiagnosticSeverity::Hint,
                    },
                    range: LspRange {
                        start_line,
                        start_character: start_char,
                        end_line: range
                            .get("end")
                            .and_then(|e| e.get("line"))
                            .and_then(|l| l.as_u64())
                            .unwrap_or(start_line as u64) as u32,
                        end_character: range
                            .get("end")
                            .and_then(|e| e.get("character"))
                            .and_then(|c| c.as_u64())
                            .unwrap_or(0) as u32,
                    },
                    source: d.get("source").and_then(|s| s.as_str()).map(String::from),
                    code: d.get("code").and_then(|c| {
                        c.as_str()
                            .map(String::from)
                            .or_else(|| c.as_u64().map(|n| n.to_string()))
                    }),
                })
            })
            .collect(),
        None => Vec::new(),
    }
}

pub fn format_locations(result: &serde_json::Value, label: &str) -> String {
    let locations: Vec<&serde_json::Value> = if let Some(arr) = result.as_array() {
        arr.iter().collect()
    } else if result.is_object() {
        vec![result]
    } else {
        return format!("No {label} found.");
    };

    if locations.is_empty() {
        return format!("No {label} found.");
    }

    let mut out = format!(
        "{label} ({} result{}):\n",
        locations.len(),
        if locations.len() == 1 { "" } else { "s" }
    );

    for loc in &locations {
        let uri = loc
            .get("uri")
            .or_else(|| loc.get("targetUri"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let range = loc
            .get("range")
            .or_else(|| loc.get("targetSelectionRange"))
            .or_else(|| loc.get("targetRange"));
        let (line, col) = range
            .map(|r| {
                let l = r["start"]["line"].as_u64().unwrap_or(0) + 1;
                let c = r["start"]["character"].as_u64().unwrap_or(0) + 1;
                (l, c)
            })
            .unwrap_or((1, 1));
        let path = uri_to_path_string(uri);
        out.push_str(&format!("  -> {path}:{line}:{col}\n"));
    }

    out
}

pub fn format_hover(result: &serde_json::Value) -> String {
    if result.is_null() {
        return "No hover information available.".to_string();
    }

    let contents = &result["contents"];
    let text = if let Some(s) = contents.as_str() {
        s.to_string()
    } else if let Some(value) = contents.get("value").and_then(|v| v.as_str()) {
        value.to_string()
    } else if let Some(arr) = contents.as_array() {
        arr.iter()
            .filter_map(|item| {
                item.as_str()
                    .map(String::from)
                    .or_else(|| item.get("value").and_then(|v| v.as_str()).map(String::from))
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    } else {
        format!("{contents}")
    };

    if text.is_empty() {
        "No hover information available.".to_string()
    } else {
        format!("Hover:\n{text}")
    }
}

pub fn format_document_symbols(result: &serde_json::Value) -> String {
    let symbols = match result.as_array() {
        Some(arr) if !arr.is_empty() => arr,
        _ => return "No symbols found.".to_string(),
    };

    let mut out = format!("Document symbols ({}):\n", symbols.len());
    format_symbols_recursive(symbols, &mut out, 0);
    out
}

fn format_symbols_recursive(symbols: &[serde_json::Value], out: &mut String, depth: usize) {
    let indent = "  ".repeat(depth + 1);
    for sym in symbols {
        let name = sym.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let kind = sym
            .get("kind")
            .and_then(|v| v.as_u64())
            .map(symbol_kind_name)
            .unwrap_or("?");
        let line = sym
            .get("range")
            .or_else(|| sym.get("selectionRange"))
            .or_else(|| sym.get("location").and_then(|l| l.get("range")))
            .and_then(|r| r["start"]["line"].as_u64())
            .map(|l| l + 1)
            .unwrap_or(0);

        out.push_str(&format!("{indent}{kind}: {name} (line {line})\n"));

        if let Some(children) = sym.get("children").and_then(|c| c.as_array()) {
            format_symbols_recursive(children, out, depth + 1);
        }
    }
}

pub fn format_workspace_symbols(result: &serde_json::Value) -> String {
    let symbols = match result.as_array() {
        Some(arr) if !arr.is_empty() => arr,
        _ => return "No matching symbols found.".to_string(),
    };

    let mut out = format!("Workspace symbols ({}):\n", symbols.len());

    for sym in symbols {
        let name = sym.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let kind = sym
            .get("kind")
            .and_then(|v| v.as_u64())
            .map(symbol_kind_name)
            .unwrap_or("?");
        let file = sym
            .get("location")
            .and_then(|l| l.get("uri"))
            .and_then(|u| u.as_str())
            .map(uri_to_path_string)
            .unwrap_or_default();
        let line = sym
            .get("location")
            .and_then(|l| l.get("range"))
            .and_then(|r| r["start"]["line"].as_u64())
            .map(|l| l + 1)
            .unwrap_or(0);
        let container = sym
            .get("containerName")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let container_part = if container.is_empty() {
            String::new()
        } else {
            format!(" in {container}")
        };

        out.push_str(&format!(
            "  {kind}: {name}{container_part} ({file}:{line})\n"
        ));
    }

    out
}

pub fn format_diagnostics(result: &serde_json::Value, file_path: &str) -> String {
    let items = result
        .get("items")
        .and_then(|v| v.as_array())
        .or_else(|| result.as_array());

    let items = match items {
        Some(arr) if !arr.is_empty() => arr,
        _ => return format!("No diagnostics for '{file_path}'."),
    };

    let mut out = format!("Diagnostics for '{}' ({}):\n", file_path, items.len());

    for diag in items {
        let severity = diag
            .get("severity")
            .and_then(|v| v.as_u64())
            .map(severity_label)
            .unwrap_or("?");
        let message = diag.get("message").and_then(|v| v.as_str()).unwrap_or("?");
        let line = diag
            .get("range")
            .and_then(|r| r["start"]["line"].as_u64())
            .map(|l| l + 1)
            .unwrap_or(0);
        let col = diag
            .get("range")
            .and_then(|r| r["start"]["character"].as_u64())
            .map(|c| c + 1)
            .unwrap_or(0);
        let source = diag.get("source").and_then(|v| v.as_str()).unwrap_or("");
        let source_part = if source.is_empty() {
            String::new()
        } else {
            format!(" [{source}]")
        };
        let code = diag
            .get("code")
            .map(|v| {
                if let Some(s) = v.as_str() {
                    format!(" {s}")
                } else if let Some(n) = v.as_u64() {
                    format!(" {n}")
                } else {
                    String::new()
                }
            })
            .unwrap_or_default();

        out.push_str(&format!(
            "  {severity}{code}{source_part}: {message} (line {line}:{col})\n"
        ));
    }

    out
}

pub fn format_call_hierarchy(
    items: &serde_json::Value,
    calls: &serde_json::Value,
    direction: &str,
) -> String {
    let items_arr = match items.as_array() {
        Some(arr) if !arr.is_empty() => arr,
        _ => return "No call hierarchy information available at this position.".to_string(),
    };

    let item = &items_arr[0];
    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let kind = item
        .get("kind")
        .and_then(|v| v.as_u64())
        .map(symbol_kind_name)
        .unwrap_or("?");

    let mut out = format!("Call hierarchy for {kind} '{name}':\n");

    let calls_arr = match calls.as_array() {
        Some(arr) if !arr.is_empty() => arr,
        _ => {
            out.push_str(&format!("  No {direction} calls found.\n"));
            return out;
        }
    };

    out.push_str(&format!("  {direction} calls ({}):\n", calls_arr.len()));

    for call in calls_arr {
        let caller = match call.get("from").or_else(|| call.get("to")) {
            Some(c) => c,
            None => continue,
        };
        let caller_name = caller.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let caller_kind = caller
            .get("kind")
            .and_then(|v| v.as_u64())
            .map(symbol_kind_name)
            .unwrap_or("?");
        let uri = caller.get("uri").and_then(|v| v.as_str()).unwrap_or("");
        let line = caller
            .get("range")
            .and_then(|r| r["start"]["line"].as_u64())
            .map(|l| l + 1)
            .unwrap_or(0);
        let path = uri_to_path_string(uri);
        out.push_str(&format!(
            "    -> {caller_kind} '{caller_name}' in {path}:{line}\n"
        ));
    }

    out
}

fn lsp_language_id_from_path(path: &Path) -> Option<String> {
    detect_language(path).map(|s| s.to_string())
}

fn infer_workspace_root(path: &Path) -> Option<PathBuf> {
    const MARKERS: &[&str] = &[
        ".git",
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "go.mod",
        ".sen",
    ];
    let mut cursor = path.parent()?.to_path_buf();
    loop {
        for marker in MARKERS {
            if cursor.join(marker).exists() {
                return Some(cursor);
            }
        }
        if !cursor.pop() {
            break;
        }
    }
    path.parent().map(|p| p.to_path_buf())
}

pub struct LspServiceNotifier {
    inner: Arc<LspService>,
}

impl LspServiceNotifier {
    #[must_use]
    pub fn new(inner: Arc<LspService>) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl crate::apply_model::ops_applier::LspNotifier for LspServiceNotifier {
    async fn notify_changed(&self, path: &Path, contents: &str) -> anyhow::Result<()> {
        self.inner.notify_file_changed(path, contents).await
    }
}

pub struct LspServiceContextSource {
    inner: Arc<LspService>,
}

impl LspServiceContextSource {
    #[must_use]
    pub fn new(inner: Arc<LspService>) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl crate::context::builder::LspContextSource for LspServiceContextSource {
    async fn collect(
        &self,
        focus: &[PathBuf],
    ) -> Vec<crate::context::lsp_ctx::LspSnapshot> {
        use crate::context::lsp_ctx::LspSnapshot;

        let all = self.inner.get_all_diagnostics().await;
        let mut out = Vec::with_capacity(focus.len());
        for path in focus {
            let abs = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir().unwrap_or_default().join(path)
            };
            let diagnostics = all
                .get(&abs)
                .or_else(|| all.get(path))
                .cloned()
                .unwrap_or_default();
            let summary = diagnostics
                .iter()
                .take(3)
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
                .join("; ");

            let hover = self.inner.hover_if_running(path, 0, 0).await;
            out.push(LspSnapshot {
                path: path.clone(),
                diagnostics: diagnostics.len(),
                summary,
                hover,
            });
        }
        out
    }
}
