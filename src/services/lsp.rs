// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// LSP service — real Language Server Protocol client over stdio JSON-RPC.
// Manages language server processes and provides a request/response API for
// code-intelligence operations (definition, references, hover, symbols, etc.).

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

// ── Public types (backward-compatible) ──────────────────────────────────

/// A range within a document (zero-indexed line/character offsets).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspRange {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

/// A diagnostic from an LSP server.
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

/// A symbol definition from LSP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: String,
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
    pub container: Option<String>,
}

/// Configuration for an LSP server connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    pub language_id: String,
    pub server_command: String,
    pub server_args: Vec<String>,
    pub root_path: PathBuf,
    pub initialization_options: Option<serde_json::Value>,
}

// ── Internal types ──────────────────────────────────────────────────────

struct LspServerHandle {
    _process: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: u64,
    initialized: bool,
    opened_files: HashSet<String>,
}

struct LspServiceInner {
    servers: HashMap<String, LspServerHandle>,
    overrides: HashMap<String, LspServerConfig>,
    diagnostics: HashMap<PathBuf, Vec<LspDiagnostic>>,
}

// ── LspService ──────────────────────────────────────────────────────────

/// LSP service managing connections to language servers.
#[derive(Clone)]
pub struct LspService {
    inner: Arc<Mutex<LspServiceInner>>,
}

