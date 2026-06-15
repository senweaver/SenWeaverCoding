// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use anyhow::Context;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::net::ToSocketAddrs;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tracing::debug;

pub use dock::{
    clear_test_target_for_tab, clear_test_target_tab, current_test_target_tab, dock_controller,
    install_dock_controller, sessions_pinned_to, set_test_target_tab,
    set_prototype_ref_tab, clear_prototype_ref_tab, current_prototype_ref_tab,
    set_prototype_ref_figma, clear_prototype_ref_figma, current_prototype_ref_figma,
    DockController, DockRequest,
    DockResponse, DockTabInfo,
};

mod dock {
    use anyhow::Result;
    use async_trait::async_trait;
    use parking_lot::RwLock;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::{Arc, OnceLock};

    #[derive(Debug, Clone)]
    pub struct DockRequest {
        pub kind: String,
        pub args: Value,
        pub timeout_ms: u64,
    }

    #[derive(Debug, Clone)]
    pub struct DockResponse {
        pub ok: bool,
        pub value: Value,
        pub error: Option<String>,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct DockTabInfo {
        pub id: u32,
        #[serde(default)]
        pub url: Option<String>,
        #[serde(default)]
        pub title: Option<String>,
        #[serde(default)]
        pub active: bool,
        #[serde(default)]
        pub owner: Option<String>,
    }

    #[async_trait]
    pub trait DockController: Send + Sync {

        async fn ensure_visible(&self, session_hint: Option<String>) -> Result<()>;

        async fn exec(&self, req: DockRequest) -> Result<DockResponse>;

        async fn screenshot(&self, full_page: bool) -> Result<Vec<u8>>;

        async fn new_tab(&self, url: Option<String>, activate: bool) -> Result<u32>;

        async fn close_tab(&self, tab_id: u32) -> Result<Option<u32>>;

        async fn activate_tab(&self, tab_id: u32) -> Result<()>;

        async fn list_tabs(&self) -> Result<Vec<DockTabInfo>>;

        async fn bind_tab_to_session(
            &self,
            _session_id: String,
            _tab_id: u32,
        ) -> Result<()> {
            Ok(())
        }

        async fn unbind_tab_from_session(
            &self,
            _session_id: String,
            _tab_id: u32,
        ) -> Result<()> {
            Ok(())
        }

        async fn release_agent_tabs_for_session(
            &self,
            _session_id: String,
        ) -> Result<Vec<u32>> {
            Ok(Vec::new())
        }

        async fn present_session(&self, _session_id: String) -> Result<Option<u32>> {
            Ok(None)
        }

        async fn park(&self) -> Result<()> {
            Ok(())
        }
    }

    static CONTROLLER: OnceLock<Arc<dyn DockController>> = OnceLock::new();

    pub fn install_dock_controller(controller: Arc<dyn DockController>) {
        let _ = CONTROLLER.set(controller);
    }

    pub fn dock_controller() -> Option<Arc<dyn DockController>> {
        CONTROLLER.get().cloned()
    }

    static TEST_TARGET_TABS: OnceLock<RwLock<HashMap<String, u32>>> = OnceLock::new();

    fn pins_slot() -> &'static RwLock<HashMap<String, u32>> {
        TEST_TARGET_TABS.get_or_init(|| RwLock::new(HashMap::new()))
    }

    fn canonical_pin_session_id(session_id: &str) -> String {
        const GW: &str = "gw_";
        let trimmed = session_id.trim();
        trimmed
            .strip_prefix(GW)
            .unwrap_or(trimmed)
            .to_string()
    }

    pub fn set_test_target_tab(session_id: &str, tab_id: u32) {
        let key = canonical_pin_session_id(session_id);
        if key.is_empty() {
            return;
        }
        pins_slot().write().insert(key, tab_id);
    }

    pub fn clear_test_target_tab(session_id: &str) {
        let key = canonical_pin_session_id(session_id);
        if key.is_empty() {
            return;
        }
        pins_slot().write().remove(&key);
    }

    pub fn current_test_target_tab(session_id: &str) -> Option<u32> {
        let key = canonical_pin_session_id(session_id);
        if key.is_empty() {
            return None;
        }
        pins_slot().read().get(&key).copied()
    }

    pub fn clear_test_target_for_tab(tab_id: u32) {
        pins_slot().write().retain(|_, t| *t != tab_id);
    }

    pub fn sessions_pinned_to(tab_id: u32) -> Vec<String> {
        pins_slot()
            .read()
            .iter()
            .filter_map(|(s, t)| if *t == tab_id { Some(s.clone()) } else { None })
            .collect()
    }

    static PROTOTYPE_REF_TABS: OnceLock<RwLock<HashMap<String, u32>>> = OnceLock::new();

    fn proto_slot() -> &'static RwLock<HashMap<String, u32>> {
        PROTOTYPE_REF_TABS.get_or_init(|| RwLock::new(HashMap::new()))
    }

    pub fn set_prototype_ref_tab(session_id: &str, tab_id: u32) {
        proto_slot().write().insert(session_id.to_string(), tab_id);
    }

    pub fn clear_prototype_ref_tab(session_id: &str) {
        proto_slot().write().remove(session_id);
    }

    pub fn current_prototype_ref_tab(session_id: &str) -> Option<u32> {
        proto_slot().read().get(session_id).copied()
    }

    static PROTOTYPE_REF_FIGMA: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();

    fn proto_figma_slot() -> &'static RwLock<HashMap<String, String>> {
        PROTOTYPE_REF_FIGMA.get_or_init(|| RwLock::new(HashMap::new()))
    }

    pub fn set_prototype_ref_figma(session_id: &str, url: &str) {
        proto_figma_slot()
            .write()
            .insert(session_id.to_string(), url.to_string());
    }

    pub fn clear_prototype_ref_figma(session_id: &str) {
        proto_figma_slot().write().remove(session_id);
    }

    pub fn current_prototype_ref_figma(session_id: &str) -> Option<String> {
        proto_figma_slot().read().get(session_id).cloned()
    }
}

#[derive(Clone)]
pub struct ComputerUseConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub api_key: Option<String>,
    pub timeout_ms: u64,
    pub allow_remote_endpoint: bool,
    pub window_allowlist: Vec<String>,
    pub max_coordinate_x: Option<i64>,
    pub max_coordinate_y: Option<i64>,
}

impl std::fmt::Debug for ComputerUseConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComputerUseConfig")
            .field("enabled", &self.enabled)
            .field("endpoint", &self.endpoint)
            .field("timeout_ms", &self.timeout_ms)
            .field("allow_remote_endpoint", &self.allow_remote_endpoint)
            .field("window_allowlist", &self.window_allowlist)
            .field("max_coordinate_x", &self.max_coordinate_x)
            .field("max_coordinate_y", &self.max_coordinate_y)
            .finish_non_exhaustive()
    }
}

impl Default for ComputerUseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: "http://127.0.0.1:8787/v1/actions".into(),
            api_key: None,
            timeout_ms: 15_000,
            allow_remote_endpoint: false,
            window_allowlist: Vec::new(),
            max_coordinate_x: None,
            max_coordinate_y: None,
        }
    }
}

#[allow(dead_code)]
pub struct BrowserTool {
    security: Arc<SecurityPolicy>,
    allowed_domains: Vec<String>,
    session_name: Option<String>,
    backend: String,
    native_headless: bool,
    native_webdriver_url: String,
    native_chrome_path: Option<String>,
    computer_use: ComputerUseConfig,
    #[cfg(feature = "browser-native")]
    native_state: tokio::sync::Mutex<native_backend::NativeBrowserState>,
    preferred_tab: tokio::sync::Mutex<Option<u32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserBackendKind {
    AgentBrowser,
    RustNative,
    ComputerUse,

    TauriDock,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedBackend {
    AgentBrowser,
    RustNative,
    ComputerUse,
    TauriDock,
}

impl BrowserBackendKind {
    fn parse(raw: &str) -> anyhow::Result<Self> {
        let key = raw.trim().to_ascii_lowercase().replace('-', "_");
        match key.as_str() {
            "agent_browser" | "agentbrowser" => Ok(Self::AgentBrowser),
            "rust_native" | "native" => Ok(Self::RustNative),
            "computer_use" | "computeruse" => Ok(Self::ComputerUse),
            "tauri_dock" | "tauridock" | "dock" | "embedded" => Ok(Self::TauriDock),
            "auto" => Ok(Self::Auto),
            _ => anyhow::bail!(
                "Unsupported browser backend '{raw}'. Use 'agent_browser', 'rust_native', 'computer_use', 'tauri_dock', or 'auto'"
            ),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::AgentBrowser => "agent_browser",
            Self::RustNative => "rust_native",
            Self::ComputerUse => "computer_use",
            Self::TauriDock => "tauri_dock",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Deserialize)]
struct AgentBrowserResponse {
    success: bool,
    data: Option<Value>,
    error: Option<String>,
}

const AGENT_BROWSER_COMMAND_TIMEOUT_MS: u64 = 120_000;

const AGENT_BROWSER_MAX_OUTPUT_BYTES: usize = 256 * 1024;

fn cap_browser_text(text: &str) -> String {
    if text.len() <= AGENT_BROWSER_MAX_OUTPUT_BYTES {
        return text.to_string();
    }
    let mut end = AGENT_BROWSER_MAX_OUTPUT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[output truncated: kept first {} of {} bytes]",
        &text[..end],
        end,
        text.len()
    )
}

fn cap_agent_browser_response(resp: AgentBrowserResponse) -> AgentBrowserResponse {
    let Some(data) = resp.data else {
        return resp;
    };
    let serialized = serde_json::to_string(&data).unwrap_or_default();
    if serialized.len() <= AGENT_BROWSER_MAX_OUTPUT_BYTES {
        return AgentBrowserResponse {
            success: resp.success,
            data: Some(data),
            error: resp.error,
        };
    }
    let pretty = serde_json::to_string_pretty(&data).unwrap_or(serialized);
    AgentBrowserResponse {
        success: resp.success,
        data: Some(json!({
            "truncated": true,
            "original_bytes": pretty.len(),
            "note": format!(
                "agent-browser output exceeded {} bytes; the head of the serialized payload is kept as text",
                AGENT_BROWSER_MAX_OUTPUT_BYTES
            ),
            "output": cap_browser_text(&pretty),
        })),
        error: resp.error,
    }
}

#[derive(Debug, Deserialize)]
struct ComputerUseResponse {
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserAction {

    Open { url: String },

    Snapshot {
        #[serde(default)]
        interactive_only: bool,
        #[serde(default)]
        compact: bool,
        #[serde(default)]
        depth: Option<u32>,
    },

    Click { selector: String },

    Fill { selector: String, value: String },

    Type { selector: String, text: String },

    GetText { selector: String },

    GetStyles {
        selector: Option<String>,
        limit: Option<u64>,
    },

    GetTitle,

    GetUrl,

    Screenshot {
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        full_page: bool,
    },

    Wait {
        #[serde(default)]
        selector: Option<String>,
        #[serde(default)]
        ms: Option<u64>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        until: Option<String>,
    },

    Press { key: String },

    Hover { selector: String },

    Scroll {
        direction: String,
        #[serde(default)]
        pixels: Option<u32>,
    },

    IsVisible { selector: String },

    Close,

    Find {
        by: String,
        value: String,
        action: String,
        #[serde(default)]
        fill_value: Option<String>,
    },

    OpenTab {
        #[serde(default)]
        url: Option<String>,
        #[serde(default = "default_activate")]
        activate: bool,
    },

    CloseTab { tab: u32 },

    ActivateTab { tab: u32 },

    ListTabs,

    Assert {
        kind: String,
        #[serde(default)]
        selector: Option<String>,
        #[serde(default)]
        expected: Option<String>,
        #[serde(default)]
        attribute: Option<String>,
        #[serde(default)]
        op: Option<String>,
        #[serde(default)]
        count: Option<i64>,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },

    ConsoleLogs {
        #[serde(default)]
        level: Option<String>,
        #[serde(default)]
        since_ms: Option<u64>,
        #[serde(default)]
        clear_after: bool,
        #[serde(default)]
        limit: Option<u64>,
    },

    NetworkIdle {
        #[serde(default)]
        idle_ms: Option<u64>,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },

    ClearStorage {
        #[serde(default)]
        scope: Option<String>,
        #[serde(default)]
        force: bool,
    },

    Back,

    Forward,

    Reload,

    AttachTab {
        tab_id: u32,
    },

    CollectLinks {
        #[serde(default)]
        same_origin: Option<bool>,
        #[serde(default)]
        limit: Option<u64>,
    },

    NetworkErrors {
        #[serde(default)]
        since_ms: Option<u64>,
        #[serde(default)]
        limit: Option<u64>,
    },

    PinTestTarget { tab_id: u32 },

    ClearTestTarget,

    GetTestTarget,

    PerfVitals,

    Emulate {
        #[serde(default)]
        viewport: Option<Value>,
        #[serde(default)]
        network: Option<String>,
        #[serde(default)]
        cpu_rate: Option<f64>,
        #[serde(default)]
        reset: bool,
    },

    NetworkCapture {
        #[serde(default)]
        mode: Option<String>,
        #[serde(default)]
        request_id: Option<String>,
        #[serde(default)]
        limit: Option<u64>,
        #[serde(default)]
        url_contains: Option<String>,
        #[serde(default)]
        only_failures: bool,
        #[serde(default)]
        api_only: bool,
    },

    WebToolsList,

    WebToolsCall {
        name: String,
        #[serde(default)]
        tool_args: Option<Value>,
    },

    RunSteps { steps: Vec<Value> },
}

fn default_activate() -> bool {
    true
}

impl BrowserTool {
    async fn execute_run_steps(&self, args: Value) -> anyhow::Result<ToolResult> {
        let steps = args
            .get("steps")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if steps.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("run_steps requires a non-empty 'steps' array".into()),
            });
        }
        if steps.len() > 20 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("run_steps supports at most 20 steps per call".into()),
            });
        }
        let continue_on_error = args
            .get("continue_on_error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let parent_tab = args.get("tab_id").cloned();
        let mut results: Vec<Value> = Vec::new();
        let mut all_ok = true;
        for (idx, step) in steps.iter().enumerate() {
            let action_name = step
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !step.is_object() || action_name.is_empty() {
                all_ok = false;
                results.push(json!({
                    "index": idx,
                    "success": false,
                    "error": "each step must be an object with an 'action' field",
                }));
                if !continue_on_error {
                    break;
                }
                continue;
            }
            if action_name == "run_steps" {
                all_ok = false;
                results.push(json!({
                    "index": idx,
                    "action": "run_steps",
                    "success": false,
                    "error": "nested run_steps is not allowed",
                }));
                if !continue_on_error {
                    break;
                }
                continue;
            }
            let mut step_args = step.clone();
            if let (Some(obj), Some(tab)) = (step_args.as_object_mut(), parent_tab.as_ref()) {
                obj.entry("tab_id".to_string()).or_insert_with(|| tab.clone());
            }
            let outcome = Box::pin(Tool::execute(self, step_args)).await;
            match outcome {
                Ok(res) => {
                    if !res.success {
                        all_ok = false;
                    }
                    let mut output = res.output;
                    if output.len() > 4_000 {
                        let mut cut = 4_000;
                        while !output.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        output.truncate(cut);
                        output.push_str("\n[truncated]");
                    }
                    let failed = !res.success;
                    results.push(json!({
                        "index": idx,
                        "action": action_name,
                        "success": res.success,
                        "output": output,
                        "error": res.error,
                    }));
                    if failed && !continue_on_error {
                        break;
                    }
                }
                Err(err) => {
                    all_ok = false;
                    results.push(json!({
                        "index": idx,
                        "action": action_name,
                        "success": false,
                        "error": err.to_string(),
                    }));
                    if !continue_on_error {
                        break;
                    }
                }
            }
        }
        let summary = json!({
            "steps_total": steps.len(),
            "steps_executed": results.len(),
            "all_passed": all_ok,
            "results": results,
        });
        Ok(ToolResult {
            success: all_ok,
            output: serde_json::to_string_pretty(&summary).unwrap_or_default(),
            error: if all_ok {
                None
            } else {
                Some("one or more steps failed".into())
            },
        })
    }

    pub fn new(
        security: Arc<SecurityPolicy>,
        allowed_domains: Vec<String>,
        session_name: Option<String>,
    ) -> Self {
        Self::new_with_backend(
            security,
            allowed_domains,
            session_name,
            "agent_browser".into(),
            true,
            "http://127.0.0.1:9515".into(),
            None,
            ComputerUseConfig::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_backend(
        security: Arc<SecurityPolicy>,
        allowed_domains: Vec<String>,
        session_name: Option<String>,
        backend: String,
        native_headless: bool,
        native_webdriver_url: String,
        native_chrome_path: Option<String>,
        computer_use: ComputerUseConfig,
    ) -> Self {
        Self {
            security,
            allowed_domains: normalize_domains(allowed_domains),
            session_name,
            backend,
            native_headless,
            native_webdriver_url,
            native_chrome_path,
            computer_use,
            #[cfg(feature = "browser-native")]
            native_state: tokio::sync::Mutex::new(native_backend::NativeBrowserState::default()),
            preferred_tab: tokio::sync::Mutex::new(None),
        }
    }

    pub async fn is_agent_browser_available() -> bool {
        let cmd = if cfg!(target_os = "windows") {
            "agent-browser.cmd"
        } else {
            "agent-browser"
        };
        let mut command = crate::util::hidden_async_command(cmd);
        command
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => return false,
        };
        match tokio::time::timeout(std::time::Duration::from_secs(10), child.wait()).await {
            Ok(Ok(status)) => status.success(),
            Ok(Err(_)) => false,
            Err(_) => {
                let _ = child.start_kill();
                let _ =
                    tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await;
                false
            }
        }
    }

    pub async fn is_available() -> bool {
        Self::is_agent_browser_available().await
    }

    fn configured_backend(&self) -> anyhow::Result<BrowserBackendKind> {
        BrowserBackendKind::parse(&self.backend)
    }

    fn rust_native_compiled() -> bool {
        cfg!(feature = "browser-native")
    }

    fn rust_native_available(&self) -> bool {
        #[cfg(feature = "browser-native")]
        {
            native_backend::NativeBrowserState::is_available(
                self.native_headless,
                &self.native_webdriver_url,
                self.native_chrome_path.as_deref(),
            )
        }
        #[cfg(not(feature = "browser-native"))]
        {
            false
        }
    }

    fn computer_use_endpoint_url(&self) -> anyhow::Result<reqwest::Url> {
        if self.computer_use.timeout_ms == 0 {
            anyhow::bail!("browser.computer_use.timeout_ms must be > 0");
        }

        let endpoint = self.computer_use.endpoint.trim();
        if endpoint.is_empty() {
            anyhow::bail!("browser.computer_use.endpoint cannot be empty");
        }

        let parsed = reqwest::Url::parse(endpoint).map_err(|_| {
            anyhow::anyhow!(
                "Invalid browser.computer_use.endpoint: '{endpoint}'. Expected http(s) URL"
            )
        })?;

        let scheme = parsed.scheme();
        if scheme != "http" && scheme != "https" {
            anyhow::bail!("browser.computer_use.endpoint must use http:// or https://");
        }

        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("browser.computer_use.endpoint must include host"))?;

        let host_is_private = is_private_host(host);
        if !self.computer_use.allow_remote_endpoint && !host_is_private {
            anyhow::bail!(
                "browser.computer_use.endpoint host '{host}' is public. Set browser.computer_use.allow_remote_endpoint=true to allow it"
            );
        }

        if self.computer_use.allow_remote_endpoint && !host_is_private && scheme != "https" {
            anyhow::bail!(
                "browser.computer_use.endpoint must use https:// when allow_remote_endpoint=true and host is public"
            );
        }

        Ok(parsed)
    }

    fn computer_use_available(&self) -> anyhow::Result<bool> {
        let endpoint = self.computer_use_endpoint_url()?;
        Ok(endpoint_reachable(&endpoint, Duration::from_millis(500)))
    }

    async fn resolve_backend(&self) -> anyhow::Result<ResolvedBackend> {
        let configured = self.configured_backend()?;

        match configured {
            BrowserBackendKind::TauriDock => {
                if dock_controller().is_none() {
                    anyhow::bail!(
                        "browser.backend='tauri_dock' but no embedded dock controller is registered. \
                         This backend is only available inside the Tauri desktop shell."
                    );
                }
                Ok(ResolvedBackend::TauriDock)
            }
            BrowserBackendKind::AgentBrowser => {
                if Self::is_agent_browser_available().await {
                    Ok(ResolvedBackend::AgentBrowser)
                } else {
                    #[cfg(target_os = "windows")]
                    let install_hint = "Install with: npm install -g agent-browser (ensure npm global bin is in PATH)";
                    #[cfg(not(target_os = "windows"))]
                    let install_hint = "Install with: npm install -g agent-browser";
                    anyhow::bail!(
                        "browser.backend='{}' but agent-browser CLI is unavailable. {}",
                        configured.as_str(),
                        install_hint
                    )
                }
            }
            BrowserBackendKind::RustNative => {
                if !Self::rust_native_compiled() {
                    anyhow::bail!(
                        "browser.backend='rust_native' requires build feature 'browser-native'"
                    );
                }
                if !self.rust_native_available() {
                    anyhow::bail!(
                        "Rust-native browser backend is enabled but WebDriver endpoint is unreachable. Set browser.native_webdriver_url and start a compatible driver"
                    );
                }
                Ok(ResolvedBackend::RustNative)
            }
            BrowserBackendKind::ComputerUse => {
                if !self.computer_use.enabled {
                    anyhow::bail!(
                        "browser.backend='computer_use' but Computer Use is disabled. Enable it in Settings → Computer Use"
                    );
                }
                if !self.computer_use_available()? {
                    anyhow::bail!(
                        "browser.backend='computer_use' but sidecar endpoint is unreachable. Check browser.computer_use.endpoint and sidecar status"
                    );
                }
                Ok(ResolvedBackend::ComputerUse)
            }
            BrowserBackendKind::Auto => {
                if dock_controller().is_some() {
                    return Ok(ResolvedBackend::TauriDock);
                }
                if Self::rust_native_compiled() && self.rust_native_available() {
                    return Ok(ResolvedBackend::RustNative);
                }
                if Self::is_agent_browser_available().await {
                    return Ok(ResolvedBackend::AgentBrowser);
                }

                let computer_use_err = if !self.computer_use.enabled {
                    None
                } else {
                    match self.computer_use_available() {
                        Ok(true) => return Ok(ResolvedBackend::ComputerUse),
                        Ok(false) => None,
                        Err(err) => Some(err.to_string()),
                    }
                };

                if Self::rust_native_compiled() {
                    if let Some(err) = computer_use_err {
                        anyhow::bail!(
                            "browser.backend='auto' found no usable backend (agent-browser missing, rust-native unavailable, computer-use invalid: {err})"
                        );
                    }
                    anyhow::bail!(
                        "browser.backend='auto' found no usable backend (agent-browser missing, rust-native unavailable, computer-use sidecar unreachable)"
                    )
                }

                if let Some(err) = computer_use_err {
                    anyhow::bail!(
                        "browser.backend='auto' needs agent-browser CLI, browser-native, or valid computer-use sidecar (error: {err})"
                    );
                }

                anyhow::bail!(
                    "browser.backend='auto' needs agent-browser CLI, browser-native, or computer-use sidecar"
                )
            }
        }
    }

    fn validate_url(&self, url: &str, permissive: bool) -> anyhow::Result<()> {
        let url = url.trim();

        if url.is_empty() {
            anyhow::bail!("URL cannot be empty");
        }

        if url.starts_with("file://") {
            if permissive {
                return Ok(());
            }
            anyhow::bail!(
                "file:// URLs are blocked by browser security policy. \
                 Disable [autonomy].enable_command_policy or use the embedded dock backend \
                 to preview local HTML files."
            );
        }

        if !url.starts_with("https://") && !url.starts_with("http://") {
            anyhow::bail!("Only http://, https:// and file:// URLs are allowed");
        }

        let host = extract_host(url)?;

        if is_loopback_host(&host) {
            if permissive {
                return Ok(());
            }
            anyhow::bail!(
                "Loopback host '{host}' is blocked by browser security policy. \
                 Disable [autonomy].enable_command_policy or call this URL through the \
                 embedded dock backend (browser.backend='auto' inside the desktop app)."
            );
        }

        if self.allowed_domains.is_empty() {
            anyhow::bail!(
                "Browser tool enabled but no allowed_domains configured. \
                Add [browser].allowed_domains in config.toml"
            );
        }

        if is_private_host(&host) {
            if permissive {
                return Ok(());
            }
            anyhow::bail!("Blocked local/private host: {host}");
        }

        if !host_matches_allowlist(&host, &self.allowed_domains) {
            anyhow::bail!("Host '{host}' not in browser.allowed_domains");
        }

        Ok(())
    }

    fn url_validation_permissive(&self) -> bool {
        !self.security.is_command_policy_enabled()
    }

    async fn run_command(&self, args: &[&str]) -> anyhow::Result<AgentBrowserResponse> {
        self.run_command_with_timeout(args, AGENT_BROWSER_COMMAND_TIMEOUT_MS)
            .await
    }

    async fn run_command_with_timeout(
        &self,
        args: &[&str],
        timeout_ms: u64,
    ) -> anyhow::Result<AgentBrowserResponse> {
        let agent_browser_bin = if cfg!(target_os = "windows") {
            "agent-browser.cmd"
        } else {
            "agent-browser"
        };
        let mut cmd = crate::util::hidden_async_command(agent_browser_bin);

        if is_service_environment() {
            ensure_browser_env(&mut cmd);
        }

        if let Some(ref session) = self.session_name {
            cmd.arg("--session").arg(session);
        }

        cmd.args(args).arg("--json");

        debug!("Running: agent-browser {} --json", args.join(" "));

        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let output =
            match tokio::time::timeout(Duration::from_millis(timeout_ms), cmd.output()).await {
                Ok(out) => out?,
                Err(_) => {
                    return Ok(AgentBrowserResponse {
                        success: false,
                        data: None,
                        error: Some(format!(
                            "agent-browser command '{}' timed out after {}ms and was killed",
                            args.join(" "),
                            timeout_ms
                        )),
                    });
                }
            };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !stderr.is_empty() {
            debug!("agent-browser stderr: {}", stderr);
        }

        if let Ok(resp) = serde_json::from_str::<AgentBrowserResponse>(&stdout) {
            return Ok(cap_agent_browser_response(resp));
        }

        if output.status.success() {
            Ok(AgentBrowserResponse {
                success: true,
                data: Some(json!({ "output": cap_browser_text(stdout.trim()) })),
                error: None,
            })
        } else {
            Ok(AgentBrowserResponse {
                success: false,
                data: None,
                error: Some(stderr.trim().to_string()),
            })
        }
    }

    #[allow(clippy::too_many_lines)]
    async fn execute_agent_browser_action(
        &self,
        action: BrowserAction,
    ) -> anyhow::Result<ToolResult> {
        match action {
            BrowserAction::Open { url } => {
                self.validate_url(&url, self.url_validation_permissive())?;
                let resp = self.run_command(&["open", &url]).await?;
                self.to_result(resp)
            }

            BrowserAction::Snapshot {
                interactive_only,
                compact,
                depth,
            } => {
                let mut args = vec!["snapshot"];
                if interactive_only {
                    args.push("-i");
                }
                if compact {
                    args.push("-c");
                }
                let depth_str;
                if let Some(d) = depth {
                    args.push("-d");
                    depth_str = d.to_string();
                    args.push(&depth_str);
                }
                let resp = self.run_command(&args).await?;
                self.to_result(resp)
            }

            BrowserAction::Click { selector } => {
                let resp = self.run_command(&["click", &selector]).await?;
                self.to_result(resp)
            }

            BrowserAction::Fill { selector, value } => {
                let resp = self.run_command(&["fill", &selector, &value]).await?;
                self.to_result(resp)
            }

            BrowserAction::Type { selector, text } => {
                let resp = self.run_command(&["type", &selector, &text]).await?;
                self.to_result(resp)
            }

            BrowserAction::GetText { selector } => {
                let resp = self.run_command(&["get", "text", &selector]).await?;
                self.to_result(resp)
            }

            BrowserAction::GetTitle => {
                let resp = self.run_command(&["get", "title"]).await?;
                self.to_result(resp)
            }

            BrowserAction::GetUrl => {
                let resp = self.run_command(&["get", "url"]).await?;
                self.to_result(resp)
            }

            BrowserAction::Screenshot { path, full_page } => {
                let mut args = vec!["screenshot"];
                let abs_holder;
                if let Some(ref p) = path {
                    let anchor = self.security.safe_artifact_anchor();
                    let (abs_path, _relative_path) = resolve_screenshot_path(p, &anchor)?;
                    if let Some(parent) = abs_path.parent() {
                        if !parent.as_os_str().is_empty() {
                            tokio::fs::create_dir_all(parent).await.with_context(|| {
                                format!("failed to create screenshot dir {}", parent.display())
                            })?;
                        }
                    }
                    abs_holder = abs_path.to_string_lossy().to_string();
                    args.push(&abs_holder);
                }
                if full_page {
                    args.push("--full");
                }
                let resp = self.run_command(&args).await?;
                self.to_result(resp)
            }

            BrowserAction::Wait {
                selector,
                ms,
                text,
                until: _,
            } => {
                let mut args = vec!["wait"];
                let ms_str;
                let mut command_timeout_ms = AGENT_BROWSER_COMMAND_TIMEOUT_MS;
                if let Some(sel) = selector.as_ref() {
                    args.push(sel);
                } else if let Some(millis) = ms {
                    ms_str = millis.to_string();
                    args.push(&ms_str);
                    command_timeout_ms =
                        command_timeout_ms.max(millis.saturating_add(30_000));
                } else if let Some(ref t) = text {
                    args.push("--text");
                    args.push(t);
                }
                let resp = self
                    .run_command_with_timeout(&args, command_timeout_ms)
                    .await?;
                self.to_result(resp)
            }

            BrowserAction::Press { key } => {
                let resp = self.run_command(&["press", &key]).await?;
                self.to_result(resp)
            }

            BrowserAction::Hover { selector } => {
                let resp = self.run_command(&["hover", &selector]).await?;
                self.to_result(resp)
            }

            BrowserAction::Scroll { direction, pixels } => {
                let mut args = vec!["scroll", &direction];
                let px_str;
                if let Some(px) = pixels {
                    px_str = px.to_string();
                    args.push(&px_str);
                }
                let resp = self.run_command(&args).await?;
                self.to_result(resp)
            }

            BrowserAction::IsVisible { selector } => {
                let resp = self.run_command(&["is", "visible", &selector]).await?;
                self.to_result(resp)
            }

            BrowserAction::Close => {
                let resp = self.run_command(&["close"]).await?;
                self.to_result(resp)
            }

            BrowserAction::Find {
                by,
                value,
                action,
                fill_value,
            } => {
                let mut args = vec!["find", &by, &value, &action];
                if let Some(ref fv) = fill_value {
                    args.push(fv);
                }
                let resp = self.run_command(&args).await?;
                self.to_result(resp)
            }

            BrowserAction::OpenTab { .. }
            | BrowserAction::CloseTab { .. }
            | BrowserAction::ActivateTab { .. }
            | BrowserAction::ListTabs
            | BrowserAction::AttachTab { .. } => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "Multi-tab actions (open_tab/close_tab/activate_tab/list_tabs/attach_tab) are only \
                     supported by the embedded dock backend (tauri_dock). Run inside the \
                     SenAgentOS desktop app or switch to backend='tauri_dock'."
                        .to_string(),
                ),
            }),
            BrowserAction::Assert { .. }
            | BrowserAction::ConsoleLogs { .. }
            | BrowserAction::NetworkIdle { .. }
            | BrowserAction::ClearStorage { .. }
            | BrowserAction::Back
            | BrowserAction::Forward
            | BrowserAction::Reload
            | BrowserAction::CollectLinks { .. }
            | BrowserAction::NetworkErrors { .. }
            | BrowserAction::GetStyles { .. }
            | BrowserAction::PerfVitals
            | BrowserAction::Emulate { .. }
            | BrowserAction::NetworkCapture { .. }
            | BrowserAction::WebToolsList
            | BrowserAction::WebToolsCall { .. }
            | BrowserAction::RunSteps { .. } => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "QA actions (assert/console_logs/network_idle/clear_storage/back/forward/reload/collect_links/network_errors/get_styles/perf_vitals/emulate/network_capture/web_tools_list/web_tools_call/run_steps) require the \
                     embedded dock backend (tauri_dock). Run inside the SenAgentOS desktop app."
                        .to_string(),
                ),
            }),
            BrowserAction::PinTestTarget { .. }
            | BrowserAction::ClearTestTarget
            | BrowserAction::GetTestTarget => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "Test-target actions (pin_test_target/clear_test_target/get_test_target) require the embedded dock backend (tauri_dock). \
                     Run inside the SenAgentOS desktop app."
                        .to_string(),
                ),
            }),
        }
    }

    #[allow(clippy::unused_async)]
    async fn execute_rust_native_action(
        &self,
        action: BrowserAction,
    ) -> anyhow::Result<ToolResult> {
        #[cfg(feature = "browser-native")]
        {
            if let BrowserAction::Open { url } = &action {
                self.validate_url(url, self.url_validation_permissive())?;
            }

            let mut state = self.native_state.lock().await;

            let first_attempt = state
                .execute_action(
                    action.clone(),
                    self.native_headless,
                    &self.native_webdriver_url,
                    self.native_chrome_path.as_deref(),
                )
                .await;

            let output = match first_attempt {
                Ok(output) => output,
                Err(err) => {
                    if !is_recoverable_rust_native_error(&err) {
                        return Err(err);
                    }

                    state.reset_session().await;
                    state
                        .execute_action(
                            action,
                            self.native_headless,
                            &self.native_webdriver_url,
                            self.native_chrome_path.as_deref(),
                        )
                        .await
                        .with_context(|| "rust_native backend retry after session reset failed")?
                }
            };

            Ok(ToolResult {
                success: true,
                output: cap_browser_text(&serde_json::to_string_pretty(&output).unwrap_or_default()),
                error: None,
            })
        }

        #[cfg(not(feature = "browser-native"))]
        {
            let _ = action;
            anyhow::bail!(
                "Rust-native browser backend is not compiled. Rebuild with --features browser-native"
            )
        }
    }

    fn validate_coordinate(&self, key: &str, value: i64, max: Option<i64>) -> anyhow::Result<()> {
        if value < 0 {
            anyhow::bail!("'{key}' must be >= 0")
        }
        if let Some(limit) = max {
            if limit < 0 {
                anyhow::bail!("Configured coordinate limit for '{key}' must be >= 0")
            }
            if value > limit {
                anyhow::bail!("'{key}'={value} exceeds configured limit {limit}")
            }
        }
        Ok(())
    }

    fn read_required_i64(
        &self,
        params: &serde_json::Map<String, Value>,
        key: &str,
    ) -> anyhow::Result<i64> {
        params
            .get(key)
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid '{key}' parameter"))
    }

    fn validate_computer_use_action(
        &self,
        action: &str,
        params: &serde_json::Map<String, Value>,
    ) -> anyhow::Result<()> {
        match action {
            "open" => {
                let url = params
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("Missing 'url' for open action"))?;
                self.validate_url(url, self.url_validation_permissive())?;
            }
            "mouse_move" | "mouse_click" => {
                let x = self.read_required_i64(params, "x")?;
                let y = self.read_required_i64(params, "y")?;
                self.validate_coordinate("x", x, self.computer_use.max_coordinate_x)?;
                self.validate_coordinate("y", y, self.computer_use.max_coordinate_y)?;
            }
            "mouse_drag" => {
                let from_x = self.read_required_i64(params, "from_x")?;
                let from_y = self.read_required_i64(params, "from_y")?;
                let to_x = self.read_required_i64(params, "to_x")?;
                let to_y = self.read_required_i64(params, "to_y")?;
                self.validate_coordinate("from_x", from_x, self.computer_use.max_coordinate_x)?;
                self.validate_coordinate("to_x", to_x, self.computer_use.max_coordinate_x)?;
                self.validate_coordinate("from_y", from_y, self.computer_use.max_coordinate_y)?;
                self.validate_coordinate("to_y", to_y, self.computer_use.max_coordinate_y)?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn execute_computer_use_action(
        &self,
        action: &str,
        args: &Value,
    ) -> anyhow::Result<ToolResult> {
        let endpoint = self.computer_use_endpoint_url()?;

        let mut params = args
            .as_object()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("browser args must be a JSON object"))?;
        params.remove("action");

        self.validate_computer_use_action(action, &params)?;

        let payload = json!({
            "action": action,
            "params": params,
            "policy": {
                "allowed_domains": self.allowed_domains,
                "window_allowlist": self.computer_use.window_allowlist,
                "max_coordinate_x": self.computer_use.max_coordinate_x,
                "max_coordinate_y": self.computer_use.max_coordinate_y,
            },
            "metadata": {
                "session_name": self.session_name,
                "source": "sen.browser",
                "version": env!("CARGO_PKG_VERSION"),
            }
        });

        let client = crate::services::require_services()
            .proxy_runtime()
            .build_client("tool.browser");
        let mut request = client
            .post(endpoint)
            .timeout(Duration::from_millis(self.computer_use.timeout_ms))
            .json(&payload);

        if let Some(api_key) = self.computer_use.api_key.as_deref() {
            let token = api_key.trim();
            if !token.is_empty() {
                request = request.bearer_auth(token);
            }
        }

        let response = request.send().await.with_context(|| {
            format!(
                "Failed to call computer-use sidecar at {}",
                self.computer_use.endpoint
            )
        })?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read computer-use sidecar response body")?;

        if let Ok(parsed) = serde_json::from_str::<ComputerUseResponse>(&body) {
            if status.is_success() && parsed.success.unwrap_or(true) {
                let output = parsed
                    .data
                    .map(|data| serde_json::to_string_pretty(&data).unwrap_or_default())
                    .unwrap_or_else(|| {
                        serde_json::to_string_pretty(&json!({
                            "backend": "computer_use",
                            "action": action,
                            "ok": true,
                        }))
                        .unwrap_or_default()
                    });

                return Ok(ToolResult {
                    success: true,
                    output,
                    error: None,
                });
            }

            let error = parsed.error.or_else(|| {
                if status.is_success() && parsed.success == Some(false) {
                    Some("computer-use sidecar returned success=false".to_string())
                } else {
                    Some(format!(
                        "computer-use sidecar request failed with status {status}"
                    ))
                }
            });

            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error,
            });
        }

        if status.is_success() {
            return Ok(ToolResult {
                success: true,
                output: body,
                error: None,
            });
        }

        Ok(ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!(
                "computer-use sidecar request failed with status {status}: {}",
                body.trim()
            )),
        })
    }

    async fn execute_action(
        &self,
        action: BrowserAction,
        backend: ResolvedBackend,
        request_tab_id: Option<u32>,
    ) -> anyhow::Result<ToolResult> {
        match backend {
            ResolvedBackend::AgentBrowser => self.execute_agent_browser_action(action).await,
            ResolvedBackend::RustNative => self.execute_rust_native_action(action).await,
            ResolvedBackend::TauriDock => {
                self.execute_tauri_dock_action(action, request_tab_id).await
            }
            ResolvedBackend::ComputerUse => anyhow::bail!(
                "Internal error: computer_use backend must be handled before BrowserAction parsing"
            ),
        }
    }

    #[allow(clippy::too_many_lines)]
    async fn execute_tauri_dock_action(
        &self,
        action: BrowserAction,
        request_tab_id: Option<u32>,
    ) -> anyhow::Result<ToolResult> {
        let controller = dock_controller().ok_or_else(|| {
            anyhow::anyhow!("Internal error: tauri_dock backend selected but controller is gone")
        })?;
        let in_designer = matches!(
            crate::agent::coding_mode::active_coding_mode(),
            crate::agent::coding_mode::CodingMode::Designer
        );
        if !in_designer || action_opens_external_url(&action) {
            let _ = controller
                .ensure_visible(self.session_name.clone())
                .await;
        } else {
            let _ = controller.park().await;
        }

        const DEFAULT_TIMEOUT_MS: u64 = 30_000;

        let preferred = *self.preferred_tab.lock().await;
        let session_id_opt: Option<String> =
            crate::session::current_session_context().map(|c| c.session_id);
        let effective_tab_id = request_tab_id
            .or(preferred)
            .or_else(|| session_id_opt.as_deref().and_then(current_test_target_tab));

        match &action {
            BrowserAction::PinTestTarget { tab_id } => {

                let tabs = controller
                    .list_tabs()
                    .await
                    .with_context(|| "tauri_dock list_tabs failed for pin_test_target")?;
                let Some(info) = tabs.into_iter().find(|t| t.id == *tab_id) else {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "pin_test_target: tab_id {tab_id} not found. Run action=list_tabs to discover available tabs."
                        )),
                    });
                };
                if let Some(sid) = session_id_opt.as_deref() {
                    set_test_target_tab(sid, *tab_id);
                }
                {
                    let mut guard = self.preferred_tab.lock().await;
                    *guard = Some(*tab_id);
                }
                let owner = info.owner.clone();
                return Ok(dock_ok_result(
                    "pin_test_target",
                    json!({
                        "tab_id": tab_id,
                        "url": info.url,
                        "title": info.title,
                        "owner": owner.unwrap_or_else(|| "agent".to_string()),
                    }),
                ));
            }
            BrowserAction::ClearTestTarget => {
                if let Some(sid) = session_id_opt.as_deref() {
                    clear_test_target_tab(sid);
                }
                {
                    let mut guard = self.preferred_tab.lock().await;
                    *guard = None;
                }
                return Ok(dock_ok_result(
                    "clear_test_target",
                    json!({ "cleared": true }),
                ));
            }
            BrowserAction::GetTestTarget => {
                let pinned = session_id_opt.as_deref().and_then(current_test_target_tab);
                let local_pref = preferred;
                let resolved = pinned.or(local_pref);
                let mut payload = json!({
                    "tab_id": resolved,
                    "session_pinned": pinned,
                    "preferred_tab": local_pref,
                });
                if let Some(tab_id) = resolved {
                    if let Ok(tabs) = controller.list_tabs().await {
                        if let Some(info) = tabs.into_iter().find(|t| t.id == tab_id) {
                            payload["url"] = serde_json::Value::from(info.url);
                            payload["title"] = serde_json::Value::from(info.title);
                            payload["owner"] = serde_json::Value::from(
                                info.owner.unwrap_or_else(|| "agent".to_string()),
                            );
                        }
                    }
                }
                return Ok(dock_ok_result("get_test_target", payload));
            }
            BrowserAction::OpenTab { url, activate } => {
                if let Some(url) = url.as_ref() {
                    self.validate_url(url, true)?;
                }
                let new_id = controller
                    .new_tab(url.clone(), *activate)
                    .await
                    .with_context(|| "tauri_dock new_tab failed")?;
                return Ok(dock_ok_result(
                    "open_tab",
                    json!({ "tab": new_id, "url": url, "activate": activate }),
                ));
            }
            BrowserAction::CloseTab { tab } => {
                let new_active = controller
                    .close_tab(*tab)
                    .await
                    .with_context(|| "tauri_dock close_tab failed")?;
                return Ok(dock_ok_result(
                    "close_tab",
                    json!({ "closed": tab, "active": new_active }),
                ));
            }
            BrowserAction::ActivateTab { tab } => {
                controller
                    .activate_tab(*tab)
                    .await
                    .with_context(|| "tauri_dock activate_tab failed")?;
                return Ok(dock_ok_result(
                    "activate_tab",
                    json!({ "active": tab }),
                ));
            }
            BrowserAction::ListTabs => {
                let tabs = controller
                    .list_tabs()
                    .await
                    .with_context(|| "tauri_dock list_tabs failed")?;
                let active_id = tabs.iter().find(|t| t.active).map(|t| t.id);
                let tabs_json: Vec<Value> = tabs
                    .iter()
                    .map(|t| {
                        json!({
                            "tab_id": t.id,
                            "id": t.id,
                            "url": t.url,
                            "title": t.title,
                            "is_active": t.active,
                            "active": t.active,
                            "owner": t.owner.clone().unwrap_or_else(|| "agent".to_string()),
                        })
                    })
                    .collect();
                return Ok(dock_ok_result(
                    "list_tabs",
                    json!({ "tabs": tabs_json, "active_tab_id": active_id }),
                ));
            }
            BrowserAction::AttachTab { tab_id } => {
                let tabs = controller
                    .list_tabs()
                    .await
                    .with_context(|| "tauri_dock list_tabs failed")?;
                let Some(info) = tabs.into_iter().find(|t| t.id == *tab_id) else {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "attach_tab: tab_id {tab_id} not found. Run action=list_tabs to discover available tabs."
                        )),
                    });
                };
                controller
                    .activate_tab(*tab_id)
                    .await
                    .with_context(|| "tauri_dock activate_tab failed")?;
                {
                    let mut guard = self.preferred_tab.lock().await;
                    *guard = Some(*tab_id);
                }
                if let Some(sid) = session_id_opt.as_deref() {
                    set_test_target_tab(sid, *tab_id);
                }
                let owner = info.owner.clone();
                let is_user_tab = owner.as_deref() == Some("user");
                let mut payload = json!({
                    "attached": tab_id,
                    "tab_id": tab_id,
                    "url": info.url,
                    "title": info.title,
                    "owner": owner.clone().unwrap_or_else(|| "agent".to_string()),
                    "pinned_as_test_target": true,
                });
                if is_user_tab {
                    payload["takeover"] = Value::Bool(true);
                }
                return Ok(dock_ok_result("attach_tab", payload));
            }
            _ => {}
        }

        if let BrowserAction::Screenshot { path, full_page } = &action {
            let png = controller
                .screenshot(*full_page)
                .await
                .with_context(|| "tauri_dock screenshot failed")?;
            if let Some(target) = path.as_ref() {
                let anchor = self.security.safe_artifact_anchor();
                let (abs_path, relative_path) = resolve_screenshot_path(target, &anchor)?;
                if let Some(parent) = abs_path.parent() {
                    if !parent.as_os_str().is_empty() {
                        tokio::fs::create_dir_all(parent).await.with_context(|| {
                            format!("failed to create screenshot dir {}", parent.display())
                        })?;
                    }
                }
                tokio::fs::write(&abs_path, &png).await.with_context(|| {
                    format!("failed to write screenshot to {}", abs_path.display())
                })?;
                let mut payload = json!({
                    "path": relative_path,
                    "saved_to": abs_path.to_string_lossy(),
                    "bytes": png.len(),
                    "full_page": full_page,
                });
                if target.starts_with("auto://") {
                    payload["auto"] = Value::Bool(true);
                }
                let res = dock_ok_result("screenshot", payload);
                return Ok(self
                    .decorate_for_effective_tab(controller.as_ref(), res, effective_tab_id)
                    .await);
            }
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
            let res = dock_ok_result(
                "screenshot",
                json!({
                    "png_base64": encoded,
                    "bytes": png.len(),
                    "full_page": full_page,
                }),
            );
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), res, effective_tab_id)
                .await);
        }

        if let BrowserAction::Assert {
            kind,
            selector,
            expected,
            attribute,
            op,
            count,
            timeout_ms,
        } = action.clone()
        {
            let result = execute_assert(
                controller.as_ref(),
                kind,
                selector,
                expected,
                attribute,
                op,
                count,
                timeout_ms,
                effective_tab_id,
            )
            .await?;
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        if let BrowserAction::ConsoleLogs {
            level,
            since_ms,
            clear_after,
            limit,
        } = action.clone()
        {
            let mut args_obj = serde_json::Map::new();
            if let Some(level) = level {
                args_obj.insert("level".into(), Value::String(level));
            }
            if let Some(since) = since_ms {
                args_obj.insert("since_ms".into(), Value::from(since));
            }
            if clear_after {
                args_obj.insert("clear_after".into(), Value::Bool(true));
            }
            if let Some(limit) = limit {
                args_obj.insert("limit".into(), Value::from(limit));
            }
            let args_value = inject_tab_id_into_args(Value::Object(args_obj), effective_tab_id);
            let resp = controller
                .exec(DockRequest {
                    kind: "console_logs".to_string(),
                    args: args_value,
                    timeout_ms: DEFAULT_TIMEOUT_MS,
                })
                .await?;
            let result = dock_response_to_result("console_logs", resp);
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        if let BrowserAction::CollectLinks { same_origin, limit } = action.clone() {
            let mut args_obj = serde_json::Map::new();
            if let Some(so) = same_origin {
                args_obj.insert("same_origin".into(), Value::Bool(so));
            }
            if let Some(limit) = limit {
                args_obj.insert("limit".into(), Value::from(limit));
            }
            let args_value = inject_tab_id_into_args(Value::Object(args_obj), effective_tab_id);
            let resp = controller
                .exec(DockRequest {
                    kind: "collect_links".to_string(),
                    args: args_value,
                    timeout_ms: DEFAULT_TIMEOUT_MS,
                })
                .await?;
            let result = dock_response_to_result("collect_links", resp);
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        if let BrowserAction::GetStyles { selector, limit } = action.clone() {
            let mut args_obj = serde_json::Map::new();
            if let Some(sel) = selector {
                args_obj.insert("selector".into(), Value::String(sel));
            }
            if let Some(limit) = limit {
                args_obj.insert("limit".into(), Value::from(limit));
            }
            let args_value = inject_tab_id_into_args(Value::Object(args_obj), effective_tab_id);
            let resp = controller
                .exec(DockRequest {
                    kind: "get_styles".to_string(),
                    args: args_value,
                    timeout_ms: DEFAULT_TIMEOUT_MS,
                })
                .await?;
            let result = dock_response_to_result("get_styles", resp);
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        if matches!(action, BrowserAction::PerfVitals) {
            let args_value =
                inject_tab_id_into_args(Value::Object(serde_json::Map::new()), effective_tab_id);
            let resp = controller
                .exec(DockRequest {
                    kind: "perf_vitals".to_string(),
                    args: args_value,
                    timeout_ms: DEFAULT_TIMEOUT_MS,
                })
                .await?;
            let result = dock_response_to_result("perf_vitals", resp);
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        if let BrowserAction::Emulate {
            viewport,
            network,
            cpu_rate,
            reset,
        } = action.clone()
        {
            let mut args_obj = serde_json::Map::new();
            if let Some(vp) = viewport {
                args_obj.insert("viewport".into(), vp);
            }
            if let Some(net) = network {
                args_obj.insert("network".into(), Value::String(net));
            }
            if let Some(rate) = cpu_rate {
                if let Some(num) = serde_json::Number::from_f64(rate) {
                    args_obj.insert("cpu_rate".into(), Value::Number(num));
                }
            }
            if reset {
                args_obj.insert("reset".into(), Value::Bool(true));
            }
            let args_value = inject_tab_id_into_args(Value::Object(args_obj), effective_tab_id);
            let resp = controller
                .exec(DockRequest {
                    kind: "emulate".to_string(),
                    args: args_value,
                    timeout_ms: DEFAULT_TIMEOUT_MS,
                })
                .await?;
            let result = dock_response_to_result("emulate", resp);
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        if let BrowserAction::NetworkCapture {
            mode,
            request_id,
            limit,
            url_contains,
            only_failures,
            api_only,
        } = action.clone()
        {
            let mut args_obj = serde_json::Map::new();
            if let Some(mode) = mode {
                args_obj.insert("mode".into(), Value::String(mode));
            }
            if let Some(rid) = request_id {
                args_obj.insert("request_id".into(), Value::String(rid));
            }
            if let Some(limit) = limit {
                args_obj.insert("limit".into(), Value::from(limit));
            }
            if let Some(filter) = url_contains {
                args_obj.insert("url_contains".into(), Value::String(filter));
            }
            if only_failures {
                args_obj.insert("only_failures".into(), Value::Bool(true));
            }
            if api_only {
                args_obj.insert("api_only".into(), Value::Bool(true));
            }
            let args_value = inject_tab_id_into_args(Value::Object(args_obj), effective_tab_id);
            let resp = controller
                .exec(DockRequest {
                    kind: "network_capture".to_string(),
                    args: args_value,
                    timeout_ms: DEFAULT_TIMEOUT_MS,
                })
                .await?;
            let result = dock_response_to_result("network_capture", resp);
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        if matches!(action, BrowserAction::WebToolsList) {
            let args_value =
                inject_tab_id_into_args(Value::Object(serde_json::Map::new()), effective_tab_id);
            let resp = controller
                .exec(DockRequest {
                    kind: "web_tools_list".to_string(),
                    args: args_value,
                    timeout_ms: DEFAULT_TIMEOUT_MS,
                })
                .await?;
            let result = dock_response_to_result("web_tools_list", resp);
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        if let BrowserAction::WebToolsCall { name, tool_args } = action.clone() {
            let mut args_obj = serde_json::Map::new();
            args_obj.insert("name".into(), Value::String(name));
            if let Some(ta) = tool_args {
                args_obj.insert("tool_args".into(), ta);
            }
            let args_value = inject_tab_id_into_args(Value::Object(args_obj), effective_tab_id);
            let resp = controller
                .exec(DockRequest {
                    kind: "web_tools_call".to_string(),
                    args: args_value,
                    timeout_ms: 30_000,
                })
                .await?;
            let result = dock_response_to_result("web_tools_call", resp);
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        if let BrowserAction::NetworkErrors { since_ms, limit } = action.clone() {
            let mut args_obj = serde_json::Map::new();
            if let Some(since) = since_ms {
                args_obj.insert("since_ms".into(), Value::from(since));
            }
            if let Some(limit) = limit {
                args_obj.insert("limit".into(), Value::from(limit));
            }
            let args_value = inject_tab_id_into_args(Value::Object(args_obj), effective_tab_id);
            let resp = controller
                .exec(DockRequest {
                    kind: "network_errors".to_string(),
                    args: args_value,
                    timeout_ms: DEFAULT_TIMEOUT_MS,
                })
                .await?;
            let result = dock_response_to_result("network_errors", resp);
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        if let BrowserAction::NetworkIdle {
            idle_ms,
            timeout_ms,
        } = action.clone()
        {
            let idle_ms_val = idle_ms.unwrap_or(500);
            let timeout_val = timeout_ms.unwrap_or(15_000);
            let args_value = inject_tab_id_into_args(
                json!({
                    "idle_ms": idle_ms_val,
                    "timeout_ms": timeout_val,
                }),
                effective_tab_id,
            );
            let resp = controller
                .exec(DockRequest {
                    kind: "network_idle".to_string(),
                    args: args_value,
                    timeout_ms: timeout_val.saturating_add(2_000),
                })
                .await?;
            let result = dock_response_to_result("network_idle", resp);
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        if let BrowserAction::ClearStorage { scope, force } = action.clone() {
            if !force {
                let pinned = session_id_opt.as_deref().and_then(current_test_target_tab);
                let target = effective_tab_id;
                let pin_hits = match (target, pinned) {
                    (Some(t), Some(p)) => t == p,
                    _ => false,
                };
                let owner_blocks = if let Some(tab_id) = target {
                    matches!(
                        lookup_tab_owner(controller.as_ref(), tab_id).await.as_deref(),
                        Some("user")
                    )
                } else {
                    false
                };
                if pin_hits || owner_blocks {
                    let reason = if pin_hits {
                        "tab is pinned as the QA test target"
                    } else {
                        "tab is owned by the user"
                    };
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "clear_storage refused: {reason}. Pass force=true to override, \
                             but this will wipe the user's login session and cookies."
                        )),
                    });
                }
            }
            let args_value =
                inject_tab_id_into_args(json!({ "scope": scope }), effective_tab_id);
            let resp = controller
                .exec(DockRequest {
                    kind: "clear_storage".to_string(),
                    args: args_value,
                    timeout_ms: DEFAULT_TIMEOUT_MS,
                })
                .await?;
            let result = dock_response_to_result("clear_storage", resp);
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        if matches!(action, BrowserAction::Back) {
            let args_value = inject_tab_id_into_args(json!({}), effective_tab_id);
            let resp = controller
                .exec(DockRequest {
                    kind: "history_back".to_string(),
                    args: args_value,
                    timeout_ms: DEFAULT_TIMEOUT_MS,
                })
                .await?;
            let result = dock_response_to_result("back", resp);
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        if matches!(action, BrowserAction::Forward) {
            let args_value = inject_tab_id_into_args(json!({}), effective_tab_id);
            let resp = controller
                .exec(DockRequest {
                    kind: "history_forward".to_string(),
                    args: args_value,
                    timeout_ms: DEFAULT_TIMEOUT_MS,
                })
                .await?;
            let result = dock_response_to_result("forward", resp);
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        if matches!(action, BrowserAction::Reload) {
            let args_value = inject_tab_id_into_args(json!({}), effective_tab_id);
            let resp = controller
                .exec(DockRequest {
                    kind: "history_reload".to_string(),
                    args: args_value,
                    timeout_ms: DEFAULT_TIMEOUT_MS,
                })
                .await?;
            let result = dock_response_to_result("reload", resp);
            return Ok(self
                .decorate_for_effective_tab(controller.as_ref(), result, effective_tab_id)
                .await);
        }

        let (kind, args, timeout_ms, action_name): (&'static str, Value, u64, &'static str) =
            match action {
                BrowserAction::Open { url } => {
                    self.validate_url(&url, true)?;
                    (
                        "navigate",
                        json!({ "url": url }),
                        DEFAULT_TIMEOUT_MS,
                        "open",
                    )
                }
                BrowserAction::Snapshot {
                    interactive_only,
                    compact,
                    depth,
                } => (
                    "snapshot",
                    json!({
                        "interactive_only": interactive_only,
                        "compact": compact,
                        "depth": depth,
                    }),
                    DEFAULT_TIMEOUT_MS,
                    "snapshot",
                ),
                BrowserAction::Click { selector } => (
                    "click",
                    json!({ "selector": selector }),
                    DEFAULT_TIMEOUT_MS,
                    "click",
                ),
                BrowserAction::Fill { selector, value } => (
                    "set_value",
                    json!({ "selector": selector, "value": value }),
                    DEFAULT_TIMEOUT_MS,
                    "fill",
                ),
                BrowserAction::Type { selector, text } => (
                    "type_text",
                    json!({ "selector": selector, "text": text }),
                    DEFAULT_TIMEOUT_MS,
                    "type",
                ),
                BrowserAction::GetText { selector } => (
                    "get_text",
                    json!({ "selector": selector }),
                    DEFAULT_TIMEOUT_MS,
                    "get_text",
                ),
                BrowserAction::GetTitle => {
                    ("get_title", json!({}), DEFAULT_TIMEOUT_MS, "get_title")
                }
                BrowserAction::GetUrl => ("get_url", json!({}), DEFAULT_TIMEOUT_MS, "get_url"),
                BrowserAction::Screenshot { .. } => {
                    return Err(anyhow::anyhow!(
                        "browser action 'screenshot' must be dispatched via execute_action; reached dock fallback by mistake"
                    ));
                }
                BrowserAction::Wait {
                    selector,
                    ms,
                    text,
                    until,
                } => {
                    let timeout_ms = ms.unwrap_or(15_000);
                    let mut wait_args = serde_json::Map::new();
                    if let Some(s) = selector {
                        wait_args.insert("selector".into(), Value::String(s));
                    }
                    if let Some(t) = text {
                        wait_args.insert("text".into(), Value::String(t));
                    }
                    if let Some(u) = until {
                        wait_args.insert("until".into(), Value::String(u));
                    }
                    wait_args.insert("timeout_ms".into(), Value::from(timeout_ms));
                    (
                        "wait_for",
                        Value::Object(wait_args),
                        timeout_ms.saturating_add(2_000),
                        "wait",
                    )
                }
                BrowserAction::Press { key } => (
                    "press_key",
                    json!({ "key": key }),
                    DEFAULT_TIMEOUT_MS,
                    "press",
                ),
                BrowserAction::Hover { selector } => (
                    "hover",
                    json!({ "selector": selector }),
                    DEFAULT_TIMEOUT_MS,
                    "hover",
                ),
                BrowserAction::Scroll { direction, pixels } => (
                    "scroll",
                    json!({
                        "direction": direction,
                        "pixels": pixels,
                    }),
                    DEFAULT_TIMEOUT_MS,
                    "scroll",
                ),
                BrowserAction::IsVisible { selector } => (
                    "is_visible",
                    json!({ "selector": selector }),
                    DEFAULT_TIMEOUT_MS,
                    "is_visible",
                ),
                BrowserAction::Close => {
                    ("dock_close", json!({}), DEFAULT_TIMEOUT_MS, "close")
                }
                BrowserAction::Find {
                    by,
                    value,
                    action: find_action,
                    fill_value,
                } => (
                    "find",
                    json!({
                        "by": by,
                        "value": value,
                        "action": find_action,
                        "fill_value": fill_value,
                    }),
                    DEFAULT_TIMEOUT_MS,
                    "find",
                ),

                BrowserAction::OpenTab { .. }
                | BrowserAction::CloseTab { .. }
                | BrowserAction::ActivateTab { .. }
                | BrowserAction::ListTabs
                | BrowserAction::AttachTab { .. } => {
                    return Err(anyhow::anyhow!(
                        "browser tab action must be dispatched via execute_action; reached dock fallback by mistake"
                    ));
                }
                BrowserAction::Assert { .. }
                | BrowserAction::ConsoleLogs { .. }
                | BrowserAction::NetworkIdle { .. }
                | BrowserAction::ClearStorage { .. }
                | BrowserAction::Back
                | BrowserAction::Forward
                | BrowserAction::Reload
                | BrowserAction::CollectLinks { .. }
                | BrowserAction::NetworkErrors { .. }
                | BrowserAction::GetStyles { .. }
                | BrowserAction::PerfVitals
                | BrowserAction::Emulate { .. }
                | BrowserAction::NetworkCapture { .. }
                | BrowserAction::WebToolsList
                | BrowserAction::WebToolsCall { .. }
                | BrowserAction::RunSteps { .. } => {
                    return Err(anyhow::anyhow!(
                        "browser QA action must be dispatched via execute_action; reached dock fallback by mistake"
                    ));
                }
                BrowserAction::PinTestTarget { .. }
                | BrowserAction::ClearTestTarget
                | BrowserAction::GetTestTarget => {
                    return Err(anyhow::anyhow!(
                        "browser test-target action must be dispatched via execute_action; reached dock fallback by mistake"
                    ));
                }
            };

        let args = inject_tab_id_into_args(args, effective_tab_id);

        let resp = controller
            .exec(DockRequest {
                kind: kind.to_string(),
                args,
                timeout_ms,
            })
            .await?;

        if !resp.ok {
            let raw_err = resp
                .error
                .unwrap_or_else(|| format!("dock backend reported failure for {kind}"));
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(sanitize_browser_output(&raw_err)),
            });
        }

        if kind == "navigate" {
            let reused = resp
                .value
                .as_object()
                .and_then(|m| m.get("reused"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !reused {
                tokio::time::sleep(Duration::from_millis(120)).await;
                const NAV_READY_TIMEOUT_MS: u64 = 12_000;
                let wait_args = inject_tab_id_into_args(
                    json!({
                        "ready_state": "interactive",
                        "timeout_ms": NAV_READY_TIMEOUT_MS,
                    }),
                    effective_tab_id,
                );
                let _ = controller
                    .exec(DockRequest {
                        kind: "wait_for".to_string(),
                        args: wait_args,
                        timeout_ms: NAV_READY_TIMEOUT_MS.saturating_add(2_000),
                    })
                    .await;
            }
        }

        let final_result = dock_ok_result(action_name, resp.value);
        Ok(self
            .decorate_for_effective_tab(controller.as_ref(), final_result, effective_tab_id)
            .await)
    }

    async fn decorate_for_effective_tab(
        &self,
        controller: &dyn DockController,
        result: ToolResult,
        effective_tab_id: Option<u32>,
    ) -> ToolResult {
        let Some(tab_id) = effective_tab_id else {
            return result;
        };
        let owner = lookup_tab_owner(controller, tab_id).await;
        decorate_result_with_owner(result, tab_id, owner)
    }

    #[allow(clippy::unnecessary_wraps, clippy::unused_self)]
    fn to_result(&self, resp: AgentBrowserResponse) -> anyhow::Result<ToolResult> {
        if resp.success {
            let output = resp
                .data
                .map(|d| {
                    let pretty = serde_json::to_string_pretty(&d).unwrap_or_default();
                    sanitize_browser_output(&pretty)
                })
                .unwrap_or_default();
            Ok(ToolResult {
                success: true,
                output,
                error: None,
            })
        } else {
            Ok(ToolResult {
                success: false,
                output: String::new(),
                error: resp
                    .error
                    .map(|err| sanitize_browser_output(&err)),
            })
        }
    }
}

fn sanitize_browser_output(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let sanitizer = crate::services::governance::pii_sanitizer::global_sanitizer();
    if !sanitizer.enabled() {
        return input.to_string();
    }
    let in_debug = matches!(
        crate::agent::coding_mode::active_coding_mode(),
        crate::agent::coding_mode::CodingMode::Debug
    );
    if !in_debug {
        return input.to_string();
    }
    let (clean, _) = sanitizer.sanitize(input);
    clean
}

#[async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        concat!(
            "**Built-in browser**  -  the SenAgentOS embedded dock webview is the ONLY browser the user wants you to drive ",
            "for opening pages, navigating URLs, reading page content, screenshots, and any in-app browsing task. ",
            "When the user says \"open <site>\", \"打开<网站>\", \"navigate to ...\", call this tool with ",
            "`action='open'` and `url=<target>`  -  that single call fully opens the page in the visible dock and you ",
            "MUST NOT additionally call `browser_open`, `browser_delegate`, or any external/system-browser launcher; ",
            "doing so causes the URL to also pop in Chrome/Edge which the user has explicitly forbidden. ",
            "Backends: `auto` selects tauri_dock inside the desktop app (TauriDockController) so every ",
            "navigate/click/fill/screenshot is rendered live in the dock; other backends (agent-browser, rust-native, ",
            "computer_use) are fallbacks. ",
            "Supports DOM actions plus optional OS-level actions (mouse_move, mouse_click, mouse_drag, ",
            "key_type, key_press, screen_capture) through a computer-use sidecar. ",
            "**Selector formats** (all backends): `@e1`-style refs returned by `snapshot`, raw CSS (e.g. `#id`, ",
            "`.class`, `[data-x=y]`), `text=<exact-or-substring>`, and `label=<label-text>`. ",
            "**Workflow**: call `action='snapshot'` to enumerate interactive elements (each gets an `@e<n>` ref), ",
            "then drive them via click/fill/type/hover passing that ref as `selector`. ",
            "**Navigation**: `action='open'` on the dock backend automatically waits for the new page to become ",
            "interactive before returning, so a follow-up `click`/`get_text` will run against the loaded DOM. ",
            "`action='open'` already fulfils \"open this URL\" requests on its own  -  do NOT chain a second tool to ",
            "re-open the same URL. ",
            "**Wait**: `action='wait'` with only `ms` sleeps for that many milliseconds (use it sparingly  -  prefer ",
            "`selector` or `text` for resilience). Use `selector` to wait until an element is visible, or `text` ",
            "to wait until specific text appears anywhere in the body. ",
            "**Multi-tab** (`open_tab`/`close_tab`/`activate_tab`/`list_tabs`/`attach_tab`) is only available in the dock backend. ",
            "Use `list_tabs` to enumerate every tab (including ones the user already opened) with `{tab_id, owner, url, title, is_active}`. ",
            "`attach_tab(tab_id=<id>)` switches to a specific tab and binds subsequent commands to it  -  required when ",
            "operating on a user-pre-authenticated tab so no credentials are needed. ",
            "Use `collect_links` to enumerate same-origin `<a href>`/`form action` for BFS exploration, and `network_errors` to pull ",
            "recent `status >= 400` fetch/XHR responses for backend coverage checks. ",
            "Enforces `browser.allowed_domains` for public hosts when the command policy is enabled. ",
            "**Local preview workflow**: for static HTML/JS/CSS, navigate directly to a `file:///<absolute path>/index.html` ",
            "URL  -  the embedded dock backend supports file:// natively, no HTTP server required. ",
            "For dev servers (e.g. `python -m http.server`, `vite`, `next dev`), first launch the server with the ",
            "`shell` tool and `background: true` (otherwise the foreground command will time out and be killed), ",
            "then call this tool with `action='open'` and `url='http://localhost:<port>'`. The dock backend always ",
            "permits localhost / 127.0.0.1 / ::1 / file:// so allowed_domains and private-host blocks do not apply there."
        )
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["open", "snapshot", "click", "fill", "type", "get_text",
                             "get_styles",
                             "get_title", "get_url", "screenshot", "wait", "press",
                             "hover", "scroll", "is_visible", "close", "find",
                             "open_tab", "close_tab", "activate_tab", "list_tabs",
                             "attach_tab", "collect_links", "network_errors",
                             "mouse_move", "mouse_click", "mouse_drag", "key_type",
                             "key_press", "screen_capture",
                             "assert", "console_logs", "network_idle", "clear_storage",
                             "back", "forward", "reload",
                             "pin_test_target", "clear_test_target", "get_test_target",
                             "perf_vitals", "emulate", "network_capture",
                             "web_tools_list", "web_tools_call", "run_steps"],
                    "description": "Browser action to perform (OS-level actions require backend=computer_use; tab_*/QA/test-target actions require backend=tauri_dock). Use pin_test_target/get_test_target/clear_test_target in Debug mode to lock automated testing onto a user-pre-authenticated tab. Use get_styles for visual QA: with 'selector' it returns the element's computed styles (color/background/font/border-radius/box-shadow/spacing + bounding rect); without 'selector' it returns a page-level style audit aggregating distinct text colors, background colors, font families, font sizes and border radii across visible elements - use it to quantify theme consistency and compare against design tokens. Use perf_vitals to read real Core Web Vitals (LCP/FCP/CLS/INP-worst, long tasks, TTFB, transfer bytes) collected since page load. Use emulate (CDP, Windows dock) to test responsive layouts (viewport={width,height,mobile}) and degraded networks (network=offline|slow-3g|fast-3g|none, cpu_rate=1..20); ALWAYS call emulate with reset=true when done. Use network_capture (CDP, Windows dock) for full request/response auditing: mode=start before exercising the page, then mode=dump (filters: api_only/only_failures/url_contains/limit) to cross-check API data against rendered UI, mode=body with request_id to inspect a JSON response, mode=stop when finished. Use web_tools_list to discover WebMCP tools the page registered via navigator.modelContext, and web_tools_call (name + tool_args) to invoke one as a structured fast path instead of clicking through the UI - always re-verify the visible UI afterwards. Use run_steps with steps=[{action,...},...] to execute up to 20 simple actions in one call (no nested run_steps; stops on first failure unless continue_on_error)."
                },
                "tab": {
                    "type": "integer",
                    "description": "Tab id (for close_tab / activate_tab; tauri_dock backend only)"
                },
                "tab_id": {
                    "type": "integer",
                    "description": "When provided, run the action against this tab id (use `list_tabs` to discover). Required for `attach_tab`. Setting it lets the assistant operate on a user-owned tab without credentials."
                },
                "same_origin": {
                    "type": "boolean",
                    "description": "For collect_links: when true, drop URLs whose origin differs from the current page"
                },
                "activate": {
                    "type": "boolean",
                    "description": "When true (default), focus the new tab after open_tab"
                },
                "url": {
                    "type": "string",
                    "description": "URL to navigate to (for 'open' action)"
                },
                "selector": {
                    "type": "string",
                    "description": "Element selector. Supports: @e<n> refs from snapshot (e.g. @e1), CSS (#id, .class, [attr=val]), text=<substring|exact> or label=<text>"
                },
                "value": {
                    "type": "string",
                    "description": "Value to fill or type"
                },
                "text": {
                    "type": "string",
                    "description": "Text to type or wait for"
                },
                "key": {
                    "type": "string",
                    "description": "Key to press (Enter, Tab, Escape, etc.)"
                },
                "x": {
                    "type": "integer",
                    "description": "Screen X coordinate (computer_use: mouse_move/mouse_click)"
                },
                "y": {
                    "type": "integer",
                    "description": "Screen Y coordinate (computer_use: mouse_move/mouse_click)"
                },
                "from_x": {
                    "type": "integer",
                    "description": "Drag source X coordinate (computer_use: mouse_drag)"
                },
                "from_y": {
                    "type": "integer",
                    "description": "Drag source Y coordinate (computer_use: mouse_drag)"
                },
                "to_x": {
                    "type": "integer",
                    "description": "Drag target X coordinate (computer_use: mouse_drag)"
                },
                "to_y": {
                    "type": "integer",
                    "description": "Drag target Y coordinate (computer_use: mouse_drag)"
                },
                "button": {
                    "type": "string",
                    "enum": ["left", "right", "middle"],
                    "description": "Mouse button for computer_use mouse_click"
                },
                "direction": {
                    "type": "string",
                    "enum": ["up", "down", "left", "right"],
                    "description": "Scroll direction"
                },
                "pixels": {
                    "type": "integer",
                    "description": "Pixels to scroll"
                },
                "interactive_only": {
                    "type": "boolean",
                    "description": "For snapshot: only show interactive elements"
                },
                "compact": {
                    "type": "boolean",
                    "description": "For snapshot: remove empty structural elements"
                },
                "depth": {
                    "type": "integer",
                    "description": "For snapshot: limit tree depth"
                },
                "full_page": {
                    "type": "boolean",
                    "description": "For screenshot: capture full page"
                },
                "path": {
                    "type": "string",
                    "description": "File path for screenshot"
                },
                "ms": {
                    "type": "integer",
                    "description": "Milliseconds to wait"
                },
                "by": {
                    "type": "string",
                    "enum": ["role", "text", "label", "placeholder", "testid"],
                    "description": "For find: semantic locator type"
                },
                "find_action": {
                    "type": "string",
                    "enum": ["click", "fill", "text", "hover", "check"],
                    "description": "For find: action to perform on found element"
                },
                "fill_value": {
                    "type": "string",
                    "description": "For find with fill action: value to fill"
                },
                "assert_kind": {
                    "type": "string",
                    "enum": [
                        "text",
                        "visible",
                        "not_visible",
                        "url",
                        "title",
                        "attribute",
                        "value",
                        "count",
                        "console_clean"
                    ],
                    "description": "For assert action: type of assertion to evaluate"
                },
                "expected": {
                    "type": "string",
                    "description": "For assert action: expected value/substring to match"
                },
                "attribute": {
                    "type": "string",
                    "description": "For assert action with kind=attribute: attribute name"
                },
                "op": {
                    "type": "string",
                    "enum": ["==", "!=", ">", ">=", "<", "<="],
                    "description": "For assert count: comparison operator"
                },
                "count": {
                    "type": "integer",
                    "description": "For assert count: expected element count"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Custom timeout in milliseconds (assert / network_idle)"
                },
                "until": {
                    "type": "string",
                    "enum": ["network_idle", "load", "dom_content_loaded"],
                    "description": "For wait action: page lifecycle event to wait for"
                },
                "level": {
                    "type": "string",
                    "enum": ["log", "info", "warn", "error", "debug"],
                    "description": "For console_logs: filter by log level"
                },
                "since_ms": {
                    "type": "integer",
                    "description": "For console_logs: only entries newer than this unix-ms timestamp"
                },
                "clear_after": {
                    "type": "boolean",
                    "description": "For console_logs: clear ring buffer after returning entries"
                },
                "limit": {
                    "type": "integer",
                    "description": "For console_logs: maximum number of entries"
                },
                "idle_ms": {
                    "type": "integer",
                    "description": "For network_idle (or wait until=network_idle): required idle window in milliseconds"
                },
                "scope": {
                    "type": "string",
                    "enum": ["all", "cookies", "local", "session", "indexeddb", "cache"],
                    "description": "For clear_storage: which storage scope to wipe (default=all)"
                },
                "force": {
                    "type": "boolean",
                    "description": "For clear_storage: required (`true`) to wipe storage on tabs that are user-owned or pinned as QA test target. Without `force`, the action is refused to protect the user's pre-authenticated session."
                },
                "viewport": {
                    "type": "object",
                    "description": "For emulate: {width,height,mobile?,device_scale_factor?} device-metrics override, e.g. {\"width\":375,\"height\":812,\"mobile\":true} for responsive QA"
                },
                "network": {
                    "type": "string",
                    "enum": ["offline", "slow-3g", "fast-3g", "none"],
                    "description": "For emulate: network condition preset (none = remove throttling)"
                },
                "cpu_rate": {
                    "type": "number",
                    "description": "For emulate: CPU slowdown multiplier 1-20 (1 = no throttling)"
                },
                "reset": {
                    "type": "boolean",
                    "description": "For emulate: clear ALL overrides (viewport + network + cpu). Always call this after degraded-condition tests."
                },
                "mode": {
                    "type": "string",
                    "enum": ["start", "stop", "dump", "body", "clear"],
                    "description": "For network_capture: start recording / stop / dump captured requests / fetch one response body / clear buffer"
                },
                "request_id": {
                    "type": "string",
                    "description": "For network_capture mode=body: the request_id returned by mode=dump"
                },
                "url_contains": {
                    "type": "string",
                    "description": "For network_capture mode=dump: only return requests whose URL contains this substring"
                },
                "only_failures": {
                    "type": "boolean",
                    "description": "For network_capture mode=dump: only return failed requests (network error or HTTP >= 400)"
                },
                "api_only": {
                    "type": "boolean",
                    "description": "For network_capture mode=dump: only return XHR/Fetch requests (API calls)"
                },
                "name": {
                    "type": "string",
                    "description": "For web_tools_call: registered WebMCP tool name (from web_tools_list)"
                },
                "tool_args": {
                    "type": "object",
                    "description": "For web_tools_call: arguments object matching the tool's input_schema"
                },
                "steps": {
                    "type": "array",
                    "items": { "type": "object" },
                    "description": "For run_steps: ordered list of action objects (same shape as top-level args, e.g. [{\"action\":\"open\",\"url\":\"...\"},{\"action\":\"assert\",\"assert_kind\":\"text\",\"expected\":\"...\"}]); max 20, no nested run_steps"
                },
                "continue_on_error": {
                    "type": "boolean",
                    "description": "For run_steps: keep executing remaining steps after a failure (default false)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {

        if !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: autonomy is read-only".into()),
            });
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: rate limit exceeded".into()),
            });
        }

        if args.get("action").and_then(|v| v.as_str()) == Some("run_steps") {
            return self.execute_run_steps(args).await;
        }

        let _resource_guard = match crate::session::acquire_browser_for_current_session().await {
            Some(Ok(g)) => Some(g),
            Some(Err(e)) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("{e}")),
                });
            }
            None => None,
        };

        let args = match crate::services::governance::credential_vault::try_get_credential_vault() {
            Some(vault) => vault.resolve_json(&args),
            None => args,
        };

        let backend = match self.resolve_backend().await {
            Ok(selected) => selected,
            Err(error) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(error.to_string()),
                });
            }
        };

        let action_str = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;

        if !is_supported_browser_action(action_str) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Unknown action: {action_str}")),
            });
        }

        if backend == ResolvedBackend::ComputerUse {
            return self.execute_computer_use_action(action_str, &args).await;
        }

        if is_computer_use_only_action(action_str) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(unavailable_action_for_backend_error(action_str, backend)),
            });
        }

        let action = match parse_browser_action(action_str, &args) {
            Ok(a) => a,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e.to_string()),
                });
            }
        };

        let request_tab_id = args
            .get("tab_id")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as u32);

        self.execute_action(action, backend, request_tab_id).await
    }
}