impl LspService {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(LspServiceInner {
                servers: HashMap::new(),
                overrides: HashMap::new(),
                diagnostics: HashMap::new(),
            })),
        }
    }

    /// Register a custom LSP server configuration (overrides auto-detection).
    pub async fn register_server(&self, config: LspServerConfig) {
        let mut inner = self.inner.lock().await;
        inner.overrides.insert(config.language_id.clone(), config);
    }

    /// Send an LSP request, starting the server if needed.
    ///
    /// If `file_path` is provided the document is opened (via `didOpen`) before
    /// the request is sent, which is required for most `textDocument/*` methods.
    pub async fn request(
        &self,
        language: &str,
        workspace_root: &Path,
        file_path: Option<&Path>,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let mut inner = self.inner.lock().await;
        inner
            .execute_request(language, workspace_root, file_path, method, params)
            .await
    }

    /// Send an LSP notification (no response expected).
    pub async fn notify(
        &self,
        language: &str,
        workspace_root: &Path,
        method: &str,
        params: serde_json::Value,
    ) -> Result<()> {
        let mut inner = self.inner.lock().await;
        inner.ensure_started(language, workspace_root).await?;
        let handle = inner.servers.get_mut(language).unwrap();
        handle.send_notification(method, params).await
    }

    /// List running language servers.
    pub async fn list_servers(&self) -> Vec<(String, String)> {
        let inner = self.inner.lock().await;
        inner
            .servers
            .keys()
            .map(|lang| (lang.clone(), "Running".to_string()))
            .collect()
    }

    /// Shut down all running language servers.
    pub async fn shutdown_all(&self) {
        let mut inner = self.inner.lock().await;
        let languages: Vec<String> = inner.servers.keys().cloned().collect();
        for lang in languages {
            if let Some(mut handle) = inner.servers.remove(&lang) {
                let _ = handle.shutdown().await;
            }
        }
    }

    pub async fn get_diagnostics(&self, file: &PathBuf) -> Vec<LspDiagnostic> {
        let inner = self.inner.lock().await;
        inner.diagnostics.get(file).cloned().unwrap_or_default()
    }

    pub async fn get_all_diagnostics(&self) -> HashMap<PathBuf, Vec<LspDiagnostic>> {
        let inner = self.inner.lock().await;
        inner.diagnostics.clone()
    }

    pub async fn update_diagnostics(&self, file: PathBuf, diagnostics: Vec<LspDiagnostic>) {
        let mut inner = self.inner.lock().await;
        if diagnostics.is_empty() {
            inner.diagnostics.remove(&file);
        } else {
            inner.diagnostics.insert(file, diagnostics);
        }
    }

    pub async fn refresh_diagnostics(
        &self,
        file: &Path,
        language: &str,
        workspace_root: &Path,
    ) -> Result<Vec<LspDiagnostic>> {
        let mut inner = self.inner.lock().await;
        inner.ensure_started(language, workspace_root).await?;

        let file_uri = path_to_uri(file);
        let params = json!({
            "textDocument": {
                "uri": file_uri
            }
        });

        let result = inner
            .execute_request(language, workspace_root, Some(file), "textDocument/diagnostic", params)
            .await;

        match result {
            Ok(response) => {
                let diags = parse_lsp_diagnostics(&response);
                let path = file.to_path_buf();
                inner.diagnostics.insert(path, diags.clone());
                Ok(diags)
            }
            Err(e) => {
                tracing::debug!(error = %e, "textDocument/diagnostic not supported, trying alternative");
                Ok(inner
                    .diagnostics
                    .get(&file.to_path_buf())
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

// ── LspServiceInner ─────────────────────────────────────────────────────

impl LspServiceInner {
    async fn ensure_started(&mut self, language: &str, workspace_root: &Path) -> Result<()> {
        if self.servers.contains_key(language) {
            return Ok(());
        }
        let handle = if let Some(config) = self.overrides.get(language).cloned() {
            LspServerHandle::start_with_config(config).await?
        } else {
            LspServerHandle::start(language, workspace_root).await?
        };
        self.servers.insert(language.to_string(), handle);
        Ok(())
    }

    /// Start the server, remove-and-reinsert it around the request so a
    /// failed request automatically drops (kills) the dead server process.
    async fn execute_request(
        &mut self,
        language: &str,
        workspace_root: &Path,
        file_path: Option<&Path>,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.ensure_started(language, workspace_root).await?;

        let mut handle = self.servers.remove(language).unwrap();

        let result = handle
            .execute_with_open(file_path, language, method, params)
            .await;

        match result {
            Ok(val) => {
                self.servers.insert(language.to_string(), handle);
                Ok(val)
            }
            Err(e) => {
                // Drop the handle (kills server) so the next call re-creates it.
                Err(e.context("LSP server error; the server will be restarted on next attempt"))
            }
        }
    }
}

// ── LspServerHandle ─────────────────────────────────────────────────────

impl LspServerHandle {
    async fn start(language: &str, workspace_root: &Path) -> Result<Self> {
        let (binary, args) = detect_server(language)?;

        let mut cmd = tokio::process::Command::new(&binary);
        cmd.args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);

        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

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

        let mut handle = Self {
            _process: process,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 1,
            initialized: false,
            opened_files: HashSet::new(),
        };

        handle.initialize(workspace_root).await?;
        Ok(handle)
    }

    async fn start_with_config(config: LspServerConfig) -> Result<Self> {
        let mut cmd = tokio::process::Command::new(&config.server_command);
        cmd.args(&config.server_args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);

        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

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

        let mut handle = Self {
            _process: process,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 1,
            initialized: false,
            opened_files: HashSet::new(),
        };

        handle.initialize(&config.root_path).await?;
        Ok(handle)
    }

    // ── LSP lifecycle ───────────────────────────────────────────────────

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
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "definition": { "linkSupport": true },
                    "references": {},
                    "documentSymbol": {
                        "hierarchicalDocumentSymbolSupport": true
                    },
                    "publishDiagnostics": { "relatedInformation": true },
                    "diagnostic": {},
                    "callHierarchy": {}
                },
                "workspace": {
                    "symbol": {},
                    "workspaceFolders": true
                }
            },
            "rootUri": &root_uri,
            "workspaceFolders": [{
                "uri": &root_uri,
                "name": ws_name
            }]
        });

        tokio::time::timeout(INIT_TIMEOUT, self.send_request("initialize", params))
            .await
            .context("Language server initialization timed out")??;

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

    // ── File management ─────────────────────────────────────────────────

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

        self.opened_files.insert(uri);
        Ok(())
    }

    /// Open the file if needed, then forward the request.
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

    // ── JSON-RPC transport ──────────────────────────────────────────────

    async fn send_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
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

        tokio::time::timeout(REQUEST_TIMEOUT, async {
            loop {
                let resp = read_message(&mut self.reader).await?;

                // Match our request ID — both integer and float representations.
                let resp_id = resp.get("id").and_then(|v| v.as_u64());
                if resp_id == Some(id) {
                    if let Some(error) = resp.get("error") {
                        let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
                        let message = error
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("Unknown error");
                        anyhow::bail!("LSP error {code}: {message}");
                    }
                    return Ok(resp.get("result").cloned().unwrap_or(json!(null)));
                }
                // Not our response — a notification or server-initiated request;
                // skip it and keep reading. A production client would dispatch
                // notifications (e.g. publishDiagnostics) here.
            }
        })
        .await
        .context("LSP request timed out")?
    }

    async fn send_notification(&mut self, method: &str, params: serde_json::Value) -> Result<()> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        write_message(&mut self.stdin, &msg).await
    }
}