#[cfg(feature = "browser-native")]
mod native_backend {
    use super::BrowserAction;
    use anyhow::{Context, Result};
    use base64::Engine;
    use fantoccini::actions::{InputSource, MouseActions, PointerAction};
    use fantoccini::key::Key;
    use fantoccini::{Client, ClientBuilder, Locator};
    use serde_json::{Map, Value, json};
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    #[derive(Default)]
    pub struct NativeBrowserState {
        client: Option<Client>,
    }

    impl NativeBrowserState {
        pub fn is_available(
            _headless: bool,
            webdriver_url: &str,
            _chrome_path: Option<&str>,
        ) -> bool {
            webdriver_endpoint_reachable(webdriver_url, Duration::from_millis(500))
        }

        #[allow(clippy::too_many_lines)]
        pub async fn execute_action(
            &mut self,
            action: BrowserAction,
            headless: bool,
            webdriver_url: &str,
            chrome_path: Option<&str>,
        ) -> Result<Value> {
            match action {
                BrowserAction::Open { url } => {
                    self.ensure_session(headless, webdriver_url, chrome_path)
                        .await?;
                    let client = self.active_client()?;
                    client
                        .goto(&url)
                        .await
                        .with_context(|| format!("Failed to open URL: {url}"))?;
                    let current_url = client
                        .current_url()
                        .await
                        .context("Failed to read current URL after navigation")?;

                    Ok(json!({
                        "backend": "rust_native",
                        "action": "open",
                        "url": current_url.as_str(),
                    }))
                }
                BrowserAction::Snapshot {
                    interactive_only,
                    compact,
                    depth,
                } => {
                    let client = self.active_client()?;
                    let snapshot = client
                        .execute(
                            &snapshot_script(interactive_only, compact, depth.map(i64::from)),
                            vec![],
                        )
                        .await
                        .context("Failed to evaluate snapshot script")?;

                    Ok(json!({
                        "backend": "rust_native",
                        "action": "snapshot",
                        "data": snapshot,
                    }))
                }
                BrowserAction::Click { selector } => {
                    let client = self.active_client()?;
                    find_element(client, &selector).await?.click().await?;

                    Ok(json!({
                        "backend": "rust_native",
                        "action": "click",
                        "selector": selector,
                    }))
                }
                BrowserAction::Fill { selector, value } => {
                    let client = self.active_client()?;
                    let element = find_element(client, &selector).await?;
                    let _ = element.clear().await;
                    element.send_keys(&value).await?;

                    Ok(json!({
                        "backend": "rust_native",
                        "action": "fill",
                        "selector": selector,
                    }))
                }
                BrowserAction::Type { selector, text } => {
                    let client = self.active_client()?;
                    find_element(client, &selector)
                        .await?
                        .send_keys(&text)
                        .await?;

                    Ok(json!({
                        "backend": "rust_native",
                        "action": "type",
                        "selector": selector,
                        "typed": text.len(),
                    }))
                }
                BrowserAction::GetText { selector } => {
                    let client = self.active_client()?;
                    let text = find_element(client, &selector).await?.text().await?;

                    Ok(json!({
                        "backend": "rust_native",
                        "action": "get_text",
                        "selector": selector,
                        "text": text,
                    }))
                }
                BrowserAction::GetTitle => {
                    let client = self.active_client()?;
                    let title = client.title().await.context("Failed to read page title")?;

                    Ok(json!({
                        "backend": "rust_native",
                        "action": "get_title",
                        "title": title,
                    }))
                }
                BrowserAction::GetUrl => {
                    let client = self.active_client()?;
                    let url = client
                        .current_url()
                        .await
                        .context("Failed to read current URL")?;

                    Ok(json!({
                        "backend": "rust_native",
                        "action": "get_url",
                        "url": url.as_str(),
                    }))
                }
                BrowserAction::Screenshot { path, full_page } => {
                    let client = self.active_client()?;
                    let png = client
                        .screenshot()
                        .await
                        .context("Failed to capture screenshot")?;
                    let mut payload = json!({
                        "backend": "rust_native",
                        "action": "screenshot",
                        "full_page": full_page,
                        "bytes": png.len(),
                    });

                    if let Some(path_str) = path {
                        let anchor = self.security.safe_artifact_anchor();
                        let (abs_path, relative_path) =
                            resolve_screenshot_path(&path_str, &anchor)?;
                        if let Some(parent) = abs_path.parent() {
                            if !parent.as_os_str().is_empty() {
                                tokio::fs::create_dir_all(parent).await.with_context(|| {
                                    format!(
                                        "failed to create screenshot dir {}",
                                        parent.display()
                                    )
                                })?;
                            }
                        }
                        tokio::fs::write(&abs_path, &png).await.with_context(|| {
                            format!("failed to write screenshot to {}", abs_path.display())
                        })?;
                        payload["path"] = Value::String(relative_path);
                        payload["saved_to"] = Value::String(abs_path.to_string_lossy().to_string());
                    } else {
                        payload["png_base64"] =
                            Value::String(base64::engine::general_purpose::STANDARD.encode(&png));
                    }

                    Ok(payload)
                }
                BrowserAction::Wait {
                    selector,
                    ms,
                    text,
                    until: _,
                } => {
                    let client = self.active_client()?;
                    if let Some(sel) = selector.as_ref() {
                        wait_for_selector(client, sel).await?;
                        Ok(json!({
                            "backend": "rust_native",
                            "action": "wait",
                            "selector": sel,
                        }))
                    } else if let Some(duration_ms) = ms {
                        tokio::time::sleep(Duration::from_millis(duration_ms)).await;
                        Ok(json!({
                            "backend": "rust_native",
                            "action": "wait",
                            "ms": duration_ms,
                        }))
                    } else if let Some(needle) = text.as_ref() {
                        let xpath = xpath_contains_text(needle);
                        client
                            .wait()
                            .for_element(Locator::XPath(&xpath))
                            .await
                            .with_context(|| {
                                format!("Timed out waiting for text to appear: {needle}")
                            })?;
                        Ok(json!({
                            "backend": "rust_native",
                            "action": "wait",
                            "text": needle,
                        }))
                    } else {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        Ok(json!({
                            "backend": "rust_native",
                            "action": "wait",
                            "ms": 250,
                        }))
                    }
                }
                BrowserAction::Press { key } => {
                    let client = self.active_client()?;
                    let key_input = webdriver_key(&key);
                    match client.active_element().await {
                        Ok(element) => {
                            element.send_keys(&key_input).await?;
                        }
                        Err(_) => {
                            find_element(client, "body")
                                .await?
                                .send_keys(&key_input)
                                .await?;
                        }
                    }

                    Ok(json!({
                        "backend": "rust_native",
                        "action": "press",
                        "key": key,
                    }))
                }
                BrowserAction::Hover { selector } => {
                    let client = self.active_client()?;
                    let element = find_element(client, &selector).await?;
                    hover_element(client, &element).await?;

                    Ok(json!({
                        "backend": "rust_native",
                        "action": "hover",
                        "selector": selector,
                    }))
                }
                BrowserAction::Scroll { direction, pixels } => {
                    let client = self.active_client()?;
                    let amount = i64::from(pixels.unwrap_or(600));
                    let (dx, dy) = match direction.as_str() {
                        "up" => (0, -amount),
                        "down" => (0, amount),
                        "left" => (-amount, 0),
                        "right" => (amount, 0),
                        _ => anyhow::bail!(
                            "Unsupported scroll direction '{direction}'. Use up/down/left/right"
                        ),
                    };

                    let position = client
                        .execute(
                            "window.scrollBy(arguments[0], arguments[1]); return { x: window.scrollX, y: window.scrollY };",
                            vec![json!(dx), json!(dy)],
                        )
                        .await
                        .context("Failed to execute scroll script")?;

                    Ok(json!({
                        "backend": "rust_native",
                        "action": "scroll",
                        "position": position,
                    }))
                }
                BrowserAction::IsVisible { selector } => {
                    let client = self.active_client()?;
                    let visible = match find_element(client, &selector).await {
                        Ok(element) => element.is_displayed().await.unwrap_or(false),
                        Err(_) => false,
                    };

                    Ok(json!({
                        "backend": "rust_native",
                        "action": "is_visible",
                        "selector": selector,
                        "visible": visible,
                    }))
                }
                BrowserAction::Close => {
                    self.reset_session().await;

                    Ok(json!({
                        "backend": "rust_native",
                        "action": "close",
                        "closed": true,
                    }))
                }
                BrowserAction::Find {
                    by,
                    value,
                    action,
                    fill_value,
                } => {
                    let client = self.active_client()?;
                    let selector = selector_for_find(&by, &value);
                    let element = find_element(client, &selector).await?;

                    let payload = match action.as_str() {
                        "click" => {
                            element.click().await?;
                            json!({"result": "clicked"})
                        }
                        "fill" => {
                            let fill = fill_value.ok_or_else(|| {
                                anyhow::anyhow!("find_action='fill' requires fill_value")
                            })?;
                            let _ = element.clear().await;
                            element.send_keys(&fill).await?;
                            json!({"result": "filled", "typed": fill.len()})
                        }
                        "text" => {
                            let text = element.text().await?;
                            json!({"result": "text", "text": text})
                        }
                        "hover" => {
                            hover_element(client, &element).await?;
                            json!({"result": "hovered"})
                        }
                        "check" => {
                            let checked_before = element_checked(&element).await?;
                            if !checked_before {
                                element.click().await?;
                            }
                            let checked_after = element_checked(&element).await?;
                            json!({
                                "result": "checked",
                                "checked_before": checked_before,
                                "checked_after": checked_after,
                            })
                        }
                        _ => anyhow::bail!(
                            "Unsupported find_action '{action}'. Use click/fill/text/hover/check"
                        ),
                    };

                    Ok(json!({
                        "backend": "rust_native",
                        "action": "find",
                        "by": by,
                        "value": value,
                        "selector": selector,
                        "data": payload,
                    }))
                }

                BrowserAction::OpenTab { .. }
                | BrowserAction::CloseTab { .. }
                | BrowserAction::ActivateTab { .. }
                | BrowserAction::ListTabs => anyhow::bail!(
                    "Multi-tab actions are only supported by the embedded dock backend \
                     (tauri_dock). Switch backend or run inside the SenAgentOS desktop app."
                ),
                BrowserAction::Assert { .. }
                | BrowserAction::ConsoleLogs { .. }
                | BrowserAction::NetworkIdle { .. }
                | BrowserAction::ClearStorage { .. }
                | BrowserAction::Back
                | BrowserAction::Forward
                | BrowserAction::Reload
                | BrowserAction::GetStyles { .. }
                | BrowserAction::PerfVitals
                | BrowserAction::Emulate { .. }
                | BrowserAction::NetworkCapture { .. }
                | BrowserAction::WebToolsList
                | BrowserAction::WebToolsCall { .. }
                | BrowserAction::RunSteps { .. } => anyhow::bail!(
                    "QA actions (assert/console_logs/network_idle/clear_storage/back/forward/reload/get_styles/perf_vitals/emulate/network_capture/web_tools_list/web_tools_call/run_steps) require the \
                     embedded dock backend (tauri_dock). Run inside the SenAgentOS desktop app."
                ),
                BrowserAction::PinTestTarget { .. }
                | BrowserAction::ClearTestTarget
                | BrowserAction::GetTestTarget => anyhow::bail!(
                    "Test-target actions (pin_test_target/clear_test_target/get_test_target) require the embedded dock backend (tauri_dock). \
                     Run inside the SenAgentOS desktop app."
                ),
            }
        }