// ── JSON-RPC framing (Content-Length header) ────────────────────────────

async fn write_message(stdin: &mut ChildStdin, msg: &serde_json::Value) -> Result<()> {
    let body = serde_json::to_string(msg)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    stdin.write_all(header.as_bytes()).await?;
    stdin.write_all(body.as_bytes()).await?;
    stdin.flush().await?;
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

// ── Language / server detection ─────────────────────────────────────────

/// Detect language identifier from a file path's extension.
pub fn detect_language(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()? {
        "rs" => Some("rust"),
        "ts" | "tsx" => Some("typescript"),
        "js" | "jsx" | "mjs" | "cjs" => Some("javascript"),
        "py" | "pyi" => Some("python"),
        "go" => Some("go"),
        "c" | "h" => Some("c"),
        "cpp" | "cxx" | "cc" | "hpp" | "hxx" => Some("cpp"),
        "java" => Some("java"),
        "zig" => Some("zig"),
        "lua" => Some("lua"),
        "rb" => Some("ruby"),
        "swift" => Some("swift"),
        "kt" | "kts" => Some("kotlin"),
        "css" => Some("css"),
        "scss" => Some("scss"),
        "less" => Some("less"),
        "html" | "htm" => Some("html"),
        "json" => Some("json"),
        "jsonc" => Some("jsonc"),
        "yaml" | "yml" => Some("yaml"),
        "toml" => Some("toml"),
        _ => None,
    }
}

/// Known (binary, args) candidates for each language.
fn server_candidates(language: &str) -> &'static [(&'static str, &'static [&'static str])] {
    match language {
        "rust" => &[("rust-analyzer", &[])],
        "typescript" | "javascript" => &[("typescript-language-server", &["--stdio"])],
        "python" => &[("pylsp", &[]), ("pyright-langserver", &["--stdio"])],
        "go" => &[("gopls", &[])],
        "c" | "cpp" => &[("clangd", &[])],
        "java" => &[("jdtls", &[])],
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

// ── URI helpers ─────────────────────────────────────────────────────────

/// Convert a filesystem path to a `file://` URI.
pub fn path_to_uri(path: &Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let s = abs.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

fn uri_to_path_string(uri: &str) -> String {
    uri.strip_prefix("file:///")
        .or_else(|| uri.strip_prefix("file://"))
        .unwrap_or(uri)
        .to_string()
}

// ── LSP enum helpers ────────────────────────────────────────────────────

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

// ── Diagnostic parsing ──────────────────────────────────────────────────

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
                let start_char =
                    range.get("start")?.get("character")?.as_u64().unwrap_or(0) as u32;
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
                            .unwrap_or(start_line as u64)
                            as u32,
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

// ── Response formatting ─────────────────────────────────────────────────
// These take raw JSON-RPC result values and produce human-readable strings.

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
        out.push_str(&format!("  → {path}:{line}:{col}\n"));
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
            "    → {caller_kind} '{caller_name}' in {path}:{line}\n"
        ));
    }

    out
}