        pub async fn reset_session(&mut self) {
            if let Some(client) = self.client.take() {
                let _ = client.close().await;
            }
        }

        async fn ensure_session(
            &mut self,
            headless: bool,
            webdriver_url: &str,
            chrome_path: Option<&str>,
        ) -> Result<()> {
            if self.client.is_some() {
                return Ok(());
            }

            let mut capabilities: Map<String, Value> = Map::new();
            let mut chrome_options: Map<String, Value> = Map::new();
            let mut args: Vec<Value> = Vec::new();

            if headless {
                args.push(Value::String("--headless=new".to_string()));
                args.push(Value::String("--disable-gpu".to_string()));
            }

            if super::is_service_environment() {
                args.push(Value::String("--no-sandbox".to_string()));
                args.push(Value::String("--disable-dev-shm-usage".to_string()));
            }

            if !args.is_empty() {
                chrome_options.insert("args".to_string(), Value::Array(args));
            }

            if let Some(path) = chrome_path {
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    chrome_options.insert("binary".to_string(), Value::String(trimmed.to_string()));
                }
            }

            if !chrome_options.is_empty() {
                capabilities.insert(
                    "goog:chromeOptions".to_string(),
                    Value::Object(chrome_options),
                );
            }

            let mut builder =
                ClientBuilder::rustls().context("Failed to initialize rustls connector")?;
            if !capabilities.is_empty() {
                builder.capabilities(capabilities);
            }

            let client = builder
                .connect(webdriver_url)
                .await
                .with_context(|| {
                    format!(
                        "Failed to connect to WebDriver at {webdriver_url}. Start chromedriver/geckodriver first"
                    )
                })?;

            self.client = Some(client);
            Ok(())
        }

        fn active_client(&self) -> Result<&Client> {
            self.client.as_ref().ok_or_else(|| {
                anyhow::anyhow!("No active native browser session. Run browser action='open' first")
            })
        }
    }

    fn webdriver_endpoint_reachable(webdriver_url: &str, timeout: Duration) -> bool {
        let parsed = match reqwest::Url::parse(webdriver_url) {
            Ok(url) => url,
            Err(_) => return false,
        };

        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return false;
        }

        let host = match parsed.host_str() {
            Some(h) if !h.is_empty() => h,
            _ => return false,
        };

        let port = parsed.port_or_known_default().unwrap_or(4444);
        let mut addrs = match (host, port).to_socket_addrs() {
            Ok(iter) => iter,
            Err(_) => return false,
        };

        let addr = match addrs.next() {
            Some(a) => a,
            None => return false,
        };

        TcpStream::connect_timeout(&addr, timeout).is_ok()
    }

    fn selector_for_find(by: &str, value: &str) -> String {
        let escaped = css_attr_escape(value);
        match by {
            "role" => format!(r#"[role=\"{escaped}\"]"#),
            "label" => format!("label={value}"),
            "placeholder" => format!(r#"[placeholder=\"{escaped}\"]"#),
            "testid" => format!(r#"[data-testid=\"{escaped}\"]"#),
            _ => format!("text={value}"),
        }
    }

    async fn wait_for_selector(client: &Client, selector: &str) -> Result<()> {
        match parse_selector(selector) {
            SelectorKind::Css(css) => {
                client
                    .wait()
                    .for_element(Locator::Css(&css))
                    .await
                    .with_context(|| format!("Timed out waiting for selector '{selector}'"))?;
            }
            SelectorKind::XPath(xpath) => {
                client
                    .wait()
                    .for_element(Locator::XPath(&xpath))
                    .await
                    .with_context(|| format!("Timed out waiting for selector '{selector}'"))?;
            }
        }
        Ok(())
    }

    async fn find_element(
        client: &Client,
        selector: &str,
    ) -> Result<fantoccini::elements::Element> {
        let element = match parse_selector(selector) {
            SelectorKind::Css(css) => client
                .find(Locator::Css(&css))
                .await
                .with_context(|| format!("Failed to find element by CSS '{css}'"))?,
            SelectorKind::XPath(xpath) => client
                .find(Locator::XPath(&xpath))
                .await
                .with_context(|| format!("Failed to find element by XPath '{xpath}'"))?,
        };
        Ok(element)
    }

    async fn hover_element(client: &Client, element: &fantoccini::elements::Element) -> Result<()> {
        let actions = MouseActions::new("mouse".to_string()).then(PointerAction::MoveToElement {
            element: element.clone(),
            duration: Some(Duration::from_millis(150)),
            x: 0.0,
            y: 0.0,
        });

        client
            .perform_actions(actions)
            .await
            .context("Failed to perform hover action")?;
        let _ = client.release_actions().await;
        Ok(())
    }

    async fn element_checked(element: &fantoccini::elements::Element) -> Result<bool> {
        let checked = element
            .prop("checked")
            .await
            .context("Failed to read checkbox checked property")?
            .unwrap_or_default()
            .to_ascii_lowercase();
        Ok(matches!(checked.as_str(), "true" | "checked" | "1"))
    }

    enum SelectorKind {
        Css(String),
        XPath(String),
    }

    fn parse_selector(selector: &str) -> SelectorKind {
        let trimmed = selector.trim();
        if let Some(text_query) = trimmed.strip_prefix("text=") {
            return SelectorKind::XPath(xpath_contains_text(text_query));
        }

        if let Some(label_query) = trimmed.strip_prefix("label=") {
            let literal = xpath_literal(label_query);
            return SelectorKind::XPath(format!(
                "(//label[contains(normalize-space(.), {literal})]/following::*[self::input or self::textarea or self::select][1] | //*[@aria-label and contains(normalize-space(@aria-label), {literal})] | //label[contains(normalize-space(.), {literal})])"
            ));
        }

        if trimmed.starts_with('@') {
            let escaped = css_attr_escape(trimmed);
            return SelectorKind::Css(format!(r#"[data-zc-ref=\"{escaped}\"]"#));
        }

        SelectorKind::Css(trimmed.to_string())
    }

    fn css_attr_escape(input: &str) -> String {
        input
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', " ")
    }

    fn xpath_contains_text(text: &str) -> String {
        format!("//*[contains(normalize-space(.), {})]", xpath_literal(text))
    }

    fn xpath_literal(input: &str) -> String {
        if !input.contains('"') {
            return format!("\"{input}\"");
        }
        if !input.contains('\'') {
            return format!("'{input}'");
        }

        let segments: Vec<&str> = input.split('"').collect();
        let mut parts: Vec<String> = Vec::new();
        for (index, part) in segments.iter().enumerate() {
            if !part.is_empty() {
                parts.push(format!("\"{part}\""));
            }
            if index + 1 < segments.len() {
                parts.push("'\"'".to_string());
            }
        }

        if parts.is_empty() {
            "\"\"".to_string()
        } else {
            format!("concat({})", parts.join(","))
        }
    }

    fn webdriver_key(key: &str) -> String {
        match key.trim().to_ascii_lowercase().as_str() {
            "enter" => Key::Enter.to_string(),
            "return" => Key::Return.to_string(),
            "tab" => Key::Tab.to_string(),
            "escape" | "esc" => Key::Escape.to_string(),
            "backspace" => Key::Backspace.to_string(),
            "delete" => Key::Delete.to_string(),
            "space" => Key::Space.to_string(),
            "arrowup" | "up" => Key::Up.to_string(),
            "arrowdown" | "down" => Key::Down.to_string(),
            "arrowleft" | "left" => Key::Left.to_string(),
            "arrowright" | "right" => Key::Right.to_string(),
            "home" => Key::Home.to_string(),
            "end" => Key::End.to_string(),
            "pageup" => Key::PageUp.to_string(),
            "pagedown" => Key::PageDown.to_string(),
            other => other.to_string(),
        }
    }

    fn snapshot_script(interactive_only: bool, compact: bool, depth: Option<i64>) -> String {
        let depth_literal = depth
            .map(|level| level.to_string())
            .unwrap_or_else(|| "null".to_string());

        format!(
            r#"(() => {{
  const interactiveOnly = {interactive_only};
  const compact = {compact};
  const maxDepth = {depth_literal};
  const nodes = [];
  const root = document.body || document.documentElement;
  let counter = 0;

  const isVisible = (el) => {{
    const style = window.getComputedStyle(el);
    if (style.display === 'none' || style.visibility === 'hidden' || Number(style.opacity || 1) === 0) {{
      return false;
    }}
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }};

  const isInteractive = (el) => {{
    if (el.matches('a,button,input,select,textarea,summary,[role],*[tabindex]')) return true;
    return typeof el.onclick === 'function';
  }};

  const describe = (el, depth) => {{
    const interactive = isInteractive(el);
    const text = (el.innerText || el.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 140);
    if (interactiveOnly && !interactive) return;
    if (compact && !interactive && !text) return;

    const ref = '@e' + (++counter);
    el.setAttribute('data-zc-ref', ref);
    nodes.push({{
      ref,
      depth,
      tag: el.tagName.toLowerCase(),
      id: el.id || null,
      role: el.getAttribute('role'),
      text,
      interactive,
    }});
  }};

  const walk = (el, depth) => {{
    if (!(el instanceof Element)) return;
    if (maxDepth !== null && depth > maxDepth) return;
    if (isVisible(el)) {{
      describe(el, depth);
    }}
    for (const child of el.children) {{
      walk(child, depth + 1);
      if (nodes.length >= 400) return;
    }}
  }};

  if (root) walk(root, 0);

  return {{
    title: document.title,
    url: window.location.href,
    count: nodes.length,
    nodes,
  }};
}})();"#
        )
    }
}

fn parse_browser_action(action_str: &str, args: &Value) -> anyhow::Result<BrowserAction> {
    match action_str {
        "open" => {
            let url = args
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'url' for open action"))?;
            Ok(BrowserAction::Open { url: url.into() })
        }
        "snapshot" => Ok(BrowserAction::Snapshot {
            interactive_only: args
                .get("interactive_only")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true),
            compact: args
                .get("compact")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true),
            depth: args
                .get("depth")
                .and_then(serde_json::Value::as_u64)
                .map(|d| u32::try_from(d).unwrap_or(u32::MAX)),
        }),
        "click" => {
            let selector = args
                .get("selector")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'selector' for click"))?;
            Ok(BrowserAction::Click {
                selector: selector.into(),
            })
        }
        "fill" => {
            let selector = args
                .get("selector")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'selector' for fill"))?;
            let value = args
                .get("value")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'value' for fill"))?;
            Ok(BrowserAction::Fill {
                selector: selector.into(),
                value: value.into(),
            })
        }
        "type" => {
            let selector = args
                .get("selector")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'selector' for type"))?;
            let text = args
                .get("text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'text' for type"))?;
            Ok(BrowserAction::Type {
                selector: selector.into(),
                text: text.into(),
            })
        }
        "get_text" => {
            let selector = args
                .get("selector")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'selector' for get_text"))?;
            Ok(BrowserAction::GetText {
                selector: selector.into(),
            })
        }
        "get_styles" => Ok(BrowserAction::GetStyles {
            selector: args
                .get("selector")
                .and_then(|v| v.as_str())
                .map(String::from),
            limit: args.get("limit").and_then(serde_json::Value::as_u64),
        }),
        "perf_vitals" => Ok(BrowserAction::PerfVitals),
        "emulate" => Ok(BrowserAction::Emulate {
            viewport: args.get("viewport").filter(|v| v.is_object()).cloned(),
            network: args
                .get("network")
                .and_then(|v| v.as_str())
                .map(String::from),
            cpu_rate: args.get("cpu_rate").and_then(serde_json::Value::as_f64),
            reset: args
                .get("reset")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }),
        "network_capture" => Ok(BrowserAction::NetworkCapture {
            mode: args.get("mode").and_then(|v| v.as_str()).map(String::from),
            request_id: args
                .get("request_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            limit: args.get("limit").and_then(serde_json::Value::as_u64),
            url_contains: args
                .get("url_contains")
                .and_then(|v| v.as_str())
                .map(String::from),
            only_failures: args
                .get("only_failures")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            api_only: args
                .get("api_only")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }),
        "web_tools_list" => Ok(BrowserAction::WebToolsList),
        "web_tools_call" => {
            let name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'name' for web_tools_call"))?;
            Ok(BrowserAction::WebToolsCall {
                name: name.into(),
                tool_args: args.get("tool_args").cloned(),
            })
        }
        "run_steps" => Ok(BrowserAction::RunSteps {
            steps: args
                .get("steps")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default(),
        }),
        "get_title" => Ok(BrowserAction::GetTitle),
        "get_url" => Ok(BrowserAction::GetUrl),
        "screenshot" => Ok(BrowserAction::Screenshot {
            path: args.get("path").and_then(|v| v.as_str()).map(String::from),
            full_page: args
                .get("full_page")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }),
        "wait" => Ok(BrowserAction::Wait {
            selector: args
                .get("selector")
                .and_then(|v| v.as_str())
                .map(String::from),
            ms: args.get("ms").and_then(serde_json::Value::as_u64),
            text: args.get("text").and_then(|v| v.as_str()).map(String::from),
            until: args.get("until").and_then(|v| v.as_str()).map(String::from),
        }),
        "press" => {
            let key = args
                .get("key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'key' for press"))?;
            Ok(BrowserAction::Press { key: key.into() })
        }
        "hover" => {
            let selector = args
                .get("selector")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'selector' for hover"))?;
            Ok(BrowserAction::Hover {
                selector: selector.into(),
            })
        }
        "scroll" => {
            let direction = args
                .get("direction")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'direction' for scroll"))?;
            Ok(BrowserAction::Scroll {
                direction: direction.into(),
                pixels: args
                    .get("pixels")
                    .and_then(serde_json::Value::as_u64)
                    .map(|p| u32::try_from(p).unwrap_or(u32::MAX)),
            })
        }
        "is_visible" => {
            let selector = args
                .get("selector")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'selector' for is_visible"))?;
            Ok(BrowserAction::IsVisible {
                selector: selector.into(),
            })
        }
        "close" => Ok(BrowserAction::Close),
        "find" => {
            let by = args
                .get("by")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'by' for find"))?;
            let value = args
                .get("value")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'value' for find"))?;
            let action = args
                .get("find_action")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'find_action' for find"))?;
            Ok(BrowserAction::Find {
                by: by.into(),
                value: value.into(),
                action: action.into(),
                fill_value: args
                    .get("fill_value")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            })
        }
        "open_tab" => Ok(BrowserAction::OpenTab {
            url: args
                .get("url")
                .and_then(|v| v.as_str())
                .map(String::from),
            activate: args
                .get("activate")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
        }),
        "close_tab" => {
            let tab = args
                .get("tab")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing 'tab' for close_tab"))?;
            Ok(BrowserAction::CloseTab { tab: tab as u32 })
        }
        "activate_tab" => {
            let tab = args
                .get("tab")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing 'tab' for activate_tab"))?;
            Ok(BrowserAction::ActivateTab { tab: tab as u32 })
        }
        "list_tabs" => Ok(BrowserAction::ListTabs),
        "assert" => {
            let kind = args
                .get("assert_kind")
                .and_then(|v| v.as_str())
                .or_else(|| args.get("kind").and_then(|v| v.as_str()))
                .ok_or_else(|| anyhow::anyhow!("Missing 'assert_kind' for assert action"))?
                .to_string();
            Ok(BrowserAction::Assert {
                kind,
                selector: args
                    .get("selector")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                expected: args
                    .get("expected")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                attribute: args
                    .get("attribute")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                op: args.get("op").and_then(|v| v.as_str()).map(String::from),
                count: args
                    .get("count")
                    .and_then(serde_json::Value::as_i64),
                timeout_ms: args
                    .get("timeout_ms")
                    .and_then(serde_json::Value::as_u64),
            })
        }
        "console_logs" => Ok(BrowserAction::ConsoleLogs {
            level: args.get("level").and_then(|v| v.as_str()).map(String::from),
            since_ms: args.get("since_ms").and_then(serde_json::Value::as_u64),
            clear_after: args
                .get("clear_after")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            limit: args.get("limit").and_then(serde_json::Value::as_u64),
        }),
        "network_idle" => Ok(BrowserAction::NetworkIdle {
            idle_ms: args.get("idle_ms").and_then(serde_json::Value::as_u64),
            timeout_ms: args.get("timeout_ms").and_then(serde_json::Value::as_u64),
        }),
        "clear_storage" => Ok(BrowserAction::ClearStorage {
            scope: args.get("scope").and_then(|v| v.as_str()).map(String::from),
            force: args
                .get("force")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }),
        "back" => Ok(BrowserAction::Back),
        "forward" => Ok(BrowserAction::Forward),
        "reload" => Ok(BrowserAction::Reload),
        "attach_tab" => {
            let tab_id = args
                .get("tab_id")
                .and_then(|v| v.as_u64())
                .or_else(|| args.get("tab").and_then(|v| v.as_u64()))
                .ok_or_else(|| anyhow::anyhow!("Missing 'tab_id' for attach_tab"))?;
            Ok(BrowserAction::AttachTab {
                tab_id: tab_id as u32,
            })
        }
        "collect_links" => Ok(BrowserAction::CollectLinks {
            same_origin: args
                .get("same_origin")
                .and_then(serde_json::Value::as_bool),
            limit: args.get("limit").and_then(serde_json::Value::as_u64),
        }),
        "network_errors" => Ok(BrowserAction::NetworkErrors {
            since_ms: args.get("since_ms").and_then(serde_json::Value::as_u64),
            limit: args.get("limit").and_then(serde_json::Value::as_u64),
        }),
        "pin_test_target" => {
            let tab_id = args
                .get("tab_id")
                .and_then(|v| v.as_u64())
                .or_else(|| args.get("tab").and_then(|v| v.as_u64()))
                .ok_or_else(|| anyhow::anyhow!("Missing 'tab_id' for pin_test_target"))?;
            Ok(BrowserAction::PinTestTarget {
                tab_id: tab_id as u32,
            })
        }
        "clear_test_target" => Ok(BrowserAction::ClearTestTarget),
        "get_test_target" => Ok(BrowserAction::GetTestTarget),
        other => anyhow::bail!("Unsupported browser action: {other}"),
    }
}

fn is_supported_browser_action(action: &str) -> bool {
    matches!(
        action,
        "open"
            | "snapshot"
            | "click"
            | "fill"
            | "type"
            | "get_text"
            | "get_styles"
            | "perf_vitals"
            | "emulate"
            | "network_capture"
            | "web_tools_list"
            | "web_tools_call"
            | "run_steps"
            | "get_title"
            | "get_url"
            | "screenshot"
            | "wait"
            | "press"
            | "hover"
            | "scroll"
            | "is_visible"
            | "close"
            | "find"
            | "open_tab"
            | "close_tab"
            | "activate_tab"
            | "list_tabs"
            | "mouse_move"
            | "mouse_click"
            | "mouse_drag"
            | "key_type"
            | "key_press"
            | "screen_capture"
            | "assert"
            | "console_logs"
            | "network_idle"
            | "clear_storage"
            | "back"
            | "forward"
            | "reload"
            | "attach_tab"
            | "collect_links"
            | "network_errors"
            | "pin_test_target"
            | "clear_test_target"
            | "get_test_target"
    )
}

fn is_computer_use_only_action(action: &str) -> bool {
    matches!(
        action,
        "mouse_move" | "mouse_click" | "mouse_drag" | "key_type" | "key_press" | "screen_capture"
    )
}

fn backend_name(backend: ResolvedBackend) -> &'static str {
    match backend {
        ResolvedBackend::AgentBrowser => "agent_browser",
        ResolvedBackend::RustNative => "rust_native",
        ResolvedBackend::ComputerUse => "computer_use",
        ResolvedBackend::TauriDock => "tauri_dock",
    }
}

fn dock_ok_result(action: &str, value: Value) -> ToolResult {
    let payload = json!({
        "backend": "tauri_dock",
        "action": action,
        "data": value,
    });
    let raw = serde_json::to_string_pretty(&payload).unwrap_or_default();
    ToolResult {
        success: true,
        output: sanitize_browser_output(&raw),
        error: None,
    }
}

fn unavailable_action_for_backend_error(action: &str, backend: ResolvedBackend) -> String {
    format!(
        "Action '{action}' is unavailable for backend '{}'",
        backend_name(backend)
    )
}
#[cfg(feature = "browser-native")]
fn is_recoverable_rust_native_error(err: &anyhow::Error) -> bool {
    let message = format!("{err:#}").to_ascii_lowercase();

    if message.contains("invalid session id")
        || message.contains("no such window")
        || message.contains("session not created")
        || message.contains("connection reset")
        || message.contains("broken pipe")
    {
        return true;
    }

    message.contains("webdriver") && (message.contains("timed out") || message.contains("timeout"))
}

fn normalize_domains(domains: Vec<String>) -> Vec<String> {
    domains
        .into_iter()
        .map(|d| d.trim().to_lowercase())
        .filter(|d| !d.is_empty())
        .collect()
}

fn endpoint_reachable(endpoint: &reqwest::Url, timeout: Duration) -> bool {
    let host = match endpoint.host_str() {
        Some(host) if !host.is_empty() => host,
        _ => return false,
    };

    let port = match endpoint.port_or_known_default() {
        Some(port) => port,
        None => return false,
    };

    let mut addrs = match (host, port).to_socket_addrs() {
        Ok(addrs) => addrs,
        Err(_) => return false,
    };

    let addr = match addrs.next() {
        Some(addr) => addr,
        None => return false,
    };

    std::net::TcpStream::connect_timeout(&addr, timeout).is_ok()
}

fn extract_host(url_str: &str) -> anyhow::Result<String> {

    let url = url_str.trim();
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("file://"))
        .unwrap_or(url);

    let authority = without_scheme.split('/').next().unwrap_or(without_scheme);

    let host = if authority.starts_with('[') {

        authority.find(']').map_or(authority, |i| &authority[..=i])
    } else {

        authority.split(':').next().unwrap_or(authority)
    };

    if host.is_empty() {
        anyhow::bail!("Invalid URL: no host");
    }

    Ok(host.to_lowercase())
}

fn action_opens_external_url(action: &BrowserAction) -> bool {
    let url = match action {
        BrowserAction::Open { url } => url.trim(),
        BrowserAction::OpenTab { url: Some(u), .. } => u.trim(),
        _ => return false,
    };
    if url.is_empty() || url.starts_with("file://") {
        return false;
    }
    match extract_host(url) {
        Ok(host) => !is_loopback_host(&host),
        Err(_) => false,
    }
}

fn is_loopback_host(host: &str) -> bool {
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);

    if bare == "localhost" || bare.ends_with(".localhost") {
        return true;
    }

    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => v4.is_loopback(),
            std::net::IpAddr::V6(v6) => v6.is_loopback(),
        };
    }

    false
}

fn is_private_host(host: &str) -> bool {

    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);

    if bare == "localhost" || bare.ends_with(".localhost") {
        return true;
    }

    if bare
        .rsplit('.')
        .next()
        .is_some_and(|label| label == "local")
    {
        return true;
    }

    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => is_non_global_v4(v4),
            std::net::IpAddr::V6(v6) => is_non_global_v6(v6),
        };
    }

    false
}

fn is_non_global_v4(v4: std::net::Ipv4Addr) -> bool {
    let [a, b, _, _] = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || v4.is_multicast()

        || (a == 100 && (64..=127).contains(&b))

        || a >= 240

        || (a == 192 && b == 0)
        || (a == 198 && b == 51)
        || (a == 203 && b == 0)

        || (a == 198 && (18..=19).contains(&b))
}

fn is_non_global_v6(v6: std::net::Ipv6Addr) -> bool {
    let segs = v6.segments();
    v6.is_loopback()
        || v6.is_unspecified()
        || v6.is_multicast()

        || (segs[0] & 0xfe00) == 0xfc00

        || (segs[0] & 0xffc0) == 0xfe80

        || v6.to_ipv4_mapped().is_some_and(is_non_global_v4)
}

fn is_service_environment() -> bool {
    if std::env::var_os("INVOCATION_ID").is_some() {
        return true;
    }
    if std::env::var_os("JOURNAL_STREAM").is_some() {
        return true;
    }
    #[cfg(target_os = "linux")]
    if std::path::Path::new("/run/openrc").exists() && std::env::var_os("HOME").is_none() {
        return true;
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("HOME").is_none() {
        return true;
    }
    false
}

fn ensure_browser_env(cmd: &mut Command) {
    if std::env::var_os("HOME").is_none() {
        cmd.env("HOME", "/tmp");
    }
    let existing = std::env::var("CHROMIUM_FLAGS").unwrap_or_default();
    if !existing.contains("--no-sandbox") {
        let new_flags = if existing.is_empty() {
            "--no-sandbox --disable-dev-shm-usage".to_string()
        } else {
            format!("{existing} --no-sandbox --disable-dev-shm-usage")
        };
        cmd.env("CHROMIUM_FLAGS", new_flags);
    }
}

fn host_matches_allowlist(host: &str, allowed: &[String]) -> bool {
    allowed.iter().any(|pattern| {
        if pattern == "*" {
            return true;
        }
        if pattern.starts_with("*.") {

            let suffix = &pattern[1..];
            host.ends_with(suffix) || host == &pattern[2..]
        } else {

            host == pattern || host.ends_with(&format!(".{pattern}"))
        }
    })
}

fn resolve_screenshot_path(
    target: &str,
    anchor: &std::path::Path,
) -> anyhow::Result<(std::path::PathBuf, String)> {
    if let Some(rest) = target.strip_prefix("auto://") {
        let mut parts = rest.splitn(2, '/');
        let run_id = parts
            .next()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("auto:// screenshot path requires a <run_id> segment")
            })?;
        let remainder = parts.next().unwrap_or("step.png");
        let remainder = remainder.trim_matches('/');
        let remainder = if remainder.is_empty() {
            "step.png".to_string()
        } else {
            remainder.to_string()
        };
        let safe_run_id = sanitize_path_segment(run_id);
        let safe_remainder: std::path::PathBuf = remainder
            .split('/')
            .map(sanitize_path_segment)
            .collect();
        let relative = std::path::PathBuf::from(".senagentos")
            .join("debug-reports")
            .join(&safe_run_id)
            .join("screenshots")
            .join(&safe_remainder);
        let absolute = anchor.join(&relative);
        let rel_str = relative.to_string_lossy().replace('\\', "/");
        return Ok((absolute, rel_str));
    }
    let path = std::path::PathBuf::from(target);
    if path.is_absolute() {
        if crate::security::is_system_path(&path) {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "screenshot.png".to_string());
            let safe_name = sanitize_path_segment(&name);
            let abs = anchor.join(&safe_name);
            return Ok((abs, safe_name));
        }
        return Ok((path.clone(), target.to_string()));
    }
    let abs = anchor.join(&path);
    Ok((abs, target.to_string()))
}

fn sanitize_path_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            'A'..='Z'
            | 'a'..='z'
            | '0'..='9'
            | '_'
            | '-'
            | '.'
            | '(' | ')'
            | '[' | ']' => out.push(ch),
            ' ' => out.push('_'),
            _ => out.push('_'),
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

fn dock_response_to_result(action: &str, resp: DockResponse) -> ToolResult {
    if !resp.ok {
        let raw_err = resp
            .error
            .unwrap_or_else(|| format!("dock backend reported failure for {action}"));
        return ToolResult {
            success: false,
            output: String::new(),
            error: Some(sanitize_browser_output(&raw_err)),
        };
    }
    dock_ok_result(action, resp.value)
}

fn inject_tab_id_into_args(args: Value, tab_id: Option<u32>) -> Value {
    let Some(tab) = tab_id else {
        return args;
    };
    match args {
        Value::Null => json!({ "tab_id": tab }),
        Value::Object(mut map) => {
            if !map.contains_key("tab_id") {
                map.insert("tab_id".into(), Value::from(tab));
            }
            Value::Object(map)
        }
        other => other,
    }
}

async fn lookup_tab_owner(controller: &dyn DockController, tab_id: u32) -> Option<String> {
    controller
        .list_tabs()
        .await
        .ok()
        .and_then(|tabs| {
            tabs.into_iter()
                .find(|t| t.id == tab_id)
                .and_then(|t| t.owner)
        })
}

fn decorate_result_with_owner(mut result: ToolResult, tab_id: u32, owner: Option<String>) -> ToolResult {
    if !result.success {
        return result;
    }
    let parsed: Value = match serde_json::from_str(&result.output) {
        Ok(v) => v,
        Err(_) => return result,
    };
    let Value::Object(mut map) = parsed else {
        return result;
    };
    let is_user_tab = owner.as_deref() == Some("user");
    map.insert("tab_id".into(), Value::from(tab_id));
    if let Some(owner_str) = owner.clone() {
        map.insert("owner".into(), Value::String(owner_str));
    }
    if is_user_tab {
        map.insert("takeover".into(), Value::Bool(true));
    }
    let merged = Value::Object(map);
    result.output = serde_json::to_string_pretty(&merged).unwrap_or(result.output);
    result
}

#[allow(clippy::too_many_arguments)]
async fn execute_assert(
    controller: &dyn DockController,
    kind: String,
    selector: Option<String>,
    expected: Option<String>,
    attribute: Option<String>,
    op: Option<String>,
    count: Option<i64>,
    timeout_ms: Option<u64>,
    effective_tab_id: Option<u32>,
) -> anyhow::Result<ToolResult> {
    let start = std::time::Instant::now();
    let dock_timeout = timeout_ms.unwrap_or(30_000);
    let kind_lower = kind.to_ascii_lowercase();

    macro_rules! dock_exec {
        ($k:expr, $args:expr) => {{
            let merged_args = inject_tab_id_into_args($args, effective_tab_id);
            controller
                .exec(DockRequest {
                    kind: $k.to_string(),
                    args: merged_args,
                    timeout_ms: dock_timeout,
                })
                .await
        }};
    }

    let assert_failure = |actual: serde_json::Value, message: String| -> ToolResult {
        let payload = json!({
            "passed": false,
            "kind": kind_lower,
            "selector": selector,
            "expected": expected,
            "actual": actual,
            "elapsed_ms": start.elapsed().as_millis() as u64,
            "reason": message,
        });
        dock_ok_result("assert", payload)
    };

    let assert_success = |actual: serde_json::Value| -> ToolResult {
        let payload = json!({
            "passed": true,
            "kind": kind_lower,
            "selector": selector,
            "expected": expected,
            "actual": actual,
            "elapsed_ms": start.elapsed().as_millis() as u64,
        });
        dock_ok_result("assert", payload)
    };

    match kind_lower.as_str() {
        "text" => {
            let sel = selector
                .clone()
                .ok_or_else(|| anyhow::anyhow!("assert text requires selector"))?;
            let exp = expected
                .clone()
                .ok_or_else(|| anyhow::anyhow!("assert text requires expected"))?;
            let resp = dock_exec!("get_text", json!({ "selector": sel }))?;
            if !resp.ok {
                return Ok(assert_failure(
                    serde_json::Value::Null,
                    resp.error.unwrap_or_else(|| "get_text failed".into()),
                ));
            }
            let actual_text = resp
                .value
                .as_object()
                .and_then(|m| m.get("text").or_else(|| m.get("value")))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if actual_text.contains(&exp) {
                Ok(assert_success(serde_json::Value::String(actual_text)))
            } else {
                Ok(assert_failure(
                    serde_json::Value::String(actual_text),
                    "text did not contain expected substring".to_string(),
                ))
            }
        }
        "visible" | "not_visible" => {
            let sel = selector
                .clone()
                .ok_or_else(|| anyhow::anyhow!("assert visibility requires selector"))?;
            let resp = dock_exec!("is_visible", json!({ "selector": sel }))?;
            if !resp.ok {
                return Ok(assert_failure(
                    serde_json::Value::Null,
                    resp.error.unwrap_or_else(|| "is_visible failed".into()),
                ));
            }
            let visible = resp
                .value
                .as_object()
                .and_then(|m| {
                    m.get("visible")
                        .or_else(|| m.get("is_visible"))
                        .or_else(|| m.get("value"))
                })
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let want_visible = kind_lower == "visible";
            if visible == want_visible {
                Ok(assert_success(serde_json::Value::Bool(visible)))
            } else {
                Ok(assert_failure(
                    serde_json::Value::Bool(visible),
                    format!(
                        "expected element {} but actual={}",
                        if want_visible { "visible" } else { "hidden" },
                        visible
                    ),
                ))
            }
        }
        "url" | "title" => {
            let dock_kind = if kind_lower == "url" { "get_url" } else { "get_title" };
            let resp = dock_exec!(dock_kind, json!({}))?;
            if !resp.ok {
                return Ok(assert_failure(
                    serde_json::Value::Null,
                    resp.error.unwrap_or_else(|| format!("{dock_kind} failed")),
                ));
            }
            let actual_str = resp
                .value
                .as_object()
                .and_then(|m| {
                    m.get(if kind_lower == "url" { "url" } else { "title" })
                        .or_else(|| m.get("value"))
                })
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let exp = expected.clone().unwrap_or_default();
            let passed = if exp.is_empty() {
                !actual_str.is_empty()
            } else {
                actual_str.contains(&exp)
            };
            if passed {
                Ok(assert_success(serde_json::Value::String(actual_str)))
            } else {
                Ok(assert_failure(
                    serde_json::Value::String(actual_str),
                    format!("{kind_lower} mismatch"),
                ))
            }
        }
        "attribute" => {
            let sel = selector
                .clone()
                .ok_or_else(|| anyhow::anyhow!("assert attribute requires selector"))?;
            let attr = attribute
                .clone()
                .ok_or_else(|| anyhow::anyhow!("assert attribute requires attribute"))?;
            let exp = expected.clone().unwrap_or_default();
            let resp = dock_exec!(
                "get_attribute",
                json!({ "selector": sel, "attribute": attr })
            )?;
            if !resp.ok {
                return Ok(assert_failure(
                    serde_json::Value::Null,
                    resp.error
                        .unwrap_or_else(|| "get_attribute failed".into()),
                ));
            }
            let actual_str = resp
                .value
                .as_object()
                .and_then(|m| m.get("value").or_else(|| m.get("attribute")))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let passed = if exp.is_empty() {
                !actual_str.is_empty()
            } else {
                actual_str == exp || actual_str.contains(&exp)
            };
            if passed {
                Ok(assert_success(serde_json::Value::String(actual_str)))
            } else {
                Ok(assert_failure(
                    serde_json::Value::String(actual_str),
                    "attribute mismatch".to_string(),
                ))
            }
        }
        "value" => {
            let sel = selector
                .clone()
                .ok_or_else(|| anyhow::anyhow!("assert value requires selector"))?;
            let exp = expected.clone().unwrap_or_default();
            let resp = dock_exec!(
                "get_attribute",
                json!({ "selector": sel, "attribute": "value" })
            )?;
            if !resp.ok {
                return Ok(assert_failure(
                    serde_json::Value::Null,
                    resp.error
                        .unwrap_or_else(|| "get_attribute failed".into()),
                ));
            }
            let actual_str = resp
                .value
                .as_object()
                .and_then(|m| m.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let passed = if exp.is_empty() {
                !actual_str.is_empty()
            } else {
                actual_str == exp
            };
            if passed {
                Ok(assert_success(serde_json::Value::String(actual_str)))
            } else {
                Ok(assert_failure(
                    serde_json::Value::String(actual_str),
                    "value mismatch".to_string(),
                ))
            }
        }
        "count" => {
            let sel = selector
                .clone()
                .ok_or_else(|| anyhow::anyhow!("assert count requires selector"))?;
            let op_str = op.clone().unwrap_or_else(|| "==".to_string());
            let want = count.unwrap_or(0);
            let resp = dock_exec!("count", json!({ "selector": sel }))?;
            if !resp.ok {
                return Ok(assert_failure(
                    serde_json::Value::Null,
                    resp.error.unwrap_or_else(|| "count failed".into()),
                ));
            }
            let actual_count = resp
                .value
                .as_object()
                .and_then(|m| m.get("count").or_else(|| m.get("value")))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let passed = compare_count(actual_count, &op_str, want);
            if passed {
                Ok(assert_success(serde_json::Value::from(actual_count)))
            } else {
                Ok(assert_failure(
                    serde_json::Value::from(actual_count),
                    format!("count {} {} {} failed", actual_count, op_str, want),
                ))
            }
        }
        "console_clean" => {
            let resp = dock_exec!("console_logs", json!({ "level": "error" }))?;
            if !resp.ok {
                return Ok(assert_failure(
                    serde_json::Value::Null,
                    resp.error.unwrap_or_else(|| "console_logs failed".into()),
                ));
            }
            let entries = resp
                .value
                .as_object()
                .and_then(|m| m.get("entries"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let error_count = entries
                .iter()
                .filter(|e| {
                    e.as_object()
                        .and_then(|o| o.get("level"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.eq_ignore_ascii_case("error"))
                        .unwrap_or(false)
                })
                .count();
            if error_count == 0 {
                Ok(assert_success(serde_json::Value::from(0)))
            } else {
                Ok(assert_failure(
                    serde_json::Value::from(error_count as u64),
                    format!("found {error_count} console errors"),
                ))
            }
        }
        other => Ok(ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("Unsupported assert_kind '{other}'")),
        }),
    }
}

fn compare_count(actual: i64, op: &str, expected: i64) -> bool {
    match op.trim() {
        "==" | "=" | "eq" => actual == expected,
        "!=" | "ne" => actual != expected,
        ">" | "gt" => actual > expected,
        ">=" | "gte" => actual >= expected,
        "<" | "lt" => actual < expected,
        "<=" | "lte" => actual <= expected,
        _ => actual == expected,
    }
}
