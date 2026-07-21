// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod export;

use crate::config::traits::ChannelConfig;
use crate::providers::{is_glm_alias, is_zai_alias};
use crate::security::{AutonomyLevel, DomainMatcher};
use anyhow::{Context, Result};
use directories::UserDirs;
use reqwest as reqwest_proxy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use tokio::fs::File;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

pub use crate::config::domain::memory::{
    MemoryConfig, MemoryPolicyConfig, QdrantConfig, SearchMode,
};

const SUPPORTED_PROXY_SERVICE_KEYS: &[&str] = &[
    "provider.anthropic",
    "provider.compatible",
    "provider.copilot",
    "provider.gemini",
    "provider.glm",
    "provider.ollama",
    "provider.openai",
    "provider.openrouter",
    "channel.dingtalk",
    "channel.discord",
    "channel.feishu",
    "channel.lark",
    "channel.matrix",
    "channel.mattermost",
    "channel.nextcloud_talk",
    "channel.qq",
    "channel.signal",
    "channel.slack",
    "channel.telegram",
    "channel.wati",
    "channel.whatsapp",
    "tool.browser",
    "tool.composio",
    "tool.http_request",
    "tool.pushover",
    "tool.web_search",
    "memory.embeddings",
    "tunnel.custom",
    "transcription.groq",
];

const SUPPORTED_PROXY_SERVICE_SELECTORS: &[&str] = &[
    "provider.*",
    "channel.*",
    "tool.*",
    "memory.*",
    "tunnel.*",
    "transcription.*",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProxyScope {

    Environment,

    #[default]
    Internal,

    Services,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProxyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub http_proxy: Option<String>,
    #[serde(default)]
    pub https_proxy: Option<String>,
    #[serde(default)]
    pub all_proxy: Option<String>,
    #[serde(default)]
    pub no_proxy: Vec<String>,
    #[serde(default)]
    pub scope: ProxyScope,
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default = "default_true")]
    pub system_detect: bool,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            http_proxy: None,
            https_proxy: None,
            all_proxy: None,
            no_proxy: Vec::new(),
            scope: ProxyScope::Internal,
            services: Vec::new(),
            system_detect: true,
        }
    }
}

impl ProxyConfig {

    pub fn supported_service_keys() -> &'static [&'static str] {
        SUPPORTED_PROXY_SERVICE_KEYS
    }
    pub fn supported_service_selectors() -> &'static [&'static str] {
        SUPPORTED_PROXY_SERVICE_SELECTORS
    }

    pub fn registered_service_keys() -> Vec<String> {
        crate::services::proxy::registry::snapshot_services()
    }
    pub fn has_any_proxy_url(&self) -> bool {
        normalize_proxy_url_option(self.http_proxy.as_deref()).is_some()
            || normalize_proxy_url_option(self.https_proxy.as_deref()).is_some()
            || normalize_proxy_url_option(self.all_proxy.as_deref()).is_some()
    }
    pub fn has_explicit_proxy(&self) -> bool {
        self.enabled && self.has_any_proxy_url()
    }
    pub fn normalized_services(&self) -> Vec<String> {
        normalize_service_list(self.services.clone())
    }
    pub fn normalized_no_proxy(&self) -> Vec<String> {
        normalize_no_proxy_list(self.no_proxy.clone())
    }
    pub fn validate(&self) -> Result<()> {
        for (field, value) in [
            ("http_proxy", self.http_proxy.as_deref()),
            ("https_proxy", self.https_proxy.as_deref()),
            ("all_proxy", self.all_proxy.as_deref()),
        ] {
            if let Some(url) = normalize_proxy_url_option(value) {
                validate_proxy_url(field, &url)?;
            }
        }
        for selector in self.normalized_services() {
            if !is_supported_proxy_service_selector(&selector) {
                anyhow::bail!("Unsupported proxy service selector '{selector}'");
            }
        }
        for entry in self.normalized_no_proxy() {
            validate_no_proxy_entry(&entry)?;
        }
        if self.enabled && !self.has_any_proxy_url() {
            anyhow::bail!("Proxy is enabled but no proxy URL is configured");
        }
        if self.enabled
            && self.scope == ProxyScope::Services
            && self.normalized_services().is_empty()
        {
            anyhow::bail!("proxy.scope='services' requires a non-empty proxy.services list");
        }
        if self.enabled
            && self.scope == ProxyScope::Environment
            && !self.normalized_services().is_empty()
        {
            anyhow::bail!(
                "proxy.scope='environment' applies to the OS env only and is mutually exclusive with proxy.services; remove proxy.services or switch scope to 'services'"
            );
        }
        Ok(())
    }
    pub fn should_apply_to_service(&self, service_key: &str) -> bool {
        if !self.enabled {
            return false;
        }
        match &self.scope {
            ProxyScope::Environment => false,
            ProxyScope::Internal => true,
            ProxyScope::Services => {
                let sk = service_key.trim().to_ascii_lowercase();
                if sk.is_empty() {
                    return false;
                }
                self.normalized_services()
                    .iter()
                    .any(|sel| service_selector_matches(sel, &sk))
            }
        }
    }
    pub fn apply_to_reqwest_builder(
        &self,
        mut builder: reqwest::ClientBuilder,
        service_key: &str,
    ) -> reqwest::ClientBuilder {
        if !self.should_apply_to_service(service_key) {
            return builder;
        }
        let no_proxy = self.no_proxy_value();
        type ProxyCtor = fn(&str) -> Result<reqwest_proxy::Proxy, reqwest::Error>;
        let all_ctor: ProxyCtor = |u| reqwest_proxy::Proxy::all(u);
        let http_ctor: ProxyCtor = |u| reqwest_proxy::Proxy::http(u);
        let https_ctor: ProxyCtor = |u| reqwest_proxy::Proxy::https(u);
        for (url_opt, make) in [
            (
                normalize_proxy_url_option(self.all_proxy.as_deref()),
                all_ctor,
            ),
            (
                normalize_proxy_url_option(self.http_proxy.as_deref()),
                http_ctor,
            ),
            (
                normalize_proxy_url_option(self.https_proxy.as_deref()),
                https_ctor,
            ),
        ] {
            if let Some(url) = url_opt {
                match make(&url) {
                    Ok(p) => {
                        builder = builder.proxy(apply_no_proxy(p, no_proxy.clone()));
                    }
                    Err(e) => {
                        tracing::warn!(proxy_url = %url, service_key, "Ignoring invalid proxy URL: {e}");
                    }
                }
            }
        }
        builder
    }
    pub fn apply_to_process_env(&self) {
        let no_proxy_joined = {
            let list = self.normalized_no_proxy();
            if !list.is_empty() {
                Some(list.join(","))
            } else {
                None
            }
        };
        apply_proxy_env_batch(
            normalize_proxy_url_option(self.http_proxy.as_deref()).as_deref(),
            normalize_proxy_url_option(self.https_proxy.as_deref()).as_deref(),
            normalize_proxy_url_option(self.all_proxy.as_deref()).as_deref(),
            no_proxy_joined.as_deref(),
        );
    }
    pub fn clear_process_env() {
        apply_proxy_env_batch(None, None, None, None);
    }
    fn no_proxy_value(&self) -> Option<reqwest::NoProxy> {
        let joined = {
            let list = self.normalized_no_proxy();
            if !list.is_empty() {
                Some(list.join(","))
            } else {
                None
            }
        };
        joined.as_deref().and_then(reqwest::NoProxy::from_string)
    }
}

fn apply_no_proxy(
    proxy: reqwest_proxy::Proxy,
    no_proxy: Option<reqwest::NoProxy>,
) -> reqwest_proxy::Proxy {
    proxy.no_proxy(no_proxy)
}
pub(crate) fn normalize_proxy_url_option(raw: Option<&str>) -> Option<String> {
    let v = raw?.trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}
fn normalize_no_proxy_list(values: Vec<String>) -> Vec<String> {
    normalize_comma_values(values)
}
fn normalize_service_list(values: Vec<String>) -> Vec<String> {
    let mut r = normalize_comma_values(values)
        .into_iter()
        .map(|v| v.to_ascii_lowercase())
        .collect::<Vec<_>>();
    r.sort_unstable();
    r.dedup();
    r
}
fn normalize_comma_values(values: Vec<String>) -> Vec<String> {
    let mut o = Vec::new();
    for v in values {
        for p in v.split(',') {
            let n = p.trim();
            if !n.is_empty() {
                o.push(n.to_string());
            }
        }
    }
    o.sort_unstable();
    o.dedup();
    o
}
fn is_supported_proxy_service_selector(selector: &str) -> bool {
    if crate::services::proxy::registry::is_valid_selector(selector) {
        return true;
    }

    SUPPORTED_PROXY_SERVICE_KEYS
        .iter()
        .any(|k| k.eq_ignore_ascii_case(selector))
        || SUPPORTED_PROXY_SERVICE_SELECTORS
            .iter()
            .any(|k| k.eq_ignore_ascii_case(selector))
}
fn service_selector_matches(selector: &str, service_key: &str) -> bool {
    if selector == service_key {
        return true;
    }
    if let Some(prefix) = selector.strip_suffix(".*") {
        return service_key.starts_with(prefix)
            && service_key
                .strip_prefix(prefix)
                .is_some_and(|s| s.starts_with('.'));
    }
    false
}
fn validate_proxy_url(field: &str, url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url)
        .with_context(|| format!("Invalid {field} URL: '{url}' is not a valid URL"))?;
    match parsed.scheme() {
        "http" | "https" | "socks5" | "socks5h" | "socks" => {}
        scheme => {
            anyhow::bail!("Invalid {field} URL scheme '{scheme}'");
        }
    }
    if parsed.host_str().is_none() {
        anyhow::bail!("Invalid {field} URL: host is required");
    }
    Ok(())
}

fn validate_no_proxy_entry(entry: &str) -> Result<()> {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        anyhow::bail!("proxy.no_proxy contains an empty entry");
    }
    if trimmed == "*" {
        return Ok(());
    }

    if let Some(idx) = trimmed.rfind('/') {
        let (prefix, suffix) = (&trimmed[..idx], &trimmed[idx + 1..]);
        let bits: u32 = suffix.parse().map_err(|_| {
            anyhow::anyhow!(
                "proxy.no_proxy entry '{trimmed}' has an invalid CIDR mask '{suffix}'; expected an integer"
            )
        })?;
        let max_bits = if prefix.contains(':') { 128 } else { 32 };
        if bits > max_bits {
            anyhow::bail!(
                "proxy.no_proxy entry '{trimmed}' CIDR mask '{bits}' exceeds the maximum of {max_bits}"
            );
        }
        return Ok(());
    }

    let body = trimmed.trim_start_matches('.');
    let body = body.trim_start_matches("*.");
    if body.is_empty() {
        anyhow::bail!("proxy.no_proxy entry '{trimmed}' has an empty host component");
    }
    if body
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '/' | '\\' | '?' | '#' | '@' | ',' | ';'))
    {
        anyhow::bail!(
            "proxy.no_proxy entry '{trimmed}' contains invalid characters; expected hostname, *.tld, or CIDR"
        );
    }
    Ok(())
}
fn apply_proxy_env_batch(
    http: Option<&str>,
    https: Option<&str>,
    all: Option<&str>,
    no_proxy: Option<&str>,
) {
    let pairs: [(&str, Option<&str>); 8] = [
        ("HTTP_PROXY", http),
        ("http_proxy", http),
        ("HTTPS_PROXY", https),
        ("https_proxy", https),
        ("ALL_PROXY", all),
        ("all_proxy", all),
        ("NO_PROXY", no_proxy),
        ("no_proxy", no_proxy),
    ];
    crate::util::set_runtime_vars_batch(&pairs);
}

pub fn parse_proxy_scope(raw: &str) -> Option<ProxyScope> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "environment" | "env" => Some(ProxyScope::Environment),
        "sen" | "internal" | "core" => Some(ProxyScope::Internal),
        "services" | "service" => Some(ProxyScope::Services),
        _ => None,
    }
}
pub fn parse_proxy_enabled(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[derive(Debug)]
pub struct EnvOverrideBaseline {
    pub pristine: toml::Value,
    pub env_applied: toml::Value,
}

fn strip_env_overrides(
    pristine: Option<&toml::Value>,
    env_applied: Option<&toml::Value>,
    current: toml::Value,
) -> Option<toml::Value> {
    if env_applied == Some(&current) {
        return pristine.cloned();
    }
    match current {
        toml::Value::Table(table) => {
            let pristine_table = pristine.and_then(toml::Value::as_table);
            let env_table = env_applied.and_then(toml::Value::as_table);
            let mut out = toml::map::Map::with_capacity(table.len());
            for (key, value) in table {
                let sub_pristine = pristine_table.and_then(|t| t.get(&key));
                let sub_env = env_table.and_then(|t| t.get(&key));
                if let Some(kept) = strip_env_overrides(sub_pristine, sub_env, value) {
                    out.insert(key, kept);
                }
            }
            Some(toml::Value::Table(out))
        }
        other => Some(other),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Config {

    #[serde(skip)]
    pub workspace_dir: PathBuf,

    #[serde(skip)]
    pub config_path: PathBuf,

    #[serde(skip)]
    #[schemars(skip)]
    pub env_override_baseline: Option<std::sync::Arc<EnvOverrideBaseline>>,

    #[serde(default)]
    pub api_key: Option<String>,

    pub api_url: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_path: Option<String>,

    #[serde(alias = "model_provider")]
    pub default_provider: Option<String>,

    #[serde(alias = "model")]
    pub default_model: Option<String>,

    #[serde(default)]
    pub model_providers: HashMap<String, ModelProviderConfig>,

    #[serde(
        default = "default_temperature",
        deserialize_with = "deserialize_temperature"
    )]
    pub default_temperature: f64,

    #[serde(default = "default_provider_timeout_secs")]
    pub provider_timeout_secs: u64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_max_tokens: Option<u32>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub model_context_windows: HashMap<String, u32>,

    #[serde(default)]
    pub extra_headers: HashMap<String, String>,

    #[serde(default)]
    pub observability: ObservabilityConfig,

    #[serde(default)]
    pub autonomy: AutonomyConfig,

    #[serde(default)]
    pub trust: crate::trust::TrustConfig,

    #[serde(default)]
    pub security: SecurityConfig,

    #[serde(default)]
    pub backup: BackupConfig,

    #[serde(default)]
    pub data_retention: DataRetentionConfig,

    #[serde(default)]
    pub cloud_ops: CloudOpsConfig,

    #[serde(default, skip_serializing_if = "HandsConfig::is_disabled")]
    pub hands: HandsConfig,

    #[serde(default, skip_serializing_if = "BuddyConfig::is_disabled")]
    pub buddy: BuddyConfig,

    #[serde(default)]
    pub security_ops: SecurityOpsConfig,

    #[serde(default)]
    pub runtime: RuntimeConfig,

    #[serde(default)]
    pub reliability: ReliabilityConfig,

    #[serde(default)]
    pub scheduler: SchedulerConfig,

    #[serde(default)]
    pub agent: AgentConfig,

    #[serde(default)]
    pub pacing: PacingConfig,

    #[serde(default)]
    pub agent_runtime: AgentRuntimeExtras,

    #[serde(default)]
    pub skills: SkillsConfig,

    #[serde(default)]
    pub pipeline: PipelineConfig,

    #[serde(default)]
    pub model_routes: Vec<ModelRouteConfig>,

    #[serde(default)]
    pub saved_models: Vec<SavedModel>,

    #[serde(default)]
    pub embedding_routes: Vec<EmbeddingRouteConfig>,

    #[serde(default)]
    pub query_classification: QueryClassificationConfig,

    #[serde(default)]
    pub heartbeat: HeartbeatConfig,

    #[serde(default)]
    pub lan: LanConfig,

    #[serde(default)]
    pub cron: CronConfig,

    #[serde(default)]
    pub channels_config: ChannelsConfig,

    #[serde(default)]
    pub memory: MemoryConfig,

    #[serde(default)]
    pub memory_runtime: crate::config::domain::MemoryRuntimeExtras,

    #[serde(default)]
    pub storage: StorageConfig,

    #[serde(default)]
    pub tunnel: TunnelConfig,

    #[serde(default)]
    pub gateway: GatewayConfig,

    #[serde(default)]
    pub rpc: RpcConfig,

    #[serde(default)]
    pub composio: ComposioConfig,

    #[serde(default)]
    pub microsoft365: Microsoft365Config,

    #[serde(default)]
    pub secrets: SecretsConfig,

    #[serde(default)]
    pub browser: BrowserConfig,

    #[serde(default)]
    pub browser_delegate: crate::tools::browser::delegate::BrowserDelegateConfig,

    #[serde(default)]
    pub http_request: HttpRequestConfig,

    #[serde(default)]
    pub multimodal: MultimodalConfig,

    #[serde(default)]
    pub media_pipeline: MediaPipelineConfig,

    #[serde(default)]
    pub web_fetch: WebFetchConfig,

    #[serde(default)]
    pub link_enricher: LinkEnricherConfig,

    #[serde(default)]
    pub text_browser: TextBrowserConfig,

    #[serde(default)]
    pub web_search: WebSearchConfig,

    #[serde(default)]
    pub project_intel: ProjectIntelConfig,

    #[serde(default)]
    pub google_workspace: GoogleWorkspaceConfig,

    #[serde(default)]
    pub proxy: ProxyConfig,

    #[serde(default)]
    pub identity: IdentityConfig,

    #[serde(default)]
    pub cost: CostConfig,

    #[serde(default)]
    pub evolution: crate::config::domain::evolution::EvolutionConfig,

    #[serde(default)]
    pub peripherals: PeripheralsConfig,

    #[serde(default)]
    pub delegate: DelegateToolConfig,

    #[serde(default)]
    pub agents: HashMap<String, DelegateAgentConfig>,

    #[serde(default)]
    pub swarms: HashMap<String, SwarmConfig>,

    #[serde(default)]
    pub teams: TeamsConfig,

    #[serde(default)]
    pub hooks: HooksConfig,

    #[serde(default)]
    pub hardware: HardwareConfig,

    #[serde(default)]
    pub transcription: TranscriptionConfig,

    #[serde(default)]
    pub tts: TtsConfig,

    #[serde(default, alias = "mcpServers")]
    pub mcp: McpConfig,

    #[serde(default)]
    pub nodes: NodesConfig,

    #[serde(default)]
    pub workspace: WorkspaceConfig,

    #[serde(default)]
    pub notion: NotionConfig,

    #[serde(default)]
    pub jira: JiraConfig,

    #[serde(default)]
    pub knowledge: KnowledgeConfig,

    #[serde(default)]
    pub linkedin: LinkedInConfig,

    #[serde(default)]
    pub image_gen: ImageGenConfig,

    #[serde(default)]
    pub plugins: PluginsConfig,

    #[serde(default)]
    pub locale: Option<String>,

    #[serde(default)]
    pub verifiable_intent: VerifiableIntentConfig,

    #[serde(default)]
    pub claude_code: ClaudeCodeConfig,

    #[serde(default)]
    pub claude_code_runner: ClaudeCodeRunnerConfig,

    #[serde(default)]
    pub codex_cli: CodexCliConfig,

    #[serde(default)]
    pub gemini_cli: GeminiCliConfig,

    #[serde(default)]
    pub opencode_cli: OpenCodeCliConfig,

    #[serde(default)]
    pub sop: SopConfig,

    #[serde(default)]
    pub shell_tool: ShellToolConfig,

    #[serde(default)]
    pub guardrails: crate::guardrails::GuardrailsConfig,

    #[serde(default)]
    pub plan_mode: crate::agent::plan_mode::PlanModeConfig,

    #[serde(default)]
    pub intent_analysis: crate::agent::intent::IntentAnalysisConfig,

    #[serde(default)]
    pub auto_title: crate::agent::auto_title::AutoTitleConfig,

    #[serde(default)]
    pub suggestions: crate::agent::suggestions::SuggestionsConfig,

    #[serde(default)]
    pub tool_groups: crate::tools::handler::groups::ToolGroupsConfig,

    #[serde(default)]
    pub user_profile: crate::agent::user::profile::UserProfileConfig,

    #[serde(default)]
    pub self_eval: crate::agent::self_assess::eval::SelfEvalConfig,

    #[serde(default)]
    pub feedback: crate::agent::reward::feedback::FeedbackConfig,

    #[serde(default)]
    pub experience: crate::agent::reward::experience::ExperienceConfig,

    #[serde(default)]
    pub prompt_optimizer: crate::agent::prompt::optimizer::PromptOptimizerConfig,

    #[serde(default)]
    pub skill_evolution: crate::agent::skill_evolution::SkillEvolutionConfig,

    #[serde(default)]
    pub reinforcement: crate::agent::reward::reinforcement::ReinforcementConfig,

    #[serde(default)]
    pub rbac: crate::security::rbac::RbacConfig,

    #[serde(default)]
    pub tool_output_compressor: crate::agent::tool_handler::output_compressor::ToolOutputCompressorConfig,

    #[serde(default)]
    pub code_rag: CodeRagConfig,

    #[serde(default)]
    pub token_budget: crate::agent::token::budget::TokenBudgetConfig,

    #[serde(default)]
    pub token_saver: TokenSaverConfig,

    #[serde(default)]
    pub custom_tools: CustomToolsConfig,

    #[serde(default)]
    pub lsp: LspConfig,

    #[serde(default)]
    pub permissions: crate::security::permissions::PermissionsConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CustomToolsConfig {

    #[serde(default)]
    pub tools: Vec<CustomToolDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CustomToolDef {

    pub name: String,

    #[serde(default)]
    pub description: String,

    pub command: String,

    #[serde(default)]
    pub args: Vec<String>,

    #[serde(default)]
    pub cwd: Option<String>,

    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,

    #[serde(default = "default_custom_tool_timeout")]
    pub timeout_secs: u64,

    #[serde(default)]
    pub schema: serde_json::Value,

    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_custom_tool_timeout() -> u64 {
    60
}

impl Default for CustomToolDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            command: String::new(),
            args: Vec::new(),
            cwd: None,
            env: std::collections::HashMap::new(),
            timeout_secs: default_custom_tool_timeout(),
            schema: serde_json::json!({ "type": "object" }),
            enabled: true,
        }
    }
}

impl CustomToolDef {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let trimmed_name = self.name.trim();
        if trimmed_name.is_empty() {
            errors.push("name must be non-empty".into());
        } else if !trimmed_name
            .chars()
            .next()
            .map(|c| c.is_ascii_lowercase() || c == '_')
            .unwrap_or(false)
            || !trimmed_name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            errors.push(format!(
                "name '{trimmed_name}' must match [a-z_][a-z0-9_]*"
            ));
        }
        if self.command.trim().is_empty() {
            errors.push("command must be non-empty".into());
        }
        if self.timeout_secs == 0 {
            errors.push("timeout_secs must be > 0".into());
        }
        errors
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TokenSaverLevel {

    Conservative,

    #[default]
    Balanced,

    Aggressive,
}

impl TokenSaverLevel {
    pub(crate) fn to_runtime(self) -> crate::token_saver::CompactLevel {
        match self {
            Self::Conservative => crate::token_saver::CompactLevel::Conservative,
            Self::Balanced => crate::token_saver::CompactLevel::Balanced,
            Self::Aggressive => crate::token_saver::CompactLevel::Aggressive,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TokenSaverConfig {

    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default)]
    pub level: TokenSaverLevel,

    #[serde(default = "default_true")]
    pub tee_enabled: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<PathBuf>,

    #[serde(default)]
    pub exclude_commands: Vec<String>,

    #[serde(default = "default_true")]
    pub tracking_enabled: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_filters_dir: Option<PathBuf>,
}

impl Default for TokenSaverConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            level: TokenSaverLevel::Balanced,
            tee_enabled: true,
            data_dir: None,
            exclude_commands: Vec::new(),
            tracking_enabled: true,
            custom_filters_dir: None,
        }
    }
}

impl TokenSaverConfig {

    pub fn to_runtime_ctx(&self) -> crate::token_saver::CompactContext {
        let data_dir = self.data_dir.clone().unwrap_or_else(|| {
            directories::ProjectDirs::from("", "", "sen")
                .map(|d| d.data_dir().to_path_buf())
                .unwrap_or_else(|| std::env::temp_dir().join("sen"))
        });
        crate::token_saver::CompactContext {
            level: self.level.to_runtime(),
            tee_enabled: self.tee_enabled,
            tracking_enabled: self.tracking_enabled,
            data_dir,
            custom_filters_dir: self.custom_filters_dir.clone(),
            raw_byte_cap: 1_048_576,
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub active_workspace: Option<String>,

    #[serde(default = "default_workspaces_dir")]
    pub workspaces_dir: String,

    #[serde(default = "default_true")]
    pub isolate_memory: bool,

    #[serde(default = "default_true")]
    pub isolate_secrets: bool,

    #[serde(default = "default_true")]
    pub isolate_audit: bool,

    #[serde(default)]
    pub cross_workspace_search: bool,
}

fn default_workspaces_dir() -> String {
    "~/.senweavercoding/workspaces".to_string()
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            active_workspace: None,
            workspaces_dir: default_workspaces_dir(),
            isolate_memory: true,
            isolate_secrets: true,
            isolate_audit: true,
            cross_workspace_search: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CustomHttpHeader {
    pub name: String,
    pub value: String,
    #[serde(default = "default_custom_http_header_enabled")]
    pub enabled: bool,
}

impl Default for CustomHttpHeader {
    fn default() -> Self {
        Self {
            name: String::new(),
            value: String::new(),
            enabled: true,
        }
    }
}

fn default_custom_http_header_enabled() -> bool {
    true
}

pub const DISALLOWED_CUSTOM_HEADER_NAMES: &[&str] = &[
    "content-type",
    "content-length",
    "host",
    "authorization",
    "transfer-encoding",
    "connection",
    "proxy-authorization",
];

#[must_use]
pub fn is_valid_http_header_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    name.bytes().all(|b| matches!(b,
        b'A'..=b'Z'
        | b'a'..=b'z'
        | b'0'..=b'9'
        | b'!' | b'#' | b'$' | b'%' | b'&' | b'\''
        | b'*' | b'+' | b'-' | b'.' | b'^' | b'_'
        | b'`' | b'|' | b'~'))
}

#[must_use]
pub fn is_disallowed_custom_header(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    DISALLOWED_CUSTOM_HEADER_NAMES
        .iter()
        .any(|disallowed| *disallowed == lower)
}

#[must_use]
pub fn is_valid_http_header_value(value: &str) -> bool {
    !value.contains('\r') && !value.contains('\n')
}

#[must_use]
pub fn build_custom_headers_map(
    headers: &[CustomHttpHeader],
) -> std::collections::HashMap<String, String> {
    let mut out: std::collections::HashMap<String, String> =
        std::collections::HashMap::with_capacity(headers.len());
    for header in headers {
        if !header.enabled {
            continue;
        }
        let trimmed_name = header.name.trim();
        if trimmed_name.is_empty() {
            continue;
        }
        if !is_valid_http_header_name(trimmed_name) {
            tracing::warn!(
                header_name = trimmed_name,
                "skipping invalid custom HTTP header name (contains characters outside RFC 7230 token set)"
            );
            continue;
        }
        if is_disallowed_custom_header(trimmed_name) {
            tracing::warn!(
                header_name = trimmed_name,
                "skipping reserved/disallowed custom HTTP header (managed by the provider itself)"
            );
            continue;
        }
        if !is_valid_http_header_value(&header.value) {
            tracing::warn!(
                header_name = trimmed_name,
                "skipping custom HTTP header whose value contains CR/LF (potential header injection)"
            );
            continue;
        }
        out.insert(trimmed_name.to_string(), header.value.clone());
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct ModelProviderConfig {

    #[serde(default)]
    pub name: Option<String>,

    #[serde(default)]
    pub base_url: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_path: Option<String>,

    #[serde(default)]
    pub wire_api: Option<String>,

    #[serde(default)]
    pub requires_openai_auth: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azure_openai_resource: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azure_openai_deployment: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azure_openai_api_version: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub models: std::collections::HashMap<String, String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_names: Vec<String>,

    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub model_context_windows: std::collections::HashMap<String, u32>,

    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub model_types: std::collections::HashMap<String, Vec<String>>,

    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub model_pricing: std::collections::HashMap<String, ModelPricing>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_headers: Vec<CustomHttpHeader>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
}

pub const DEFAULT_MODEL_TYPE: &str = "text";

pub const MODEL_TYPES: &[&str] = &[
    "text",
    "image-generation",
    "video-generation",
    "audio-generation",
    "image-understanding",
    "video-understanding",
    "speech-recognition",
    "embedding",
    "rerank",
    "music-generation",
];

pub fn is_known_model_type(value: &str) -> bool {
    MODEL_TYPES.contains(&value)
}

pub fn sanitize_model_types<I, S>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    for value in values {
        let trimmed = value.as_ref().trim();
        if trimmed.is_empty() || !is_known_model_type(trimmed) {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

pub fn classify_model_type(model_id: &str) -> Vec<String> {
    let id = model_id.trim().to_ascii_lowercase();
    if id.is_empty() {
        return vec![DEFAULT_MODEL_TYPE.to_string()];
    }

    let has = |needle: &str| id.contains(needle);

    if has("rerank") {
        return vec!["rerank".to_string()];
    }
    if has("embed") || has("bge") || has("gte") || has("e5-") || has("text-embedding") {
        return vec!["embedding".to_string()];
    }
    if has("whisper") || has("transcrib") || has("-stt") || has("speech-to-text") || has("asr") {
        return vec!["speech-recognition".to_string()];
    }
    if has("-tts") || has("tts-") || has("text-to-speech") || has("audio-generation") {
        return vec!["audio-generation".to_string()];
    }
    if has("dall-e")
        || has("dalle")
        || has("imagen")
        || has("flux")
        || has("stable-diffusion")
        || has("stable-image")
        || has("-sdxl")
        || has("kontext")
        || has("seedream")
        || has("image-generation")
    {
        return vec!["image-generation".to_string()];
    }
    if has("video") || has("sora") || has("veo") || has("kling") || has("seedance") {
        return vec!["video-generation".to_string()];
    }

    let mut types = vec![DEFAULT_MODEL_TYPE.to_string()];
    if has("vision")
        || has("-vl")
        || has("vl-")
        || has("-vl-")
        || has("multimodal")
        || has("omni")
        || has("4o")
    {
        types.push("image-understanding".to_string());
    }
    types
}

impl ModelProviderConfig {

    pub fn types_for_model(&self, model: &str) -> Vec<String> {
        let needle = model.trim();
        if needle.is_empty() {
            return vec![DEFAULT_MODEL_TYPE.to_string()];
        }
        let resolved = self
            .model_types
            .get(needle)
            .map(|types| sanitize_model_types(types.iter().map(String::as_str)))
            .filter(|types| !types.is_empty())
            .unwrap_or_default();
        if resolved.is_empty() {
            vec![DEFAULT_MODEL_TYPE.to_string()]
        } else {
            resolved
        }
    }

    pub fn model_supports_type(&self, model: &str, model_type: &str) -> bool {
        self.types_for_model(model)
            .iter()
            .any(|t| t == model_type)
    }

    pub fn explicit_vision_for_model(&self, model: &str) -> Option<bool> {
        let needle = model.trim();
        if needle.is_empty() {
            return None;
        }
        let configured = self.model_types.get(needle)?;
        let sanitized = sanitize_model_types(configured.iter().map(String::as_str));
        if sanitized.is_empty() {
            return None;
        }
        Some(sanitized.iter().any(|t| t == "image-understanding"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DelegateToolConfig {

    #[serde(default = "default_delegate_timeout_secs")]
    pub timeout_secs: u64,

    #[serde(default = "default_delegate_agentic_timeout_secs")]
    pub agentic_timeout_secs: u64,
}

impl Default for DelegateToolConfig {
    fn default() -> Self {
        Self {
            timeout_secs: DEFAULT_DELEGATE_TIMEOUT_SECS,
            agentic_timeout_secs: DEFAULT_DELEGATE_AGENTIC_TIMEOUT_SECS,
        }
    }
}

pub use crate::config::domain::delegate_agents::DelegateAgentConfig;

fn default_delegate_timeout_secs() -> u64 {
    DEFAULT_DELEGATE_TIMEOUT_SECS
}

fn default_delegate_agentic_timeout_secs() -> u64 {
    DEFAULT_DELEGATE_AGENTIC_TIMEOUT_SECS
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SwarmStrategy {

    Sequential,

    Parallel,

    Router,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SwarmConfig {

    pub agents: Vec<String>,

    pub strategy: SwarmStrategy,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub router_prompt: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(default = "default_swarm_timeout_secs")]
    pub timeout_secs: u64,
}

const DEFAULT_SWARM_TIMEOUT_SECS: u64 = 300;

fn default_swarm_timeout_secs() -> u64 {
    DEFAULT_SWARM_TIMEOUT_SECS
}

pub const TEMPERATURE_RANGE: std::ops::RangeInclusive<f64> = 0.0..=2.0;

const DEFAULT_TEMPERATURE: f64 = 0.7;

fn default_temperature() -> f64 {
    DEFAULT_TEMPERATURE
}

const DEFAULT_PROVIDER_TIMEOUT_SECS: u64 = 120;

fn default_provider_timeout_secs() -> u64 {
    DEFAULT_PROVIDER_TIMEOUT_SECS
}

pub const DEFAULT_DELEGATE_TIMEOUT_SECS: u64 = 120;

pub const DEFAULT_DELEGATE_AGENTIC_TIMEOUT_SECS: u64 = 300;

pub fn validate_temperature(value: f64) -> std::result::Result<f64, String> {
    if TEMPERATURE_RANGE.contains(&value) {
        Ok(value)
    } else {
        Err(format!(
            "temperature {value} is out of range (expected {}..={})",
            TEMPERATURE_RANGE.start(),
            TEMPERATURE_RANGE.end()
        ))
    }
}

fn deserialize_temperature<'de, D>(deserializer: D) -> std::result::Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: f64 = serde::Deserialize::deserialize(deserializer)?;
    validate_temperature(value).map_err(serde::de::Error::custom)
}

pub(crate) fn normalize_reasoning_effort(value: &str) -> std::result::Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "minimal" | "low" | "medium" | "high" | "xhigh" => Ok(normalized),
        _ => Err(format!(
            "reasoning_effort {value:?} is invalid (expected one of: minimal, low, medium, high, xhigh)"
        )),
    }
}

pub(crate) fn deserialize_reasoning_effort_opt<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Option<String> = Option::deserialize(deserializer)?;
    value
        .map(|raw| normalize_reasoning_effort(&raw).map_err(serde::de::Error::custom))
        .transpose()
}

pub use crate::config::domain::hardware::{HardwareConfig, HardwareTransport};

fn default_transcription_api_url() -> String {
    "https://api.groq.com/openai/v1/audio/transcriptions".into()
}

fn default_transcription_model() -> String {
    "whisper-large-v3-turbo".into()
}

fn default_transcription_max_duration_secs() -> u64 {
    120
}

fn default_transcription_provider() -> String {
    "groq".into()
}

fn default_openai_stt_model() -> String {
    "whisper-1".into()
}

fn default_deepgram_stt_model() -> String {
    "nova-2".into()
}

fn default_google_stt_language_code() -> String {
    "en-US".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TranscriptionConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_transcription_provider")]
    pub default_provider: String,

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_transcription_api_url")]
    pub api_url: String,

    #[serde(default = "default_transcription_model")]
    pub model: String,

    #[serde(default)]
    pub language: Option<String>,

    #[serde(default)]
    pub initial_prompt: Option<String>,

    #[serde(default = "default_transcription_max_duration_secs")]
    pub max_duration_secs: u64,

    #[serde(default)]
    pub openai: Option<OpenAiSttConfig>,

    #[serde(default)]
    pub deepgram: Option<DeepgramSttConfig>,

    #[serde(default)]
    pub assemblyai: Option<AssemblyAiSttConfig>,

    #[serde(default)]
    pub google: Option<GoogleSttConfig>,

    #[serde(default)]
    pub local_whisper: Option<LocalWhisperConfig>,

    #[serde(default)]
    pub transcribe_non_ptt_audio: bool,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_provider: default_transcription_provider(),
            api_key: None,
            api_url: default_transcription_api_url(),
            model: default_transcription_model(),
            language: None,
            initial_prompt: None,
            max_duration_secs: default_transcription_max_duration_secs(),
            openai: None,
            deepgram: None,
            assemblyai: None,
            google: None,
            local_whisper: None,
            transcribe_non_ptt_audio: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {

    #[default]
    Stdio,

    Http,

    Sse,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct McpServerConfig {

    pub name: String,

    #[serde(default)]
    pub transport: McpTransport,

    #[serde(default)]
    pub url: Option<String>,

    #[serde(default)]
    pub command: String,

    #[serde(default)]
    pub args: Vec<String>,

    #[serde(default)]
    pub env: HashMap<String, String>,

    #[serde(default)]
    pub headers: HashMap<String, String>,

    #[serde(default)]
    pub tool_timeout_secs: Option<u64>,

    #[serde(default = "default_mcp_server_enabled")]
    pub enabled: bool,
}

fn default_mcp_server_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct McpConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_deferred_loading")]
    pub deferred_loading: bool,

    #[serde(default, alias = "mcpServers")]
    pub servers: Vec<McpServerConfig>,
}

fn default_deferred_loading() -> bool {
    true
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            deferred_loading: default_deferred_loading(),
            servers: Vec::new(),
        }
    }
}

pub const MCP_MAX_TOOL_TIMEOUT_SECS: u64 = 3600;

pub fn validate_mcp_config(cfg: &McpConfig) -> anyhow::Result<()> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (i, s) in cfg.servers.iter().enumerate() {
        let name = s.name.trim();
        if name.is_empty() {
            anyhow::bail!("mcp.servers[{i}].name must not be empty");
        }
        if !seen.insert(name.to_ascii_lowercase()) {
            anyhow::bail!("mcp.servers[{i}] duplicate name `{name}`");
        }
        if let Some(t) = s.tool_timeout_secs {
            if t == 0 {
                anyhow::bail!("mcp.servers[{i}].tool_timeout_secs must be greater than 0");
            }
            if t > MCP_MAX_TOOL_TIMEOUT_SECS {
                anyhow::bail!(
                    "mcp.servers[{i}].tool_timeout_secs ({t}s) exceeds max ({MCP_MAX_TOOL_TIMEOUT_SECS}s)"
                );
            }
        }
        match s.transport {
            McpTransport::Stdio => {
                if s.command.trim().is_empty() {
                    anyhow::bail!("mcp.servers[{i}] stdio transport requires non-empty command");
                }
            }
            McpTransport::Http | McpTransport::Sse => {
                let url = s
                    .url
                    .as_deref()
                    .map(str::trim)
                    .filter(|u| !u.is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!("mcp.servers[{i}] {:?} transport requires url", s.transport)
                    })?;
                let parsed = reqwest::Url::parse(url)
                    .map_err(|e| anyhow::anyhow!("mcp.servers[{i}].url is not a valid URL: {e}"))?;
                match parsed.scheme() {
                    "http" | "https" => {}
                    other => anyhow::bail!(
                        "mcp.servers[{i}].url scheme `{other}` not allowed; expected http/https"
                    ),
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VerifiableIntentConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_vi_strictness")]
    pub strictness: String,
}

fn default_vi_strictness() -> String {
    "strict".to_owned()
}

impl Default for VerifiableIntentConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strictness: default_vi_strictness(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NodesConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_max_nodes")]
    pub max_nodes: usize,

    #[serde(default)]
    pub auth_token: Option<String>,
}

fn default_max_nodes() -> usize {
    16
}

impl Default for NodesConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_nodes: default_max_nodes(),
            auth_token: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TeamsConfig {

    #[serde(default)]
    pub sync_enabled: bool,

    #[serde(default = "default_max_team_size")]
    pub max_team_size: usize,

    #[serde(default = "default_team_message_channel_size")]
    pub message_channel_size: usize,
}

fn default_max_team_size() -> usize {
    20
}

fn default_team_message_channel_size() -> usize {
    256
}

impl Default for TeamsConfig {
    fn default() -> Self {
        Self {
            sync_enabled: false,
            max_team_size: default_max_team_size(),
            message_channel_size: default_team_message_channel_size(),
        }
    }
}

fn default_tts_provider() -> String {
    "openai".into()
}

fn default_tts_voice() -> String {
    "alloy".into()
}

fn default_tts_format() -> String {
    "mp3".into()
}

fn default_tts_max_text_length() -> usize {
    4096
}

fn default_openai_tts_model() -> String {
    "tts-1".into()
}

fn default_openai_tts_speed() -> f64 {
    1.0
}

fn default_elevenlabs_model_id() -> String {
    "eleven_monolingual_v1".into()
}

fn default_elevenlabs_stability() -> f64 {
    0.5
}

fn default_elevenlabs_similarity_boost() -> f64 {
    0.5
}

fn default_google_tts_language_code() -> String {
    "en-US".into()
}

fn default_edge_tts_binary_path() -> String {
    "edge-tts".into()
}

fn default_piper_tts_api_url() -> String {
    "http://127.0.0.1:5000/v1/audio/speech".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TtsConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_tts_provider")]
    pub default_provider: String,

    #[serde(default = "default_tts_voice")]
    pub default_voice: String,

    #[serde(default = "default_tts_format")]
    pub default_format: String,

    #[serde(default = "default_tts_max_text_length")]
    pub max_text_length: usize,

    #[serde(default)]
    pub openai: Option<OpenAiTtsConfig>,

    #[serde(default)]
    pub elevenlabs: Option<ElevenLabsTtsConfig>,

    #[serde(default)]
    pub google: Option<GoogleTtsConfig>,

    #[serde(default)]
    pub edge: Option<EdgeTtsConfig>,

    #[serde(default)]
    pub piper: Option<PiperTtsConfig>,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_provider: default_tts_provider(),
            default_voice: default_tts_voice(),
            default_format: default_tts_format(),
            max_text_length: default_tts_max_text_length(),
            openai: None,
            elevenlabs: None,
            google: None,
            edge: None,
            piper: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OpenAiTtsConfig {

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_openai_tts_model")]
    pub model: String,

    #[serde(default = "default_openai_tts_speed")]
    pub speed: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ElevenLabsTtsConfig {

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_elevenlabs_model_id")]
    pub model_id: String,

    #[serde(default = "default_elevenlabs_stability")]
    pub stability: f64,

    #[serde(default = "default_elevenlabs_similarity_boost")]
    pub similarity_boost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoogleTtsConfig {

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_google_tts_language_code")]
    pub language_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EdgeTtsConfig {

    #[serde(default = "default_edge_tts_binary_path")]
    pub binary_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PiperTtsConfig {

    #[serde(default = "default_piper_tts_api_url")]
    pub api_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolFilterGroupMode {

    Always,

    #[default]
    Dynamic,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolFilterGroup {

    #[serde(default)]
    pub mode: ToolFilterGroupMode,

    #[serde(default)]
    pub tools: Vec<String>,

    #[serde(default)]
    pub keywords: Vec<String>,

    #[serde(default)]
    pub filter_builtins: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OpenAiSttConfig {

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_openai_stt_model")]
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeepgramSttConfig {

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_deepgram_stt_model")]
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AssemblyAiSttConfig {

    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoogleSttConfig {

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_google_stt_language_code")]
    pub language_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LocalWhisperConfig {

    pub url: String,

    #[serde(default)]
    pub bearer_token: Option<String>,

    #[serde(default = "default_local_whisper_max_audio_bytes")]
    pub max_audio_bytes: usize,

    #[serde(default = "default_local_whisper_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_local_whisper_max_audio_bytes() -> usize {
    25 * 1024 * 1024
}

fn default_local_whisper_timeout_secs() -> u64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentConfig {

    #[serde(default)]
    pub compact_context: bool,

    #[serde(default = "default_agent_max_tool_iterations")]
    pub max_tool_iterations: usize,

    #[serde(default = "default_agent_max_history_messages")]
    pub max_history_messages: usize,

    #[serde(default = "default_agent_recent_turn_window")]
    pub recent_turn_window: usize,

    #[serde(default = "default_agent_recent_window_max_turns")]
    pub recent_window_max_turns: usize,

    #[serde(default = "default_agent_recent_window_token_ratio")]
    pub recent_window_token_ratio: f64,

    #[serde(default = "default_true")]
    pub recent_window_summary_enabled: bool,

    #[serde(default = "default_agent_recent_window_summary_batch_turns")]
    pub recent_window_summary_batch_turns: usize,

    #[serde(default = "default_agent_max_context_tokens")]
    pub max_context_tokens: usize,

    #[serde(default)]
    pub parallel_tools: bool,

    #[serde(default = "default_agent_tool_dispatcher")]
    pub tool_dispatcher: String,

    #[serde(default)]
    pub tool_call_dedup_exempt: Vec<String>,

    #[serde(default)]
    pub tool_filter_groups: Vec<ToolFilterGroup>,

    #[serde(default = "default_max_system_prompt_chars")]
    pub max_system_prompt_chars: usize,

    #[serde(default)]
    pub thinking: crate::agent::thinking::ThinkingConfig,

    #[serde(default)]
    pub history_pruning: crate::agent::history::pruner::HistoryPrunerConfig,

    #[serde(default)]
    pub context_aware_tools: bool,

    #[serde(default)]
    pub eval: crate::agent::eval::EvalConfig,

    #[serde(default)]
    pub auto_classify: Option<crate::agent::eval::AutoClassifyConfig>,

    #[serde(default)]
    pub context_compression: crate::agent::context::compressor::ContextCompressionConfig,

    #[serde(default)]
    pub global_directives: Vec<GlobalDirective>,

    #[serde(default)]
    pub project_config_dir: Option<String>,

    #[serde(default)]
    pub auto_index: AutoIndexConfig,

    #[serde(default = "default_builtin_tool_deferred_loading")]
    pub builtin_tool_deferred_loading: bool,

    #[serde(default)]
    pub tool_eager_override: bool,
}

fn default_builtin_tool_deferred_loading() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GlobalDirective {

    pub content: String,

    #[serde(default)]
    pub mode: Option<String>,
}

impl Default for GlobalDirective {
    fn default() -> Self {
        Self {
            content: String::new(),
            mode: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AutoIndexConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_auto_index_include_patterns")]
    pub include_patterns: Vec<String>,

    #[serde(default = "default_auto_index_exclude_patterns")]
    pub exclude_patterns: Vec<String>,

    #[serde(default = "default_auto_index_max_files")]
    pub max_files: usize,

    #[serde(default = "default_auto_index_refresh")]
    pub refresh_interval_secs: u64,
}

impl Default for AutoIndexConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            include_patterns: default_auto_index_include_patterns(),
            exclude_patterns: default_auto_index_exclude_patterns(),
            max_files: default_auto_index_max_files(),
            refresh_interval_secs: default_auto_index_refresh(),
        }
    }
}

fn default_auto_index_include_patterns() -> Vec<String> {
    vec![
        "**/*.rs".to_string(),
        "**/*.ts".to_string(),
        "**/*.tsx".to_string(),
        "**/*.js".to_string(),
        "**/*.jsx".to_string(),
        "**/*.py".to_string(),
        "**/*.go".to_string(),
    ]
}

fn default_auto_index_exclude_patterns() -> Vec<String> {
    vec![
        "**/node_modules/**".to_string(),
        "**/target/**".to_string(),
        "**/.git/**".to_string(),
        "**/dist/**".to_string(),
        "**/build/**".to_string(),
    ]
}

fn default_auto_index_max_files() -> usize {
    10_000
}

fn default_auto_index_refresh() -> u64 {
    3600
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CodeRagConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_code_rag_top_k")]
    pub top_k: usize,

    // On by default so semantic code retrieval works out of the box whenever an
    // embedder is reachable (local Ollama, or the openai backend inheriting the
    // configured provider key). When no embedder is reachable the vector seed
    // simply produces nothing and retrieval degrades gracefully to the lexical
    // index — never an error.
    #[serde(default = "default_true")]
    pub dense_enabled: bool,

    #[serde(default)]
    pub embedder: CodeRagEmbedderConfig,
}

impl Default for CodeRagConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            top_k: default_code_rag_top_k(),
            dense_enabled: true,
            embedder: CodeRagEmbedderConfig::default(),
        }
    }
}

fn default_code_rag_top_k() -> usize {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CodeRagEmbedderConfig {
    #[serde(default = "default_code_rag_embedder_backend")]
    pub backend: String,

    #[serde(default = "default_code_rag_embedder_model")]
    pub model: String,

    #[serde(default)]
    pub endpoint: Option<String>,

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_code_rag_embedder_dims")]
    pub dims: usize,
}

impl Default for CodeRagEmbedderConfig {
    fn default() -> Self {
        Self {
            backend: default_code_rag_embedder_backend(),
            model: default_code_rag_embedder_model(),
            endpoint: None,
            api_key: None,
            dims: default_code_rag_embedder_dims(),
        }
    }
}

fn default_code_rag_embedder_backend() -> String {
    "ollama".to_string()
}

fn default_code_rag_embedder_model() -> String {
    "nomic-embed-text".to_string()
}

fn default_code_rag_embedder_dims() -> usize {
    768
}

pub fn default_agent_max_tool_iterations() -> usize {

    2000
}

fn default_agent_max_history_messages() -> usize {
    50
}

fn default_agent_recent_turn_window() -> usize {
    3
}

fn default_agent_recent_window_max_turns() -> usize {
    12
}

fn default_agent_recent_window_token_ratio() -> f64 {
    0.25
}

fn default_agent_recent_window_summary_batch_turns() -> usize {
    4
}

fn default_agent_max_context_tokens() -> usize {
    32_000
}

fn default_agent_tool_dispatcher() -> String {
    "auto".into()
}

fn default_max_system_prompt_chars() -> usize {
    16_000
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            compact_context: true,
            max_tool_iterations: default_agent_max_tool_iterations(),
            max_history_messages: default_agent_max_history_messages(),
            recent_turn_window: default_agent_recent_turn_window(),
            recent_window_max_turns: default_agent_recent_window_max_turns(),
            recent_window_token_ratio: default_agent_recent_window_token_ratio(),
            recent_window_summary_enabled: true,
            recent_window_summary_batch_turns: default_agent_recent_window_summary_batch_turns(),
            max_context_tokens: default_agent_max_context_tokens(),
            parallel_tools: false,
            tool_dispatcher: default_agent_tool_dispatcher(),
            tool_call_dedup_exempt: Vec::new(),
            tool_filter_groups: Vec::new(),
            max_system_prompt_chars: default_max_system_prompt_chars(),
            thinking: crate::agent::thinking::ThinkingConfig::default(),
            history_pruning: crate::agent::history::pruner::HistoryPrunerConfig::default(),
            context_aware_tools: false,
            eval: crate::agent::eval::EvalConfig::default(),
            auto_classify: None,
            context_compression:
                crate::agent::context::compressor::ContextCompressionConfig::default(),
            global_directives: Vec::new(),
            project_config_dir: None,
            auto_index: AutoIndexConfig::default(),
            builtin_tool_deferred_loading: default_builtin_tool_deferred_loading(),
            tool_eager_override: false,
        }
    }
}

pub use crate::config::domain::pacing::PacingConfig;

pub use crate::config::domain::AgentRuntimeExtras;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkillsPromptInjectionMode {
    Full,

    #[default]
    Compact,
}

fn parse_skills_prompt_injection_mode(raw: &str) -> Option<SkillsPromptInjectionMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "full" => Some(SkillsPromptInjectionMode::Full),
        "compact" => Some(SkillsPromptInjectionMode::Compact),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct SkillsConfig {

    #[serde(default)]
    pub open_skills_enabled: bool,

    #[serde(default)]
    pub open_skills_dir: Option<String>,

    #[serde(default)]
    pub allow_scripts: bool,

    #[serde(default)]
    pub prompt_injection_mode: SkillsPromptInjectionMode,

    #[serde(default)]
    pub skill_creation: SkillCreationConfig,

    #[serde(default)]
    pub skill_improvement: SkillImprovementConfig,

    #[serde(default)]
    pub disabled_skills: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SkillCreationConfig {

    pub enabled: bool,

    pub max_skills: usize,

    pub similarity_threshold: f64,
}

impl Default for SkillCreationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_skills: 500,
            similarity_threshold: 0.85,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SkillImprovementConfig {

    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_skill_improvement_cooldown")]
    pub cooldown_secs: u64,
}

fn default_skill_improvement_cooldown() -> u64 {
    3600
}

impl Default for SkillImprovementConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cooldown_secs: 3600,
        }
    }
}

pub use crate::config::domain::pipeline::PipelineConfig;

pub use crate::config::domain::multimodal::MultimodalConfig;

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MediaPipelineConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_true")]
    pub transcribe_audio: bool,

    #[serde(default = "default_true")]
    pub describe_images: bool,

    #[serde(default = "default_true")]
    pub summarize_video: bool,
}

impl Default for MediaPipelineConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            transcribe_audio: true,
            describe_images: true,
            summarize_video: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IdentityConfig {

    #[serde(default = "default_identity_format")]
    pub format: String,

    #[serde(default)]
    pub aieos_path: Option<String>,

    #[serde(default)]
    pub aieos_inline: Option<String>,
}

fn default_identity_format() -> String {
    "markdown".into()
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            format: default_identity_format(),
            aieos_path: None,
            aieos_inline: None,
        }
    }
}

pub mod model_pricing;

pub use model_pricing::{CostConfig, CostEnforcementConfig, ModelPricing};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[allow(clippy::struct_excessive_bools)]
pub struct GatewayConfig {

    #[serde(default = "default_gateway_port")]
    pub port: u16,

    #[serde(default = "default_gateway_host")]
    pub host: String,

    #[serde(default = "default_true")]
    pub require_pairing: bool,

    #[serde(default)]
    pub allow_public_bind: bool,

    #[serde(default)]
    pub isolated: bool,

    #[serde(default, serialize_with = "crate::config::redact::redact_vec_string")]
    pub paired_tokens: Vec<String>,

    #[serde(default = "default_pair_rate_limit")]
    pub pair_rate_limit_per_minute: u32,

    #[serde(default = "default_webhook_rate_limit")]
    pub webhook_rate_limit_per_minute: u32,

    #[serde(default)]
    pub trust_forwarded_headers: bool,

    #[serde(default)]
    pub path_prefix: Option<String>,

    #[serde(default = "default_gateway_rate_limit_max_keys")]
    pub rate_limit_max_keys: usize,

    #[serde(default = "default_idempotency_ttl_secs")]
    pub idempotency_ttl_secs: u64,

    #[serde(default = "default_gateway_idempotency_max_keys")]
    pub idempotency_max_keys: usize,

    #[serde(default = "default_true")]
    pub session_persistence: bool,

    #[serde(default)]
    pub session_ttl_hours: u32,

    #[serde(default)]
    pub pairing_dashboard: PairingDashboardConfig,

    #[serde(default)]
    pub tls: Option<GatewayTlsConfig>,

    #[serde(
        default,
        serialize_with = "crate::config::redact::redact_optional_string"
    )]
    pub signing_secret: Option<String>,
}

pub use crate::config::domain::rpc::{RpcConfig, RpcHttpConfig};

fn default_gateway_port() -> u16 {
    42617
}

fn default_gateway_host() -> String {
    "127.0.0.1".into()
}

fn default_pair_rate_limit() -> u32 {
    10
}

fn default_webhook_rate_limit() -> u32 {
    60
}

fn default_idempotency_ttl_secs() -> u64 {
    300
}

fn default_gateway_rate_limit_max_keys() -> usize {
    10_000
}

fn default_gateway_idempotency_max_keys() -> usize {
    10_000
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            port: default_gateway_port(),
            host: default_gateway_host(),
            require_pairing: true,
            allow_public_bind: false,
            isolated: false,
            paired_tokens: Vec::new(),
            pair_rate_limit_per_minute: default_pair_rate_limit(),
            webhook_rate_limit_per_minute: default_webhook_rate_limit(),
            trust_forwarded_headers: false,
            path_prefix: None,
            rate_limit_max_keys: default_gateway_rate_limit_max_keys(),
            idempotency_ttl_secs: default_idempotency_ttl_secs(),
            idempotency_max_keys: default_gateway_idempotency_max_keys(),
            session_persistence: true,
            session_ttl_hours: 0,
            pairing_dashboard: PairingDashboardConfig::default(),
            tls: None,
            signing_secret: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PairingDashboardConfig {

    #[serde(default = "default_pairing_code_length")]
    pub code_length: usize,

    #[serde(default = "default_pairing_ttl")]
    pub code_ttl_secs: u64,

    #[serde(default = "default_max_pending_codes")]
    pub max_pending_codes: usize,

    #[serde(default = "default_max_failed_attempts")]
    pub max_failed_attempts: u32,

    #[serde(default = "default_pairing_lockout_secs")]
    pub lockout_secs: u64,
}

fn default_pairing_code_length() -> usize {
    8
}
fn default_pairing_ttl() -> u64 {
    3600
}
fn default_max_pending_codes() -> usize {
    3
}
fn default_max_failed_attempts() -> u32 {
    5
}
fn default_pairing_lockout_secs() -> u64 {
    300
}

impl Default for PairingDashboardConfig {
    fn default() -> Self {
        Self {
            code_length: default_pairing_code_length(),
            code_ttl_secs: default_pairing_ttl(),
            max_pending_codes: default_max_pending_codes(),
            max_failed_attempts: default_max_failed_attempts(),
            lockout_secs: default_pairing_lockout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GatewayTlsConfig {

    #[serde(default)]
    pub enabled: bool,

    pub cert_path: String,

    pub key_path: String,

    #[serde(default)]
    pub client_auth: Option<GatewayClientAuthConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GatewayClientAuthConfig {

    #[serde(default)]
    pub enabled: bool,

    pub ca_cert_path: String,

    #[serde(default = "default_true")]
    pub require_client_cert: bool,

    #[serde(default)]
    pub pinned_certs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ComposioConfig {

    #[serde(default, alias = "enable")]
    pub enabled: bool,

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_entity_id")]
    pub entity_id: String,
}

fn default_entity_id() -> String {
    "default".into()
}

impl Default for ComposioConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: None,
            entity_id: default_entity_id(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
pub struct Microsoft365Config {

    #[serde(default, alias = "enable")]
    pub enabled: bool,

    #[serde(default)]
    pub tenant_id: Option<String>,

    #[serde(default)]
    pub client_id: Option<String>,

    #[serde(default)]
    pub client_secret: Option<String>,

    #[serde(default = "default_ms365_auth_flow")]
    pub auth_flow: String,

    #[serde(default = "default_ms365_scopes")]
    pub scopes: Vec<String>,

    #[serde(default = "default_true")]
    pub token_cache_encrypted: bool,

    #[serde(default)]
    pub user_id: Option<String>,
}

fn default_ms365_auth_flow() -> String {
    "client_credentials".to_string()
}

fn default_ms365_scopes() -> Vec<String> {
    vec!["https://graph.microsoft.com/.default".to_string()]
}

impl std::fmt::Debug for Microsoft365Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Microsoft365Config")
            .field("enabled", &self.enabled)
            .field("tenant_id", &self.tenant_id)
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret.as_ref().map(|_| "***"))
            .field("auth_flow", &self.auth_flow)
            .field("scopes", &self.scopes)
            .field("token_cache_encrypted", &self.token_cache_encrypted)
            .field("user_id", &self.user_id)
            .finish()
    }
}

impl Default for Microsoft365Config {
    fn default() -> Self {
        Self {
            enabled: false,
            tenant_id: None,
            client_id: None,
            client_secret: None,
            auth_flow: default_ms365_auth_flow(),
            scopes: default_ms365_scopes(),
            token_cache_encrypted: true,
            user_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SecretsConfig {

    #[serde(default = "default_true")]
    pub encrypt: bool,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self { encrypt: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BrowserConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub allowed_domains: Vec<String>,

    #[serde(default)]
    pub session_name: Option<String>,

    #[serde(default = "default_browser_backend")]
    pub backend: String,

    #[serde(default = "default_true")]
    pub native_headless: bool,

    #[serde(default = "default_browser_webdriver_url")]
    pub native_webdriver_url: String,

    #[serde(default)]
    pub native_chrome_path: Option<String>,
}

fn default_browser_backend() -> String {
    "agent_browser".into()
}

fn default_browser_webdriver_url() -> String {
    "http://127.0.0.1:9515".into()
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_domains: vec!["*".into()],
            session_name: None,
            backend: default_browser_backend(),
            native_headless: default_true(),
            native_webdriver_url: default_browser_webdriver_url(),
            native_chrome_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HttpRequestConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub allowed_domains: Vec<String>,

    #[serde(default = "default_http_max_response_size")]
    pub max_response_size: usize,

    #[serde(default = "default_http_timeout_secs")]
    pub timeout_secs: u64,

    #[serde(default)]
    pub allow_private_hosts: bool,
}

impl Default for HttpRequestConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_domains: vec!["*".into()],
            max_response_size: default_http_max_response_size(),
            timeout_secs: default_http_timeout_secs(),
            allow_private_hosts: false,
        }
    }
}

fn default_http_max_response_size() -> usize {
    1_000_000
}

fn default_http_timeout_secs() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WebFetchConfig {

    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_web_fetch_allowed_domains")]
    pub allowed_domains: Vec<String>,

    #[serde(default)]
    pub blocked_domains: Vec<String>,

    #[serde(default)]
    pub allowed_private_hosts: Vec<String>,

    #[serde(default = "default_web_fetch_max_response_size")]
    pub max_response_size: usize,

    #[serde(default = "default_web_fetch_timeout_secs")]
    pub timeout_secs: u64,

    #[serde(default)]
    pub firecrawl: FirecrawlConfig,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FirecrawlMode {
    #[default]
    Scrape,

    Crawl,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FirecrawlConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_firecrawl_api_key_env")]
    pub api_key_env: String,

    #[serde(default = "default_firecrawl_api_url")]
    pub api_url: String,

    #[serde(default)]
    pub mode: FirecrawlMode,
}

fn default_firecrawl_api_key_env() -> String {
    "FIRECRAWL_API_KEY".into()
}

fn default_firecrawl_api_url() -> String {
    "https://api.firecrawl.dev/v1".into()
}

impl Default for FirecrawlConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key_env: default_firecrawl_api_key_env(),
            api_url: default_firecrawl_api_url(),
            mode: FirecrawlMode::default(),
        }
    }
}

fn default_web_fetch_max_response_size() -> usize {
    500_000
}

fn default_web_fetch_timeout_secs() -> u64 {
    30
}

fn default_web_fetch_allowed_domains() -> Vec<String> {
    vec!["*".into()]
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_domains: vec!["*".into()],
            blocked_domains: vec![],
            allowed_private_hosts: vec![],
            max_response_size: default_web_fetch_max_response_size(),
            timeout_secs: default_web_fetch_timeout_secs(),
            firecrawl: FirecrawlConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LinkEnricherConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_link_enricher_max_links")]
    pub max_links: usize,

    #[serde(default = "default_link_enricher_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_link_enricher_max_links() -> usize {
    3
}

fn default_link_enricher_timeout_secs() -> u64 {
    10
}

impl Default for LinkEnricherConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_links: default_link_enricher_max_links(),
            timeout_secs: default_link_enricher_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TextBrowserConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub preferred_browser: Option<String>,

    #[serde(default = "default_text_browser_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_text_browser_timeout_secs() -> u64 {
    30
}

impl Default for TextBrowserConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            preferred_browser: None,
            timeout_secs: default_text_browser_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ShellToolConfig {

    #[serde(default = "default_shell_tool_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_shell_tool_timeout_secs() -> u64 {
    60
}

impl Default for ShellToolConfig {
    fn default() -> Self {
        Self {
            timeout_secs: default_shell_tool_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WebSearchConfig {

    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_web_search_provider")]
    pub provider: String,

    #[serde(default)]
    pub brave_api_key: Option<String>,

    #[serde(default)]
    pub searxng_instance_url: Option<String>,

    #[serde(default)]
    pub tavily_api_key: Option<String>,

    #[serde(default)]
    pub exa_api_key: Option<String>,

    #[serde(default = "default_web_search_max_results")]
    pub max_results: usize,

    #[serde(default = "default_web_search_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_web_search_provider() -> String {
    "duckduckgo".into()
}

fn default_web_search_max_results() -> usize {
    5
}

fn default_web_search_timeout_secs() -> u64 {
    15
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: default_web_search_provider(),
            brave_api_key: None,
            searxng_instance_url: None,
            tavily_api_key: None,
            exa_api_key: None,
            max_results: default_web_search_max_results(),
            timeout_secs: default_web_search_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectIntelConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_project_intel_language")]
    pub default_language: String,

    #[serde(default = "default_project_intel_report_dir")]
    pub report_output_dir: String,

    #[serde(default)]
    pub templates_dir: Option<String>,

    #[serde(default = "default_project_intel_risk_sensitivity")]
    pub risk_sensitivity: String,

    #[serde(default = "default_true")]
    pub include_git_data: bool,

    #[serde(default)]
    pub include_jira_data: bool,

    #[serde(default)]
    pub jira_base_url: Option<String>,
}

fn default_project_intel_language() -> String {
    "en".into()
}

fn default_project_intel_report_dir() -> String {
    "~/.senweavercoding/project-reports".into()
}

fn default_project_intel_risk_sensitivity() -> String {
    "medium".into()
}

impl Default for ProjectIntelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_language: default_project_intel_language(),
            report_output_dir: default_project_intel_report_dir(),
            templates_dir: None,
            risk_sensitivity: default_project_intel_risk_sensitivity(),
            include_git_data: true,
            include_jira_data: false,
            jira_base_url: None,
        }
    }
}

pub use crate::config::domain::backup::{BackupConfig, DataRetentionConfig};

pub const DEFAULT_GWS_SERVICES: &[&str] = &[
    "drive",
    "sheets",
    "gmail",
    "calendar",
    "docs",
    "slides",
    "tasks",
    "people",
    "chat",
    "classroom",
    "forms",
    "keep",
    "meet",
    "events",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoogleWorkspaceAllowedOperation {

    pub service: String,

    pub resource: String,

    #[serde(default)]
    pub sub_resource: Option<String>,

    #[serde(default)]
    pub methods: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoogleWorkspaceConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub allowed_services: Vec<String>,

    #[serde(default)]
    pub allowed_operations: Vec<GoogleWorkspaceAllowedOperation>,

    #[serde(default)]
    pub credentials_path: Option<String>,

    #[serde(default)]
    pub default_account: Option<String>,

    #[serde(default = "default_gws_rate_limit")]
    pub rate_limit_per_minute: u32,

    #[serde(default = "default_gws_timeout_secs")]
    pub timeout_secs: u64,

    #[serde(default)]
    pub audit_log: bool,
}

fn default_gws_rate_limit() -> u32 {
    60
}

fn default_gws_timeout_secs() -> u64 {
    30
}

impl Default for GoogleWorkspaceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_services: Vec::new(),
            allowed_operations: Vec::new(),
            credentials_path: None,
            default_account: None,
            rate_limit_per_minute: default_gws_rate_limit(),
            timeout_secs: default_gws_timeout_secs(),
            audit_log: false,
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KnowledgeConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_knowledge_db_path")]
    pub db_path: String,

    #[serde(default = "default_knowledge_max_nodes")]
    pub max_nodes: usize,

    #[serde(default)]
    pub auto_capture: bool,

    #[serde(default = "default_true")]
    pub suggest_on_query: bool,

    #[serde(default)]
    pub cross_workspace_search: bool,
}

fn default_knowledge_db_path() -> String {
    "~/.senweavercoding/knowledge.db".into()
}

fn default_knowledge_max_nodes() -> usize {
    100_000
}

impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            db_path: default_knowledge_db_path(),
            max_nodes: default_knowledge_max_nodes(),
            auto_capture: false,
            suggest_on_query: true,
            cross_workspace_search: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LinkedInConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_linkedin_api_version")]
    pub api_version: String,

    #[serde(default)]
    pub content: LinkedInContentConfig,

    #[serde(default)]
    pub image: LinkedInImageConfig,
}

impl Default for LinkedInConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_version: default_linkedin_api_version(),
            content: LinkedInContentConfig::default(),
            image: LinkedInImageConfig::default(),
        }
    }
}

fn default_linkedin_api_version() -> String {
    "202602".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PluginsConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_plugins_dir")]
    pub plugins_dir: String,

    #[serde(default = "default_plugins_auto_discover")]
    pub auto_discover: bool,

    #[serde(default = "default_max_plugins")]
    pub max_plugins: usize,

    #[serde(default)]
    pub security: PluginSecurityConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PluginSecurityConfig {

    #[serde(default = "default_signature_mode")]
    pub signature_mode: String,

    #[serde(default)]
    pub trusted_publisher_keys: Vec<String>,
}

fn default_signature_mode() -> String {
    "disabled".to_string()
}

impl Default for PluginSecurityConfig {
    fn default() -> Self {
        Self {
            signature_mode: default_signature_mode(),
            trusted_publisher_keys: Vec::new(),
        }
    }
}

fn default_plugins_dir() -> String {
    "~/.senweavercoding/plugins".to_string()
}

fn default_max_plugins() -> usize {
    50
}

fn default_plugins_auto_discover() -> bool {
    true
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            plugins_dir: default_plugins_dir(),
            auto_discover: default_plugins_auto_discover(),
            max_plugins: default_max_plugins(),
            security: PluginSecurityConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct LinkedInContentConfig {

    #[serde(default)]
    pub rss_feeds: Vec<String>,

    #[serde(default)]
    pub github_users: Vec<String>,

    #[serde(default)]
    pub github_repos: Vec<String>,

    #[serde(default)]
    pub topics: Vec<String>,

    #[serde(default)]
    pub persona: String,

    #[serde(default)]
    pub instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LinkedInImageConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_image_providers")]
    pub providers: Vec<String>,

    #[serde(default = "default_true")]
    pub fallback_card: bool,

    #[serde(default = "default_card_accent_color")]
    pub card_accent_color: String,

    #[serde(default = "default_image_temp_dir")]
    pub temp_dir: String,

    #[serde(default)]
    pub stability: ImageProviderStabilityConfig,

    #[serde(default)]
    pub imagen: ImageProviderImagenConfig,

    #[serde(default)]
    pub dalle: ImageProviderDalleConfig,

    #[serde(default)]
    pub flux: ImageProviderFluxConfig,
}

fn default_image_providers() -> Vec<String> {
    vec![
        "stability".into(),
        "imagen".into(),
        "dalle".into(),
        "flux".into(),
    ]
}

fn default_card_accent_color() -> String {
    "#0A66C2".into()
}

fn default_image_temp_dir() -> String {
    "linkedin/images".into()
}

impl Default for LinkedInImageConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            providers: default_image_providers(),
            fallback_card: true,
            card_accent_color: default_card_accent_color(),
            temp_dir: default_image_temp_dir(),
            stability: ImageProviderStabilityConfig::default(),
            imagen: ImageProviderImagenConfig::default(),
            dalle: ImageProviderDalleConfig::default(),
            flux: ImageProviderFluxConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImageProviderStabilityConfig {

    #[serde(default = "default_stability_api_key_env")]
    pub api_key_env: String,

    #[serde(default = "default_stability_model")]
    pub model: String,
}

fn default_stability_api_key_env() -> String {
    "STABILITY_API_KEY".into()
}
fn default_stability_model() -> String {
    "stable-diffusion-xl-1024-v1-0".into()
}

impl Default for ImageProviderStabilityConfig {
    fn default() -> Self {
        Self {
            api_key_env: default_stability_api_key_env(),
            model: default_stability_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImageProviderImagenConfig {

    #[serde(default = "default_imagen_api_key_env")]
    pub api_key_env: String,

    #[serde(default = "default_imagen_project_id_env")]
    pub project_id_env: String,

    #[serde(default = "default_imagen_region")]
    pub region: String,
}

fn default_imagen_api_key_env() -> String {
    "GOOGLE_VERTEX_API_KEY".into()
}
fn default_imagen_project_id_env() -> String {
    "GOOGLE_CLOUD_PROJECT".into()
}
fn default_imagen_region() -> String {
    "us-central1".into()
}

impl Default for ImageProviderImagenConfig {
    fn default() -> Self {
        Self {
            api_key_env: default_imagen_api_key_env(),
            project_id_env: default_imagen_project_id_env(),
            region: default_imagen_region(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImageProviderDalleConfig {

    #[serde(default = "default_dalle_api_key_env")]
    pub api_key_env: String,

    #[serde(default = "default_dalle_model")]
    pub model: String,

    #[serde(default = "default_dalle_size")]
    pub size: String,
}

fn default_dalle_api_key_env() -> String {
    "OPENAI_API_KEY".into()
}
fn default_dalle_model() -> String {
    "dall-e-3".into()
}
fn default_dalle_size() -> String {
    "1024x1024".into()
}

impl Default for ImageProviderDalleConfig {
    fn default() -> Self {
        Self {
            api_key_env: default_dalle_api_key_env(),
            model: default_dalle_model(),
            size: default_dalle_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImageProviderFluxConfig {

    #[serde(default = "default_flux_api_key_env")]
    pub api_key_env: String,

    #[serde(default = "default_flux_model")]
    pub model: String,
}

fn default_flux_api_key_env() -> String {
    "FAL_API_KEY".into()
}
fn default_flux_model() -> String {
    "fal-ai/flux/schnell".into()
}

impl Default for ImageProviderFluxConfig {
    fn default() -> Self {
        Self {
            api_key_env: default_flux_api_key_env(),
            model: default_flux_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImageGenConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_image_gen_model")]
    pub default_model: String,

    #[serde(default = "default_image_gen_api_key_env")]
    pub api_key_env: String,
}

fn default_image_gen_model() -> String {
    "fal-ai/flux/schnell".into()
}

fn default_image_gen_api_key_env() -> String {
    "FAL_API_KEY".into()
}

impl Default for ImageGenConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_model: default_image_gen_model(),
            api_key_env: default_image_gen_api_key_env(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClaudeCodeConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_claude_code_timeout_secs")]
    pub timeout_secs: u64,

    #[serde(default = "default_claude_code_allowed_tools")]
    pub allowed_tools: Vec<String>,

    #[serde(default)]
    pub system_prompt: Option<String>,

    #[serde(default = "default_claude_code_max_output_bytes")]
    pub max_output_bytes: usize,

    #[serde(default)]
    pub env_passthrough: Vec<String>,
}

fn default_claude_code_timeout_secs() -> u64 {
    600
}

fn default_claude_code_allowed_tools() -> Vec<String> {
    vec!["Read".into(), "Edit".into(), "Bash".into(), "Write".into()]
}

fn default_claude_code_max_output_bytes() -> usize {
    2_097_152
}

impl Default for ClaudeCodeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_secs: default_claude_code_timeout_secs(),
            allowed_tools: default_claude_code_allowed_tools(),
            system_prompt: None,
            max_output_bytes: default_claude_code_max_output_bytes(),
            env_passthrough: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClaudeCodeRunnerConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub ssh_host: Option<String>,

    #[serde(default = "default_claude_code_runner_tmux_prefix")]
    pub tmux_prefix: String,

    #[serde(default = "default_claude_code_runner_session_ttl")]
    pub session_ttl: u64,
}

fn default_claude_code_runner_tmux_prefix() -> String {
    "zc-claude-".into()
}

fn default_claude_code_runner_session_ttl() -> u64 {
    3600
}

impl Default for ClaudeCodeRunnerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ssh_host: None,
            tmux_prefix: default_claude_code_runner_tmux_prefix(),
            session_ttl: default_claude_code_runner_session_ttl(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CodexCliConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_codex_cli_timeout_secs")]
    pub timeout_secs: u64,

    #[serde(default = "default_codex_cli_max_output_bytes")]
    pub max_output_bytes: usize,

    #[serde(default)]
    pub env_passthrough: Vec<String>,
}

fn default_codex_cli_timeout_secs() -> u64 {
    600
}

fn default_codex_cli_max_output_bytes() -> usize {
    2_097_152
}

impl Default for CodexCliConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_secs: default_codex_cli_timeout_secs(),
            max_output_bytes: default_codex_cli_max_output_bytes(),
            env_passthrough: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GeminiCliConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_gemini_cli_timeout_secs")]
    pub timeout_secs: u64,

    #[serde(default = "default_gemini_cli_max_output_bytes")]
    pub max_output_bytes: usize,

    #[serde(default)]
    pub env_passthrough: Vec<String>,
}

fn default_gemini_cli_timeout_secs() -> u64 {
    600
}

fn default_gemini_cli_max_output_bytes() -> usize {
    2_097_152
}

impl Default for GeminiCliConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_secs: default_gemini_cli_timeout_secs(),
            max_output_bytes: default_gemini_cli_max_output_bytes(),
            env_passthrough: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OpenCodeCliConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_opencode_cli_timeout_secs")]
    pub timeout_secs: u64,

    #[serde(default = "default_opencode_cli_max_output_bytes")]
    pub max_output_bytes: usize,

    #[serde(default)]
    pub env_passthrough: Vec<String>,
}

fn default_opencode_cli_timeout_secs() -> u64 {
    600
}

fn default_opencode_cli_max_output_bytes() -> usize {
    2_097_152
}

impl Default for OpenCodeCliConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_secs: default_opencode_cli_timeout_secs(),
            max_output_bytes: default_opencode_cli_max_output_bytes(),
            env_passthrough: Vec::new(),
        }
    }
}

pub use crate::config::domain::observability::ObservabilityConfig;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HooksConfig {

    pub enabled: bool,

    #[serde(default)]
    pub allow_workspace_hooks: bool,

    #[serde(default)]
    pub builtin: BuiltinHooksConfig,
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_workspace_hooks: false,
            builtin: BuiltinHooksConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct BuiltinHooksConfig {

    pub command_logger: bool,

    #[serde(default)]
    pub webhook_audit: WebhookAuditConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WebhookAuditConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub url: String,

    #[serde(default)]
    pub tool_patterns: Vec<String>,

    #[serde(default)]
    pub include_args: bool,

    #[serde(default = "default_max_args_bytes")]
    pub max_args_bytes: u64,
}

fn default_max_args_bytes() -> u64 {
    4096
}

impl Default for WebhookAuditConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            tool_patterns: Vec::new(),
            include_args: false,
            max_args_bytes: default_max_args_bytes(),
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AutonomyConfig {

    pub level: AutonomyLevel,

    pub workspace_only: bool,

    pub allowed_commands: Vec<String>,

    pub forbidden_paths: Vec<String>,

    pub max_actions_per_hour: u32,

    pub max_cost_per_day_cents: u32,

    #[serde(default = "default_true")]
    pub require_approval_for_medium_risk: bool,

    #[serde(default = "default_true")]
    pub block_high_risk_commands: bool,

    #[serde(default)]
    pub shell_env_passthrough: Vec<String>,

    #[serde(default = "default_auto_approve")]
    pub auto_approve: Vec<String>,

    #[serde(default = "default_always_ask")]
    pub always_ask: Vec<String>,

    #[serde(default)]
    pub allowed_roots: Vec<String>,

    #[serde(default)]
    pub non_cli_excluded_tools: Vec<String>,

    #[serde(default = "default_true")]
    pub protect_browser_tools: bool,

    #[serde(default = "default_true")]
    pub protect_mcp_tools: bool,

    #[serde(default)]
    pub auto_approve_mode_transitions: Vec<String>,

    #[serde(default = "default_true")]
    pub enable_command_policy: bool,

    // Default false: on channel/daemon/one-shot surfaces there is no human to
    // approve, so auto-running shell (incl. medium-risk) there is unsafe,
    // especially stacked on top of untrusted content ingestion. Opt in
    // explicitly for trusted automation surfaces.
    #[serde(default)]
    pub allow_shell_in_non_interactive: bool,
}

fn default_auto_approve() -> Vec<String> {
    vec![
        "file_read".into(),
        "memory_recall".into(),
        "web_search_tool".into(),
        // web_fetch is intentionally NOT auto-approved: it ingests arbitrary
        // remote page text that can carry prompt-injection payloads which then
        // drive tool calls. Require an explicit approval (or an opt-in Always)
        // before fetching untrusted URLs.
        "calculator".into(),
        "glob_search".into(),
        "content_search".into(),
        "image_info".into(),
        "weather".into(),

        "todo_write".into(),
    ]
}

fn default_always_ask() -> Vec<String> {
    vec![]
}

impl AutonomyConfig {

    pub fn ensure_default_auto_approve(&mut self) {
        let defaults = default_auto_approve();
        for entry in defaults {
            if !self.auto_approve.iter().any(|existing| existing == &entry) {
                self.auto_approve.push(entry);
            }
        }
    }
}

fn is_valid_env_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

impl Default for AutonomyConfig {
    fn default() -> Self {
        Self {
            level: AutonomyLevel::Supervised,
            workspace_only: true,
            allowed_commands: vec![
                "git".into(),
                "npm".into(),
                "cargo".into(),
                "ls".into(),
                "cat".into(),
                "grep".into(),
                "find".into(),
                "echo".into(),
                "pwd".into(),
                "wc".into(),
                "head".into(),
                "tail".into(),
                "date".into(),
                "python".into(),
                "python3".into(),
                "pip".into(),
                "node".into(),
            ],
            forbidden_paths: vec![
                "/etc".into(),
                "/root".into(),
                "/home".into(),
                "/usr".into(),
                "/bin".into(),
                "/sbin".into(),
                "/lib".into(),
                "/opt".into(),
                "/boot".into(),
                "/dev".into(),
                "/proc".into(),
                "/sys".into(),
                "/var".into(),
                "/tmp".into(),
                "~/.ssh".into(),
                "~/.gnupg".into(),
                "~/.aws".into(),
                "~/.config".into(),
            ],
            max_actions_per_hour: 0,
            max_cost_per_day_cents: 500,
            require_approval_for_medium_risk: true,
            block_high_risk_commands: true,
            shell_env_passthrough: vec![],
            auto_approve: default_auto_approve(),
            always_ask: default_always_ask(),
            allowed_roots: Vec::new(),
            non_cli_excluded_tools: Vec::new(),
            protect_browser_tools: true,
            protect_mcp_tools: true,
            auto_approve_mode_transitions: Vec::new(),
            enable_command_policy: true,
            allow_shell_in_non_interactive: false,
        }
    }
}

pub use crate::config::domain::runtime::{DockerRuntimeConfig, RuntimeConfig, WasmRuntimeConfig};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelRouteConfig {

    pub hint: String,

    pub provider: String,

    pub model: String,

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SavedModel {

    pub id: String,

    pub name: String,

    pub provider: String,

    pub api_key: Option<String>,

    #[serde(default)]
    pub base_url: Option<String>,

    pub model: String,

    #[serde(default = "default_temperature")]
    pub temperature: f64,

    #[serde(default = "default_provider_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for SavedModel {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            provider: "openrouter".to_string(),
            api_key: None,
            base_url: None,
            model: String::new(),
            temperature: default_temperature(),
            timeout_secs: default_provider_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EmbeddingRouteConfig {

    pub hint: String,

    pub provider: String,

    pub model: String,

    #[serde(default)]
    pub dimensions: Option<usize>,

    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct QueryClassificationConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub rules: Vec<ClassificationRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct ClassificationRule {

    pub hint: String,

    #[serde(default)]
    pub keywords: Vec<String>,

    #[serde(default)]
    pub patterns: Vec<String>,

    #[serde(default)]
    pub min_length: Option<usize>,

    #[serde(default)]
    pub max_length: Option<usize>,

    #[serde(default)]
    pub priority: i32,
}

pub use crate::config::domain::heartbeat::HeartbeatConfig;
pub use crate::config::domain::lan::LanConfig;

mod tunnel;

pub use tunnel::{
    CloudflareTunnelConfig, CustomTunnelConfig, NgrokTunnelConfig, OpenVpnTunnelConfig,
    PinggyTunnelConfig, TailscaleTunnelConfig, TunnelConfig,
};

mod sop;
pub use sop::{MqttConfig, SopConfig};

mod lsp;
pub use lsp::{LspConfig, LspInstallState, LspServerEntry};

mod storage;
pub use storage::{StorageConfig, StorageProviderConfig, StorageProviderSection};

mod hardware;
pub use hardware::{PeripheralBoardConfig, PeripheralsConfig};

mod runtime;
pub use runtime::{ReliabilityConfig, SchedulerConfig};

mod cron;
pub use cron::{CronConfig, CronJobDecl, CronScheduleDecl, DeliveryConfigDecl};

struct ConfigWrapper<T: ChannelConfig>(std::marker::PhantomData<T>);

impl<T: ChannelConfig> ConfigWrapper<T> {
    fn new(_: Option<&T>) -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<T: ChannelConfig> crate::config::traits::ConfigHandle for ConfigWrapper<T> {
    fn name(&self) -> &'static str {
        T::name()
    }
    fn desc(&self) -> &'static str {
        T::desc()
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChannelsConfig {

    #[serde(default = "default_true")]
    pub cli: bool,

    pub telegram: Option<TelegramConfig>,

    pub discord: Option<DiscordConfig>,

    pub discord_history: Option<DiscordHistoryConfig>,

    pub slack: Option<SlackConfig>,

    pub mattermost: Option<MattermostConfig>,

    pub webhook: Option<WebhookConfig>,

    pub imessage: Option<IMessageConfig>,

    pub matrix: Option<MatrixConfig>,

    pub signal: Option<SignalConfig>,

    pub whatsapp: Option<WhatsAppConfig>,

    pub linq: Option<LinqConfig>,

    pub wati: Option<WatiConfig>,

    pub nextcloud_talk: Option<NextcloudTalkConfig>,

    pub email: Option<crate::channels::email_channel::EmailConfig>,

    pub gmail_push: Option<crate::channels::gmail_push::GmailPushConfig>,

    pub irc: Option<IrcConfig>,

    pub lark: Option<LarkConfig>,

    pub feishu: Option<FeishuConfig>,

    pub dingtalk: Option<DingTalkConfig>,

    pub wecom: Option<WeComConfig>,

    pub qq: Option<QQConfig>,

    pub twitter: Option<TwitterConfig>,

    pub mochat: Option<MochatConfig>,
    #[cfg(feature = "channel-nostr")]
    pub nostr: Option<NostrConfig>,

    pub telnyx: Option<crate::channels::TelnyxConfig>,

    pub reddit: Option<RedditConfig>,

    pub bluesky: Option<BlueskyConfig>,

    pub voice_call: Option<crate::channels::voice::call::VoiceCallConfig>,

    #[cfg(feature = "voice-wake")]
    pub voice_wake: Option<VoiceWakeConfig>,

    #[serde(default = "default_channel_message_timeout_secs")]
    pub message_timeout_secs: u64,

    #[serde(default = "default_true")]
    pub ack_reactions: bool,

    #[serde(default = "default_false")]
    pub show_tool_calls: bool,

    #[serde(default = "default_true")]
    pub session_persistence: bool,

    #[serde(default = "default_session_backend")]
    pub session_backend: String,

    #[serde(default)]
    pub session_ttl_hours: u32,

    #[serde(default)]
    pub debounce_ms: u64,
}

impl ChannelsConfig {

    #[rustfmt::skip]
    pub fn channels_except_webhook(&self) -> Vec<(Box<dyn super::traits::ConfigHandle>, bool)> {
        vec![
            (
                Box::new(ConfigWrapper::new(self.telegram.as_ref())),
                self.telegram.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.discord.as_ref())),
                self.discord.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.slack.as_ref())),
                self.slack.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.mattermost.as_ref())),
                self.mattermost.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.imessage.as_ref())),
                self.imessage.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.matrix.as_ref())),
                self.matrix.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.signal.as_ref())),
                self.signal.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.whatsapp.as_ref())),
                self.whatsapp.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.linq.as_ref())),
                self.linq.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.wati.as_ref())),
                self.wati.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.nextcloud_talk.as_ref())),
                self.nextcloud_talk.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.email.as_ref())),
                self.email.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.gmail_push.as_ref())),
                self.gmail_push.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.irc.as_ref())),
                self.irc.is_some()
            ),
            (
                Box::new(ConfigWrapper::new(self.lark.as_ref())),
                self.lark.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.feishu.as_ref())),
                self.feishu.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.dingtalk.as_ref())),
                self.dingtalk.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.wecom.as_ref())),
                self.wecom.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.qq.as_ref())),
                self.qq.is_some()
            ),
            #[cfg(feature = "channel-nostr")]
            (
                Box::new(ConfigWrapper::new(self.nostr.as_ref())),
                self.nostr.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.telnyx.as_ref())),
                self.telnyx.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.reddit.as_ref())),
                self.reddit.is_some(),
            ),
            (
                Box::new(ConfigWrapper::new(self.bluesky.as_ref())),
                self.bluesky.is_some(),
            ),
            #[cfg(feature = "voice-wake")]
            (
                Box::new(ConfigWrapper::new(self.voice_wake.as_ref())),
                self.voice_wake.is_some(),
            ),
        ]
    }

    pub fn channels(&self) -> Vec<(Box<dyn super::traits::ConfigHandle>, bool)> {
        let mut ret = self.channels_except_webhook();
        ret.push((
            Box::new(ConfigWrapper::new(self.webhook.as_ref())),
            self.webhook.is_some(),
        ));
        ret
    }
}

fn default_channel_message_timeout_secs() -> u64 {
    300
}

fn default_session_backend() -> String {
    "sqlite".into()
}

impl Default for ChannelsConfig {
    fn default() -> Self {
        Self {
            cli: true,
            telegram: None,
            discord: None,
            discord_history: None,
            slack: None,
            mattermost: None,
            webhook: None,
            imessage: None,
            matrix: None,
            signal: None,
            whatsapp: None,
            linq: None,
            wati: None,
            nextcloud_talk: None,
            email: None,
            gmail_push: None,
            irc: None,
            lark: None,
            feishu: None,
            dingtalk: None,
            wecom: None,
            qq: None,
            twitter: None,
            mochat: None,
            #[cfg(feature = "channel-nostr")]
            nostr: None,
            telnyx: None,
            reddit: None,
            bluesky: None,
            voice_call: None,
            #[cfg(feature = "voice-wake")]
            voice_wake: None,
            message_timeout_secs: default_channel_message_timeout_secs(),
            ack_reactions: true,
            show_tool_calls: false,
            session_persistence: true,
            session_backend: default_session_backend(),
            session_ttl_hours: 0,
            debounce_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StreamMode {

    #[default]
    Off,

    Partial,

    #[serde(rename = "multi_message")]
    MultiMessage,
}

fn default_multi_message_delay_ms() -> u64 {
    800
}

fn default_matrix_draft_update_interval_ms() -> u64 {
    1500
}

pub use crate::config::domain::channels::integration::{
    DiscordConfig, DiscordHistoryConfig, MattermostConfig, SlackConfig, TelegramConfig,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WebhookConfig {

    pub port: u16,

    #[serde(default)]
    pub listen_path: Option<String>,

    #[serde(default)]
    pub send_url: Option<String>,

    #[serde(default)]
    pub send_method: Option<String>,

    #[serde(default)]
    pub auth_header: Option<String>,

    #[serde(default)]
    pub secret: Option<String>,
}

impl ChannelConfig for WebhookConfig {
    fn name() -> &'static str {
        "Webhook"
    }
    fn desc() -> &'static str {
        "HTTP endpoint"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IMessageConfig {

    pub allowed_contacts: Vec<String>,
}

impl ChannelConfig for IMessageConfig {
    fn name() -> &'static str {
        "iMessage"
    }
    fn desc() -> &'static str {
        "macOS only"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MatrixConfig {

    pub homeserver: String,

    pub access_token: String,

    #[serde(default)]
    pub user_id: Option<String>,

    #[serde(default)]
    pub device_id: Option<String>,

    pub room_id: String,

    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub allowed_rooms: Vec<String>,

    #[serde(default)]
    pub interrupt_on_new_message: bool,

    #[serde(default)]
    pub stream_mode: StreamMode,

    #[serde(default = "default_matrix_draft_update_interval_ms")]
    pub draft_update_interval_ms: u64,

    #[serde(default = "default_multi_message_delay_ms")]
    pub multi_message_delay_ms: u64,

    #[serde(default)]
    pub recovery_key: Option<String>,
}

impl ChannelConfig for MatrixConfig {
    fn name() -> &'static str {
        "Matrix"
    }
    fn desc() -> &'static str {
        "self-hosted chat"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SignalConfig {

    pub http_url: String,

    pub account: String,

    #[serde(default)]
    pub group_id: Option<String>,

    #[serde(default)]
    pub allowed_from: Vec<String>,

    #[serde(default)]
    pub ignore_attachments: bool,

    #[serde(default)]
    pub ignore_stories: bool,

    #[serde(default)]
    pub proxy_url: Option<String>,
}

impl ChannelConfig for SignalConfig {
    fn name() -> &'static str {
        "Signal"
    }
    fn desc() -> &'static str {
        "An open-source, encrypted messaging service"
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WhatsAppWebMode {

    #[default]
    Business,

    Personal,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WhatsAppChatPolicy {

    #[default]
    Allowlist,

    Ignore,

    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WhatsAppConfig {

    #[serde(default)]
    pub access_token: Option<String>,

    #[serde(default)]
    pub phone_number_id: Option<String>,

    #[serde(default)]
    pub verify_token: Option<String>,

    #[serde(default)]
    pub app_secret: Option<String>,

    #[serde(default)]
    pub session_path: Option<String>,

    #[serde(default)]
    pub pair_phone: Option<String>,

    #[serde(default)]
    pub pair_code: Option<String>,

    #[serde(default)]
    pub allowed_numbers: Vec<String>,

    #[serde(default)]
    pub mode: WhatsAppWebMode,

    #[serde(default)]
    pub dm_policy: WhatsAppChatPolicy,

    #[serde(default)]
    pub group_policy: WhatsAppChatPolicy,

    #[serde(default)]
    pub self_chat_mode: bool,

    #[serde(default)]
    pub dm_mention_patterns: Vec<String>,

    #[serde(default)]
    pub group_mention_patterns: Vec<String>,

    #[serde(default)]
    pub proxy_url: Option<String>,
}

impl ChannelConfig for WhatsAppConfig {
    fn name() -> &'static str {
        "WhatsApp"
    }
    fn desc() -> &'static str {
        "Business Cloud API"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LinqConfig {

    pub api_token: String,

    pub from_phone: String,

    #[serde(default)]
    pub signing_secret: Option<String>,

    #[serde(default)]
    pub allowed_senders: Vec<String>,
}

impl ChannelConfig for LinqConfig {
    fn name() -> &'static str {
        "Linq"
    }
    fn desc() -> &'static str {
        "iMessage/RCS/SMS via Linq API"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WatiConfig {

    pub api_token: String,

    #[serde(default = "default_wati_api_url")]
    pub api_url: String,

    #[serde(default)]
    pub tenant_id: Option<String>,

    #[serde(default)]
    pub allowed_numbers: Vec<String>,

    #[serde(default)]
    pub proxy_url: Option<String>,
}

fn default_wati_api_url() -> String {
    "https://live-mt-server.wati.io".to_string()
}

impl ChannelConfig for WatiConfig {
    fn name() -> &'static str {
        "WATI"
    }
    fn desc() -> &'static str {
        "WhatsApp via WATI Business API"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NextcloudTalkConfig {

    pub base_url: String,

    pub app_token: String,

    #[serde(default)]
    pub webhook_secret: Option<String>,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub proxy_url: Option<String>,

    #[serde(default)]
    pub bot_name: Option<String>,
}

impl ChannelConfig for NextcloudTalkConfig {
    fn name() -> &'static str {
        "NextCloud Talk"
    }
    fn desc() -> &'static str {
        "NextCloud Talk platform"
    }
}

impl WhatsAppConfig {

    pub fn backend_type(&self) -> &'static str {
        if self.phone_number_id.is_some() {
            "cloud"
        } else if self.session_path.is_some() {
            "web"
        } else {

            "cloud"
        }
    }

    pub fn is_cloud_config(&self) -> bool {
        self.phone_number_id.is_some() && self.access_token.is_some() && self.verify_token.is_some()
    }

    pub fn is_web_config(&self) -> bool {
        self.session_path.is_some()
    }

    pub fn is_ambiguous_config(&self) -> bool {
        self.phone_number_id.is_some() && self.session_path.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IrcConfig {

    pub server: String,

    #[serde(default = "default_irc_port")]
    pub port: u16,

    pub nickname: String,

    pub username: Option<String>,

    #[serde(default)]
    pub channels: Vec<String>,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    pub server_password: Option<String>,

    pub nickserv_password: Option<String>,

    pub sasl_password: Option<String>,

    pub verify_tls: Option<bool>,
}

impl ChannelConfig for IrcConfig {
    fn name() -> &'static str {
        "IRC"
    }
    fn desc() -> &'static str {
        "IRC over TLS"
    }
}

fn default_irc_port() -> u16 {
    6697
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LarkReceiveMode {
    #[default]
    Websocket,
    Webhook,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LarkConfig {

    pub app_id: String,

    pub app_secret: String,

    #[serde(default)]
    pub encrypt_key: Option<String>,

    #[serde(default)]
    pub verification_token: Option<String>,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub mention_only: bool,

    #[serde(default)]
    pub use_feishu: bool,

    #[serde(default)]
    pub receive_mode: LarkReceiveMode,

    #[serde(default)]
    pub port: Option<u16>,

    #[serde(default)]
    pub proxy_url: Option<String>,
}

impl ChannelConfig for LarkConfig {
    fn name() -> &'static str {
        "Lark"
    }
    fn desc() -> &'static str {
        "Lark Bot"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeishuConfig {

    pub app_id: String,

    pub app_secret: String,

    #[serde(default)]
    pub encrypt_key: Option<String>,

    #[serde(default)]
    pub verification_token: Option<String>,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub receive_mode: LarkReceiveMode,

    #[serde(default)]
    pub port: Option<u16>,

    #[serde(default)]
    pub proxy_url: Option<String>,
}

impl ChannelConfig for FeishuConfig {
    fn name() -> &'static str {
        "Feishu"
    }
    fn desc() -> &'static str {
        "Feishu Bot"
    }
}
pub use crate::config::domain::security::{SecurityConfig, WebAuthnConfig};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OtpMethod {

    #[default]
    Totp,

    Pairing,

    CliPrompt,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OtpConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub method: OtpMethod,

    #[serde(default = "default_otp_token_ttl_secs")]
    pub token_ttl_secs: u64,

    #[serde(default = "default_otp_cache_valid_secs")]
    pub cache_valid_secs: u64,

    #[serde(default = "default_otp_gated_actions")]
    pub gated_actions: Vec<String>,

    #[serde(default)]
    pub gated_domains: Vec<String>,

    #[serde(default)]
    pub gated_domain_categories: Vec<String>,

    #[serde(default = "default_otp_challenge_max_attempts")]
    pub challenge_max_attempts: u32,
}

fn default_otp_token_ttl_secs() -> u64 {
    30
}

fn default_otp_cache_valid_secs() -> u64 {
    300
}

fn default_otp_challenge_max_attempts() -> u32 {
    3
}

fn default_otp_gated_actions() -> Vec<String> {
    vec![
        "shell".to_string(),
        "file_write".to_string(),
        "browser_open".to_string(),
        "browser".to_string(),
        "memory_forget".to_string(),
    ]
}

impl Default for OtpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            method: OtpMethod::Totp,
            token_ttl_secs: default_otp_token_ttl_secs(),
            cache_valid_secs: default_otp_cache_valid_secs(),
            gated_actions: default_otp_gated_actions(),
            gated_domains: Vec::new(),
            gated_domain_categories: Vec::new(),
            challenge_max_attempts: default_otp_challenge_max_attempts(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EstopConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_estop_state_file")]
    pub state_file: String,

    #[serde(default = "default_true")]
    pub require_otp_to_resume: bool,
}

fn default_estop_state_file() -> String {
    "~/.senweavercoding/estop-state.json".to_string()
}

impl Default for EstopConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            state_file: default_estop_state_file(),
            require_otp_to_resume: true,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
pub struct NevisConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub instance_url: String,

    #[serde(default = "default_nevis_realm")]
    pub realm: String,

    #[serde(default)]
    pub client_id: String,

    #[serde(default)]
    pub client_secret: Option<String>,

    #[serde(default = "default_nevis_token_validation")]
    pub token_validation: String,

    #[serde(default)]
    pub jwks_url: Option<String>,

    #[serde(default)]
    pub role_mapping: Vec<NevisRoleMappingConfig>,

    #[serde(default)]
    pub require_mfa: bool,

    #[serde(default = "default_nevis_session_timeout_secs")]
    pub session_timeout_secs: u64,
}

impl std::fmt::Debug for NevisConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NevisConfig")
            .field("enabled", &self.enabled)
            .field("instance_url", &self.instance_url)
            .field("realm", &self.realm)
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("token_validation", &self.token_validation)
            .field("jwks_url", &self.jwks_url)
            .field("role_mapping", &self.role_mapping)
            .field("require_mfa", &self.require_mfa)
            .field("session_timeout_secs", &self.session_timeout_secs)
            .finish()
    }
}

impl NevisConfig {

    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        if self.instance_url.trim().is_empty() {
            return Err("nevis.instance_url is required when Nevis IAM is enabled".into());
        }

        if self.client_id.trim().is_empty() {
            return Err("nevis.client_id is required when Nevis IAM is enabled".into());
        }

        if self.realm.trim().is_empty() {
            return Err("nevis.realm is required when Nevis IAM is enabled".into());
        }

        match self.token_validation.as_str() {
            "local" | "remote" => {}
            other => {
                return Err(format!(
                    "nevis.token_validation has invalid value '{other}': \
                     expected 'local' or 'remote'"
                ));
            }
        }

        if self.token_validation == "local" && self.jwks_url.is_none() {
            return Err("nevis.jwks_url is required when token_validation is 'local'".into());
        }

        if self.session_timeout_secs == 0 {
            return Err("nevis.session_timeout_secs must be greater than 0".into());
        }

        Ok(())
    }
}

fn default_nevis_realm() -> String {
    "master".into()
}

fn default_nevis_token_validation() -> String {
    "remote".into()
}

fn default_nevis_session_timeout_secs() -> u64 {
    3600
}

impl Default for NevisConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            instance_url: String::new(),
            realm: default_nevis_realm(),
            client_id: String::new(),
            client_secret: None,
            token_validation: default_nevis_token_validation(),
            jwks_url: None,
            role_mapping: Vec::new(),
            require_mfa: false,
            session_timeout_secs: default_nevis_session_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NevisRoleMappingConfig {

    pub nevis_role: String,

    #[serde(default)]
    pub sen_permissions: Vec<String>,

    #[serde(default)]
    pub workspace_access: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SandboxConfig {

    #[serde(default)]
    pub enabled: Option<bool>,

    #[serde(default)]
    pub backend: SandboxBackend,

    #[serde(default)]
    pub firejail_args: Vec<String>,

    #[serde(default = "default_true")]
    pub confine_filesystem: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: None,
            backend: SandboxBackend::Auto,
            firejail_args: Vec::new(),
            confine_filesystem: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SandboxBackend {

    #[default]
    Auto,

    Landlock,

    Firejail,

    Bubblewrap,

    Docker,

    #[serde(alias = "sandbox-exec")]
    SandboxExec,

    Wasm,

    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ResourceLimitsConfig {

    #[serde(default = "default_max_memory_mb")]
    pub max_memory_mb: u32,

    #[serde(default = "default_max_cpu_time_seconds")]
    pub max_cpu_time_seconds: u64,

    #[serde(default = "default_max_subprocesses")]
    pub max_subprocesses: u32,

    #[serde(default = "default_memory_monitoring_enabled")]
    pub memory_monitoring: bool,
}

fn default_max_memory_mb() -> u32 {
    512
}

fn default_max_cpu_time_seconds() -> u64 {
    60
}

fn default_max_subprocesses() -> u32 {
    10
}

fn default_memory_monitoring_enabled() -> bool {
    true
}

impl Default for ResourceLimitsConfig {
    fn default() -> Self {
        Self {
            max_memory_mb: default_max_memory_mb(),
            max_cpu_time_seconds: default_max_cpu_time_seconds(),
            max_subprocesses: default_max_subprocesses(),
            memory_monitoring: default_memory_monitoring_enabled(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuditConfig {

    #[serde(default = "default_audit_enabled")]
    pub enabled: bool,

    #[serde(default = "default_audit_log_path")]
    pub log_path: String,

    #[serde(default = "default_audit_max_size_mb")]
    pub max_size_mb: u32,

    #[serde(default)]
    pub sign_events: bool,
}

fn default_audit_enabled() -> bool {
    true
}

fn default_audit_log_path() -> String {
    "audit.log".to_string()
}

fn default_audit_max_size_mb() -> u32 {
    100
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: default_audit_enabled(),
            log_path: default_audit_log_path(),
            max_size_mb: default_audit_max_size_mb(),
            sign_events: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DingTalkConfig {

    pub client_id: String,

    pub client_secret: String,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub proxy_url: Option<String>,
}

impl ChannelConfig for DingTalkConfig {
    fn name() -> &'static str {
        "DingTalk"
    }
    fn desc() -> &'static str {
        "DingTalk Stream Mode"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WeComConfig {

    pub webhook_key: String,

    #[serde(default)]
    pub allowed_users: Vec<String>,
}

impl ChannelConfig for WeComConfig {
    fn name() -> &'static str {
        "WeCom"
    }
    fn desc() -> &'static str {
        "WeCom Bot Webhook"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QQConfig {

    pub app_id: String,

    pub app_secret: String,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub proxy_url: Option<String>,
}

impl ChannelConfig for QQConfig {
    fn name() -> &'static str {
        "QQ Official"
    }
    fn desc() -> &'static str {
        "Tencent QQ Bot"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TwitterConfig {

    pub bearer_token: String,

    #[serde(default)]
    pub allowed_users: Vec<String>,
}

impl ChannelConfig for TwitterConfig {
    fn name() -> &'static str {
        "X/Twitter"
    }
    fn desc() -> &'static str {
        "X/Twitter Bot via API v2"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MochatConfig {

    pub api_url: String,

    pub api_token: String,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default = "default_mochat_poll_interval")]
    pub poll_interval_secs: u64,
}

fn default_mochat_poll_interval() -> u64 {
    5
}

impl ChannelConfig for MochatConfig {
    fn name() -> &'static str {
        "Mochat"
    }
    fn desc() -> &'static str {
        "Mochat Customer Service"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RedditConfig {

    pub client_id: String,

    pub client_secret: String,

    pub refresh_token: String,

    pub username: String,

    #[serde(default)]
    pub subreddit: Option<String>,
}

impl ChannelConfig for RedditConfig {
    fn name() -> &'static str {
        "Reddit"
    }
    fn desc() -> &'static str {
        "Reddit bot (OAuth2)"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BlueskyConfig {

    pub handle: String,

    pub app_password: String,
}

impl ChannelConfig for BlueskyConfig {
    fn name() -> &'static str {
        "Bluesky"
    }
    fn desc() -> &'static str {
        "AT Protocol"
    }
}

#[cfg(feature = "voice-wake")]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VoiceWakeConfig {

    #[serde(default = "default_voice_wake_word")]
    pub wake_word: String,

    #[serde(default = "default_voice_wake_silence_timeout_ms")]
    pub silence_timeout_ms: u32,

    #[serde(default = "default_voice_wake_energy_threshold")]
    pub energy_threshold: f32,

    #[serde(default = "default_voice_wake_max_capture_secs")]
    pub max_capture_secs: u32,
}

#[cfg(feature = "voice-wake")]
fn default_voice_wake_word() -> String {
    "hey sen".into()
}

#[cfg(feature = "voice-wake")]
fn default_voice_wake_silence_timeout_ms() -> u32 {
    2000
}

#[cfg(feature = "voice-wake")]
fn default_voice_wake_energy_threshold() -> f32 {
    0.01
}

#[cfg(feature = "voice-wake")]
fn default_voice_wake_max_capture_secs() -> u32 {
    30
}

#[cfg(feature = "voice-wake")]
impl Default for VoiceWakeConfig {
    fn default() -> Self {
        Self {
            wake_word: default_voice_wake_word(),
            silence_timeout_ms: default_voice_wake_silence_timeout_ms(),
            energy_threshold: default_voice_wake_energy_threshold(),
            max_capture_secs: default_voice_wake_max_capture_secs(),
        }
    }
}

#[cfg(feature = "voice-wake")]
impl ChannelConfig for VoiceWakeConfig {
    fn name() -> &'static str {
        "VoiceWake"
    }
    fn desc() -> &'static str {
        "voice wake word detection"
    }
}

#[cfg(feature = "channel-nostr")]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NostrConfig {

    pub private_key: String,

    #[serde(default = "default_nostr_relays")]
    pub relays: Vec<String>,

    #[serde(default)]
    pub allowed_pubkeys: Vec<String>,
}

#[cfg(feature = "channel-nostr")]
impl ChannelConfig for NostrConfig {
    fn name() -> &'static str {
        "Nostr"
    }
    fn desc() -> &'static str {
        "Nostr DMs"
    }
}

#[cfg(feature = "channel-nostr")]
pub fn default_nostr_relays() -> Vec<String> {
    vec![
        "wss://relay.damus.io".to_string(),
        "wss://nos.lol".to_string(),
        "wss://relay.primal.net".to_string(),
        "wss://relay.snort.social".to_string(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotionConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub database_id: String,
    #[serde(default = "default_notion_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_notion_status_prop")]
    pub status_property: String,
    #[serde(default = "default_notion_input_prop")]
    pub input_property: String,
    #[serde(default = "default_notion_result_prop")]
    pub result_property: String,
    #[serde(default = "default_notion_max_concurrent")]
    pub max_concurrent: usize,
    #[serde(default = "default_notion_recover_stale")]
    pub recover_stale: bool,
}

fn default_notion_poll_interval() -> u64 {
    5
}
fn default_notion_status_prop() -> String {
    "Status".into()
}
fn default_notion_input_prop() -> String {
    "Input".into()
}
fn default_notion_result_prop() -> String {
    "Result".into()
}
fn default_notion_max_concurrent() -> usize {
    4
}
fn default_notion_recover_stale() -> bool {
    true
}

impl Default for NotionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            database_id: String::new(),
            poll_interval_secs: default_notion_poll_interval(),
            status_property: default_notion_status_prop(),
            input_property: default_notion_input_prop(),
            result_property: default_notion_result_prop(),
            max_concurrent: default_notion_max_concurrent(),
            recover_stale: default_notion_recover_stale(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JiraConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub base_url: String,

    #[serde(default)]
    pub email: String,

    #[serde(default)]
    pub api_token: String,

    #[serde(default = "default_jira_allowed_actions")]
    pub allowed_actions: Vec<String>,

    #[serde(default = "default_jira_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_jira_allowed_actions() -> Vec<String> {
    vec!["get_ticket".to_string()]
}

fn default_jira_timeout_secs() -> u64 {
    30
}

impl Default for JiraConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: String::new(),
            email: String::new(),
            api_token: String::new(),
            allowed_actions: default_jira_allowed_actions(),
            timeout_secs: default_jira_timeout_secs(),
        }
    }
}

pub use crate::config::domain::cloud_ops::CloudOpsConfig;

pub use crate::buddy::types::BuddyConfig;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HandsConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub dir: Option<String>,

    #[serde(default = "default_hands_poll_secs")]
    pub poll_secs: u64,
}

fn default_hands_poll_secs() -> u64 {
    60
}

impl HandsConfig {
    pub fn is_disabled(&self) -> bool {
        !self.enabled
    }
}

impl Default for HandsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dir: None,
            poll_secs: default_hands_poll_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SecurityOpsConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_playbooks_dir")]
    pub playbooks_dir: String,

    #[serde(default)]
    pub auto_triage: bool,

    #[serde(default = "default_require_approval")]
    pub require_approval_for_actions: bool,

    #[serde(default = "default_max_auto_severity")]
    pub max_auto_severity: String,

    #[serde(default = "default_report_output_dir")]
    pub report_output_dir: String,

    #[serde(default)]
    pub siem_integration: Option<String>,
}

fn default_playbooks_dir() -> String {
    "~/.senweavercoding/playbooks".into()
}

fn default_require_approval() -> bool {
    true
}

fn default_max_auto_severity() -> String {
    "low".into()
}

fn default_report_output_dir() -> String {
    "~/.senweavercoding/security-reports".into()
}

impl Default for SecurityOpsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            playbooks_dir: default_playbooks_dir(),
            auto_triage: false,
            require_approval_for_actions: true,
            max_auto_severity: default_max_auto_severity(),
            report_output_dir: default_report_output_dir(),
            siem_integration: None,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let home =
            UserDirs::new().map_or_else(|| PathBuf::from("."), |u| u.home_dir().to_path_buf());
        let sen_dir = home.join(".senweavercoding");

        Self {
            workspace_dir: sen_dir.join("workspace"),
            config_path: sen_dir.join("config.toml"),
            env_override_baseline: None,
            api_key: None,
            api_url: None,
            api_path: None,
            default_provider: Some("openrouter".to_string()),
            default_model: None,
            model_providers: HashMap::new(),
            default_temperature: default_temperature(),
            provider_timeout_secs: default_provider_timeout_secs(),
            provider_max_tokens: None,
            model_context_windows: HashMap::new(),
            extra_headers: HashMap::new(),
            observability: ObservabilityConfig::default(),
            autonomy: AutonomyConfig::default(),
            trust: crate::trust::TrustConfig::default(),
            backup: BackupConfig::default(),
            data_retention: DataRetentionConfig::default(),
            cloud_ops: CloudOpsConfig::default(),
            hands: HandsConfig::default(),
            buddy: BuddyConfig::default(),
            security: SecurityConfig::default(),
            security_ops: SecurityOpsConfig::default(),
            runtime: RuntimeConfig::default(),
            reliability: ReliabilityConfig::default(),
            scheduler: SchedulerConfig::default(),
            agent: AgentConfig::default(),
            pacing: PacingConfig::default(),
            agent_runtime: AgentRuntimeExtras::default(),
            skills: SkillsConfig::default(),
            pipeline: PipelineConfig::default(),
            model_routes: Vec::new(),
            saved_models: Vec::new(),
            embedding_routes: Vec::new(),
            heartbeat: HeartbeatConfig::default(),
            lan: LanConfig::default(),
            cron: CronConfig::default(),
            channels_config: ChannelsConfig::default(),
            memory: MemoryConfig::default(),
            memory_runtime: crate::config::domain::MemoryRuntimeExtras::default(),
            storage: StorageConfig::default(),
            tunnel: TunnelConfig::default(),
            gateway: GatewayConfig::default(),
            rpc: RpcConfig::default(),
            composio: ComposioConfig::default(),
            microsoft365: Microsoft365Config::default(),
            secrets: SecretsConfig::default(),
            browser: BrowserConfig::default(),
            browser_delegate: crate::tools::browser::delegate::BrowserDelegateConfig::default(),
            http_request: HttpRequestConfig::default(),
            multimodal: MultimodalConfig::default(),
            media_pipeline: MediaPipelineConfig::default(),
            web_fetch: WebFetchConfig::default(),
            link_enricher: LinkEnricherConfig::default(),
            text_browser: TextBrowserConfig::default(),
            web_search: WebSearchConfig::default(),
            project_intel: ProjectIntelConfig::default(),
            google_workspace: GoogleWorkspaceConfig::default(),
            proxy: ProxyConfig::default(),
            identity: IdentityConfig::default(),
            cost: CostConfig::default(),
            evolution: crate::config::domain::evolution::EvolutionConfig::default(),
            peripherals: PeripheralsConfig::default(),
            delegate: DelegateToolConfig::default(),
            agents: HashMap::new(),
            swarms: HashMap::new(),
            teams: TeamsConfig::default(),
            hooks: HooksConfig::default(),
            hardware: HardwareConfig::default(),
            query_classification: QueryClassificationConfig::default(),
            transcription: TranscriptionConfig::default(),
            tts: TtsConfig::default(),
            mcp: McpConfig::default(),
            nodes: NodesConfig::default(),
            workspace: WorkspaceConfig::default(),
            notion: NotionConfig::default(),
            jira: JiraConfig::default(),
            knowledge: KnowledgeConfig::default(),
            linkedin: LinkedInConfig::default(),
            image_gen: ImageGenConfig::default(),
            plugins: PluginsConfig::default(),
            locale: None,
            verifiable_intent: VerifiableIntentConfig::default(),
            claude_code: ClaudeCodeConfig::default(),
            claude_code_runner: ClaudeCodeRunnerConfig::default(),
            codex_cli: CodexCliConfig::default(),
            gemini_cli: GeminiCliConfig::default(),
            opencode_cli: OpenCodeCliConfig::default(),
            sop: SopConfig::default(),
            shell_tool: ShellToolConfig::default(),
            guardrails: crate::guardrails::GuardrailsConfig::default(),
            plan_mode: crate::agent::plan_mode::PlanModeConfig::default(),
            intent_analysis: crate::agent::intent::IntentAnalysisConfig::default(),
            auto_title: crate::agent::auto_title::AutoTitleConfig::default(),
            suggestions: crate::agent::suggestions::SuggestionsConfig::default(),
            tool_groups: crate::tools::handler::groups::ToolGroupsConfig::default(),
            user_profile: crate::agent::user::profile::UserProfileConfig::default(),
            self_eval: crate::agent::self_assess::eval::SelfEvalConfig::default(),
            feedback: crate::agent::reward::feedback::FeedbackConfig::default(),
            experience: crate::agent::reward::experience::ExperienceConfig::default(),
            prompt_optimizer: crate::agent::prompt::optimizer::PromptOptimizerConfig::default(),
            skill_evolution: crate::agent::skill_evolution::SkillEvolutionConfig::default(),
            reinforcement: crate::agent::reward::reinforcement::ReinforcementConfig::default(),
            rbac: crate::security::rbac::RbacConfig::default(),
            tool_output_compressor:
                crate::agent::tool_handler::output_compressor::ToolOutputCompressorConfig::default(),
            code_rag: CodeRagConfig::default(),
            token_budget: crate::agent::token::budget::TokenBudgetConfig::default(),
            token_saver: TokenSaverConfig::default(),
            custom_tools: CustomToolsConfig::default(),
            lsp: LspConfig::default(),
            permissions: crate::security::permissions::PermissionsConfig::default(),
        }
    }
}

fn default_config_and_workspace_dirs() -> Result<(PathBuf, PathBuf)> {
    let config_dir = default_config_dir()?;
    Ok((config_dir.clone(), config_dir.join("workspace")))
}

const ACTIVE_WORKSPACE_STATE_FILE: &str = "active_workspace.toml";

#[derive(Debug, Serialize, Deserialize)]
struct ActiveWorkspaceState {
    config_dir: String,
}

fn default_config_dir() -> Result<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return Ok(PathBuf::from(home).join(".senweavercoding"));
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        if !profile.is_empty() {
            return Ok(PathBuf::from(profile).join(".senweavercoding"));
        }
    }

    if let Some(home) = UserDirs::new().map(|u| u.home_dir().to_path_buf()) {
        return Ok(home.join(".senweavercoding"));
    }

    let mut fallback = std::env::temp_dir();
    fallback.push("SenAgentOS");
    fallback.push("home");
    fallback.push(".senweavercoding");
    tracing::warn!(
        path = %fallback.display(),
        "Could not resolve a home directory; falling back to %TEMP%/SenAgentOS so the desktop \
         can still launch. Configure HOME/USERPROFILE for a stable config location."
    );
    Ok(fallback)
}

fn active_workspace_state_path(default_dir: &Path) -> PathBuf {
    default_dir.join(ACTIVE_WORKSPACE_STATE_FILE)
}

fn is_temp_directory(path: &Path) -> bool {
    let temp = std::env::temp_dir();

    let canon_temp = temp.canonicalize().unwrap_or_else(|_| temp.clone());
    let canon_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    canon_path.starts_with(&canon_temp)
}

struct ConfigWriteLock {
    path: PathBuf,
    held: bool,
}

impl ConfigWriteLock {
    const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(30);
    const ACQUIRE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

    async fn acquire(config_path: &Path) -> Self {
        let lock_path = {
            let name = config_path
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("config.toml");
            config_path.with_file_name(format!("{name}.lock"))
        };
        let deadline = std::time::Instant::now() + Self::ACQUIRE_TIMEOUT;
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(_) => {
                    return Self {
                        path: lock_path,
                        held: true,
                    };
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Steal a stale lock left behind by a crashed writer.
                    let stale = std::fs::metadata(&lock_path)
                        .and_then(|m| m.modified())
                        .map(|t| t.elapsed().unwrap_or_default() > Self::STALE_AFTER)
                        .unwrap_or(false);
                    if stale {
                        let _ = std::fs::remove_file(&lock_path);
                        continue;
                    }
                    if std::time::Instant::now() >= deadline {
                        tracing::warn!(
                            path = %lock_path.display(),
                            "config write lock still held after timeout; proceeding without it"
                        );
                        return Self {
                            path: lock_path,
                            held: false,
                        };
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(e) => {
                    tracing::warn!(
                        path = %lock_path.display(),
                        error = %e,
                        "could not create config write lock; proceeding without it"
                    );
                    return Self {
                        path: lock_path,
                        held: false,
                    };
                }
            }
        }
    }
}

impl Drop for ConfigWriteLock {
    fn drop(&mut self) {
        if self.held {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

async fn load_persisted_workspace_dirs(
    default_config_dir: &Path,
) -> Result<Option<(PathBuf, PathBuf)>> {
    let state_path = active_workspace_state_path(default_config_dir);
    if !state_path.exists() {
        return Ok(None);
    }

    let contents = match fs::read_to_string(&state_path).await {
        Ok(contents) => contents,
        Err(error) => {
            tracing::warn!(
                "Failed to read active workspace marker {}: {error}",
                state_path.display()
            );
            return Ok(None);
        }
    };

    let state: ActiveWorkspaceState = match toml::from_str(&contents) {
        Ok(state) => state,
        Err(error) => {
            tracing::warn!(
                "Failed to parse active workspace marker {}: {error}",
                state_path.display()
            );
            return Ok(None);
        }
    };

    let raw_config_dir = state.config_dir.trim();
    if raw_config_dir.is_empty() {
        tracing::warn!(
            "Ignoring active workspace marker {} because config_dir is empty",
            state_path.display()
        );
        return Ok(None);
    }

    let parsed_dir = expand_tilde_path(raw_config_dir);
    let config_dir = if parsed_dir.is_absolute() {
        parsed_dir
    } else {
        default_config_dir.join(parsed_dir)
    };
    Ok(Some((config_dir.clone(), config_dir.join("workspace"))))
}

pub(crate) async fn persist_active_workspace_config_dir(config_dir: &Path) -> Result<()> {
    persist_active_workspace_config_dir_in(config_dir, &default_config_dir()?).await
}

async fn persist_active_workspace_config_dir_in(
    config_dir: &Path,
    default_config_dir: &Path,
) -> Result<()> {
    let state_path = active_workspace_state_path(default_config_dir);

    if is_temp_directory(config_dir) && !is_temp_directory(default_config_dir) {
        tracing::warn!(
            path = %config_dir.display(),
            "Refusing to persist temp directory as active workspace marker"
        );
        return Ok(());
    }

    if config_dir == default_config_dir {
        if state_path.exists() {
            fs::remove_file(&state_path).await.with_context(|| {
                format!(
                    "Failed to clear active workspace marker: {}",
                    state_path.display()
                )
            })?;
        }
        return Ok(());
    }

    fs::create_dir_all(&default_config_dir)
        .await
        .with_context(|| {
            format!(
                "Failed to create default config directory: {}",
                default_config_dir.display()
            )
        })?;

    let state = ActiveWorkspaceState {
        config_dir: config_dir.to_string_lossy().into_owned(),
    };
    let serialized =
        toml::to_string_pretty(&state).context("Failed to serialize active workspace marker")?;

    let temp_path = default_config_dir.join(format!(
        ".{ACTIVE_WORKSPACE_STATE_FILE}.tmp-{}",
        uuid::Uuid::new_v4()
    ));
    fs::write(&temp_path, serialized).await.with_context(|| {
        format!(
            "Failed to write temporary active workspace marker: {}",
            temp_path.display()
        )
    })?;

    if let Err(error) = fs::rename(&temp_path, &state_path).await {
        let _ = fs::remove_file(&temp_path).await;
        anyhow::bail!(
            "Failed to atomically persist active workspace marker {}: {error}",
            state_path.display()
        );
    }

    sync_directory(default_config_dir).await?;
    Ok(())
}

// Stamp file sen writes into every config dir it owns. Its presence is the
// authoritative signal that a directory is a sen config home, so we never adopt
// or overwrite an unrelated project's `config.toml`.
pub(crate) const SEN_CONFIG_DIR_MARKER: &str = ".sen-config-dir";

// A directory is a sen config home only if it carries a sen marker — never
// merely because it contains a `config.toml`. Many code projects ship their own
// unrelated `config.toml`; adopting one silently clobbers the user's file.
pub(crate) fn is_sen_config_dir(dir: &Path) -> bool {
    if dir.join(SEN_CONFIG_DIR_MARKER).exists() {
        return true;
    }
    // The default `~/.senweavercoding` home is sen-owned by name.
    if dir
        .file_name()
        .is_some_and(|n| n == std::ffi::OsStr::new(".senweavercoding"))
    {
        return dir.join("config.toml").exists();
    }
    // Backward-compat for portable homes created before the stamp existed: a
    // `config.toml` sitting next to an unambiguously sen-created artifact.
    if dir.join("config.toml").exists() {
        const SEN_ARTIFACTS: &[&str] =
            &[".secret_key", "sessions", "workspace", ACTIVE_WORKSPACE_STATE_FILE];
        return SEN_ARTIFACTS.iter().any(|a| dir.join(a).exists());
    }
    false
}

pub(crate) fn resolve_config_dir_for_workspace(workspace_dir: &Path) -> (PathBuf, PathBuf) {
    let workspace_config_dir = workspace_dir.to_path_buf();
    if is_sen_config_dir(&workspace_config_dir) {
        return (
            workspace_config_dir.clone(),
            workspace_config_dir.join("workspace"),
        );
    }

    let legacy_config_dir = workspace_dir
        .parent()
        .map(|parent| parent.join(".senweavercoding"));
    if let Some(legacy_dir) = legacy_config_dir {
        if is_sen_config_dir(&legacy_dir) {
            return (legacy_dir, workspace_config_dir);
        }

        if workspace_dir
            .file_name()
            .is_some_and(|name| name == std::ffi::OsStr::new("workspace"))
        {
            return (legacy_dir, workspace_config_dir);
        }
    }

    // The target holds a foreign (non-sen) `config.toml`. Do NOT claim it: keep
    // the directory as the workspace and namespace sen's own config into a
    // clearly sen-owned `.senweavercoding` subdir so the user's file is never
    // read as ours, renamed, or overwritten.
    if workspace_config_dir.join("config.toml").exists() {
        return (
            workspace_config_dir.join(".senweavercoding"),
            workspace_config_dir,
        );
    }

    (
        workspace_config_dir.clone(),
        workspace_config_dir.join("workspace"),
    )
}

pub async fn resolve_runtime_dirs_for_onboarding() -> Result<(PathBuf, PathBuf)> {
    let (default_sen_dir, default_workspace_dir) = default_config_and_workspace_dirs()?;
    let (config_dir, workspace_dir, _) =
        resolve_runtime_config_dirs(&default_sen_dir, &default_workspace_dir).await?;
    Ok((config_dir, workspace_dir))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfigResolutionSource {
    EnvConfigDir,
    EnvWorkspace,
    ActiveWorkspaceMarker,
    DefaultConfigDir,
}

impl ConfigResolutionSource {
    const fn as_str(self) -> &'static str {
        match self {
            Self::EnvConfigDir => "SEN_CONFIG_DIR",
            Self::EnvWorkspace => "SEN_WORKSPACE",
            Self::ActiveWorkspaceMarker => "active_workspace.toml",
            Self::DefaultConfigDir => "default",
        }
    }
}

pub fn expand_tilde_path(path: &str) -> PathBuf {
    let expanded = shellexpand::tilde(path);
    let expanded_str = expanded.as_ref();

    if expanded_str.starts_with('~') {
        if let Some(user_dirs) = UserDirs::new() {
            let home = user_dirs.home_dir();

            if let Some(rest) = expanded_str.strip_prefix('~') {
                return home.join(rest.trim_start_matches(['/', '\\']));
            }
        }

        tracing::warn!(
            path = path,
            "Failed to expand tilde: HOME environment variable is not set and UserDirs failed. \
             In cron/non-TTY environments, use absolute paths or set HOME explicitly."
        );
    }

    PathBuf::from(expanded_str)
}

async fn resolve_runtime_config_dirs(
    default_sen_dir: &Path,
    default_workspace_dir: &Path,
) -> Result<(PathBuf, PathBuf, ConfigResolutionSource)> {
    if let Some(custom_config_dir) = crate::util::get_runtime_var("SEN_CONFIG_DIR") {
        let custom_config_dir = custom_config_dir.trim();
        if !custom_config_dir.is_empty() {
            let sen_dir = expand_tilde_path(custom_config_dir);
            return Ok((
                sen_dir.clone(),
                sen_dir.join("workspace"),
                ConfigResolutionSource::EnvConfigDir,
            ));
        }
    }

    if let Some(custom_workspace) = crate::util::get_runtime_var("SEN_WORKSPACE") {
        if !custom_workspace.is_empty() {
            let expanded = expand_tilde_path(&custom_workspace);
            let (sen_dir, workspace_dir) = resolve_config_dir_for_workspace(&expanded);
            return Ok((sen_dir, workspace_dir, ConfigResolutionSource::EnvWorkspace));
        }
    }

    if let Some((sen_dir, workspace_dir)) = load_persisted_workspace_dirs(default_sen_dir).await? {
        return Ok((
            sen_dir,
            workspace_dir,
            ConfigResolutionSource::ActiveWorkspaceMarker,
        ));
    }

    Ok((
        default_sen_dir.to_path_buf(),
        default_workspace_dir.to_path_buf(),
        ConfigResolutionSource::DefaultConfigDir,
    ))
}

fn resolve_runtime_config_dirs_sync(
    default_sen_dir: &Path,
    default_workspace_dir: &Path,
) -> (PathBuf, PathBuf) {
    if let Some(custom_config_dir) = crate::util::get_runtime_var("SEN_CONFIG_DIR") {
        let custom_config_dir = custom_config_dir.trim();
        if !custom_config_dir.is_empty() {
            let sen_dir = expand_tilde_path(custom_config_dir);
            return (sen_dir.clone(), sen_dir.join("workspace"));
        }
    }

    if let Some(custom_workspace) = crate::util::get_runtime_var("SEN_WORKSPACE") {
        if !custom_workspace.is_empty() {
            let expanded = expand_tilde_path(&custom_workspace);
            let (sen_dir, workspace_dir) = resolve_config_dir_for_workspace(&expanded);
            return (sen_dir, workspace_dir);
        }
    }

    if let Some((sen_dir, workspace_dir)) = load_persisted_workspace_dirs_sync(default_sen_dir) {
        return (sen_dir, workspace_dir);
    }

    (
        default_sen_dir.to_path_buf(),
        default_workspace_dir.to_path_buf(),
    )
}

pub fn sniff_gateway_isolated() -> bool {
    let Ok((default_sen_dir, default_workspace_dir)) = default_config_and_workspace_dirs() else {
        return false;
    };
    let (sen_dir, _workspace_dir) =
        resolve_runtime_config_dirs_sync(&default_sen_dir, &default_workspace_dir);
    let config_path = sen_dir.join("config.toml");
    let Ok(contents) = std::fs::read_to_string(&config_path) else {
        return false;
    };

    #[derive(serde::Deserialize)]
    struct GatewaySniff {
        #[serde(default)]
        isolated: bool,
    }
    #[derive(serde::Deserialize)]
    struct RootSniff {
        #[serde(default)]
        gateway: Option<GatewaySniff>,
    }

    toml::from_str::<RootSniff>(&contents)
        .ok()
        .and_then(|root| root.gateway)
        .map(|gateway| gateway.isolated)
        .unwrap_or(false)
}

fn load_persisted_workspace_dirs_sync(default_config_dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let state_path = active_workspace_state_path(default_config_dir);
    if !state_path.exists() {
        return None;
    }

    let contents = match std::fs::read_to_string(&state_path) {
        Ok(contents) => contents,
        Err(error) => {
            tracing::warn!(
                "Failed to read active workspace marker {}: {}",
                state_path.display(),
                error
            );
            return None;
        }
    };

    match toml::from_str::<ActiveWorkspaceState>(&contents) {
        Ok(state) => {
            let expanded = expand_tilde_path(&state.config_dir);
            let (sen_dir, workspace_dir) = resolve_config_dir_for_workspace(&expanded);
            Some((sen_dir, workspace_dir))
        }
        Err(error) => {
            tracing::warn!(
                "Failed to parse active workspace state {}: {}",
                state_path.display(),
                error
            );
            None
        }
    }
}

fn decrypt_optional_secret(
    store: &crate::security::SecretStore,
    value: &mut Option<String>,
    field_name: &str,
) -> Result<()> {
    if let Some(raw) = value.clone() {
        if crate::security::SecretStore::is_encrypted(&raw) {
            match store.decrypt(&raw) {
                Ok(plain) => *value = Some(plain),
                Err(err) => {
                    tracing::warn!(
                        field = field_name,
                        error = %err,
                        "Failed to decrypt secret; clearing in-memory value so the desktop can boot. \
                         Re-enter the credential in Settings to restore."
                    );
                    *value = None;
                }
            }
        }
    }
    Ok(())
}

fn decrypt_secret(
    store: &crate::security::SecretStore,
    value: &mut String,
    field_name: &str,
) -> Result<()> {
    if crate::security::SecretStore::is_encrypted(value) {
        match store.decrypt(value) {
            Ok(plain) => *value = plain,
            Err(err) => {
                tracing::warn!(
                    field = field_name,
                    error = %err,
                    "Failed to decrypt required secret; clearing in-memory value so the desktop can boot. \
                     The associated channel/provider will require re-entering the credential."
                );
                value.clear();
            }
        }
    }
    Ok(())
}

fn encrypt_optional_secret(
    store: &crate::security::SecretStore,
    value: &mut Option<String>,
    field_name: &str,
) -> Result<()> {
    if let Some(raw) = value.clone() {
        if !crate::security::SecretStore::is_encrypted(&raw) {
            *value = Some(
                store
                    .encrypt(&raw)
                    .with_context(|| format!("Failed to encrypt {field_name}"))?,
            );
        }
    }
    Ok(())
}

fn encrypt_secret(
    store: &crate::security::SecretStore,
    value: &mut String,
    field_name: &str,
) -> Result<()> {
    if !crate::security::SecretStore::is_encrypted(value) {
        *value = store
            .encrypt(value)
            .with_context(|| format!("Failed to encrypt {field_name}"))?;
    }
    Ok(())
}

fn config_dir_creation_error(path: &Path) -> String {
    format!(
        "Failed to create config directory: {}. If running as an OpenRC service, \
         ensure this path is writable by user 'sen'.",
        path.display()
    )
}

fn is_local_ollama_endpoint(api_url: Option<&str>) -> bool {
    let Some(raw) = api_url.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };

    reqwest::Url::parse(raw)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
        .is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1" | "0.0.0.0"))
}

fn should_apply_legacy_provider(config_provider: &Option<String>) -> bool {
    config_provider.as_deref().map_or(true, |configured| {
        configured.trim().eq_ignore_ascii_case("openrouter")
    })
}

fn has_ollama_cloud_credential(config_api_key: Option<&str>) -> bool {
    let config_key_present = config_api_key
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if config_key_present {
        return true;
    }

    ["OLLAMA_API_KEY", "SEN_API_KEY", "API_KEY"]
        .iter()
        .any(|name| {
            std::env::var(name)
                .ok()
                .is_some_and(|value| !value.trim().is_empty())
        })
}

pub fn parse_extra_headers_env(raw: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if let Some((key, value)) = entry.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            if key.is_empty() {
                tracing::warn!("Ignoring extra header with empty name in SEN_EXTRA_HEADERS");
                continue;
            }
            result.push((key.to_string(), value.to_string()));
        } else {
            tracing::warn!("Ignoring malformed extra header entry (missing ':'): {entry}");
        }
    }
    result
}

fn normalize_wire_api(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "responses" | "openai-responses" | "open-ai-responses" => Some("responses"),
        "chat_completions"
        | "chat-completions"
        | "chat"
        | "chatcompletions"
        | "openai-chat-completions"
        | "open-ai-chat-completions" => Some("chat_completions"),
        _ => None,
    }
}

fn read_codex_openai_api_key() -> Option<String> {
    let home = UserDirs::new()?.home_dir().to_path_buf();
    let auth_path = home.join(".codex").join("auth.json");
    let raw = std::fs::read_to_string(auth_path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;

    parsed
        .get("OPENAI_API_KEY")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

async fn ensure_bootstrap_files(workspace_dir: &Path) -> Result<()> {
    let defaults: &[(&str, &str)] = &[
        (
            "IDENTITY.md",
            "# IDENTITY.md ??Who Am I?\n\n\
             I am SenWeaverCoding, an autonomous AI agent.\n\n\
             ## Traits\n\
             - Helpful, precise, and safety-conscious\n\
             - I prioritize clarity and correctness\n",
        ),
        (
            "SOUL.md",
            "# SOUL.md ??Who You Are\n\n\
             You are SenWeaverCoding, an autonomous AI agent.\n\n\
             ## Core Principles\n\
             - Be helpful and accurate\n\
             - Respect user intent and boundaries\n\
             - Ask before taking destructive actions\n\
             - Prefer safe, reversible operations\n",
        ),
    ];

    for (filename, content) in defaults {
        let path = workspace_dir.join(filename);
        if !path.exists() {
            fs::write(&path, content)
                .await
                .with_context(|| format!("Failed to create default {filename} in workspace"))?;
        }
    }

    Ok(())
}

impl Config {
    pub async fn load_or_init() -> Result<Self> {
        let (default_sen_dir, default_workspace_dir) =
            default_config_and_workspace_dirs().unwrap_or_else(|err| {
                tracing::warn!(
                    error = %err,
                    "default config dirs unresolved; using %TEMP% fallback so first launch can proceed"
                );
                let mut fallback = std::env::temp_dir();
                fallback.push("SenAgentOS");
                fallback.push(".senweavercoding");
                let workspace = fallback.join("workspace");
                (fallback, workspace)
            });

        let (mut sen_dir, mut workspace_dir, resolution_source) = match resolve_runtime_config_dirs(
            &default_sen_dir,
            &default_workspace_dir,
        )
        .await
        {
            Ok(triple) => triple,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "resolve_runtime_config_dirs failed; falling back to default sen_dir so first launch can proceed"
                );
                (
                    default_sen_dir.clone(),
                    default_workspace_dir.clone(),
                    ConfigResolutionSource::DefaultConfigDir,
                )
            }
        };

        if let Err(err) = fs::create_dir_all(&sen_dir).await {
            tracing::warn!(
                error = %err,
                path = %sen_dir.display(),
                "{}; falling back to %TEMP%/SenAgentOS so the desktop can boot",
                config_dir_creation_error(&sen_dir)
            );
            let mut temp_root = std::env::temp_dir();
            temp_root.push("SenAgentOS");
            temp_root.push(".senweavercoding");
            sen_dir = temp_root.clone();
            workspace_dir = temp_root.join("workspace");
            if let Err(err2) = fs::create_dir_all(&sen_dir).await {
                tracing::error!(
                    error = %err2,
                    path = %sen_dir.display(),
                    "even %TEMP% config dir creation failed; running entirely in-memory"
                );
            }
        }
        if let Err(err) = fs::create_dir_all(&workspace_dir).await {
            tracing::warn!(
                error = %err,
                path = %workspace_dir.display(),
                "Failed to create workspace directory; falling back to %TEMP%/SenAgentOS/workspace"
            );
            let mut tmp = std::env::temp_dir();
            tmp.push("SenAgentOS");
            tmp.push("workspace");
            if let Err(err2) = fs::create_dir_all(&tmp).await {
                tracing::error!(
                    error = %err2,
                    path = %tmp.display(),
                    "even %TEMP% workspace dir creation failed; entries written there will fail at runtime"
                );
            }
            workspace_dir = tmp;
        }

        let config_path = sen_dir.join("config.toml");

        if let Err(err) = ensure_bootstrap_files(&workspace_dir).await {
            tracing::warn!(
                error = %err,
                workspace = %workspace_dir.display(),
                "Failed to write bootstrap workspace files (IDENTITY.md/SOUL.md); continuing without them"
            );
        }

        if config_path.exists() {

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = fs::metadata(&config_path).await {
                    if meta.permissions().mode() & 0o004 != 0 {
                        tracing::warn!(
                            "Config file {:?} is world-readable (mode {:o}). \
                             Consider restricting with: chmod 600 {:?}",
                            config_path,
                            meta.permissions().mode() & 0o777,
                            config_path,
                        );
                    }
                }
            }

            let contents = match fs::read_to_string(&config_path).await {
                Ok(text) => text,
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        path = %config_path.display(),
                        "Failed to read config file; falling back to defaults so the desktop can boot. \
                         Use Settings page to recreate provider/model entries."
                    );
                    let mut config = Config {
                        config_path: config_path.clone(),
                        workspace_dir,
                        ..Config::default()
                    };
                    config.apply_env_overrides_tracked();
                    if let Err(verr) = config.validate() {
                        tracing::warn!(error = %verr, "Default config validation warning (non-fatal)");
                    }
                    return Ok(config);
                }
            };

            let mut config: Config = match toml::from_str(&contents) {
                Ok(cfg) => cfg,
                Err(err) => {
                    // Only rename/overwrite a config.toml that sen actually owns.
                    // If this directory is not a sen config home (e.g. a project
                    // dir with an unrelated config.toml resolution landed on),
                    // degrade read-only: boot with in-memory defaults and never
                    // touch the user's file.
                    let owned = is_sen_config_dir(&sen_dir);
                    let backup_suffix = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let backup_path =
                        config_path.with_extension(format!("toml.bak-{backup_suffix}"));
                    let backup_ok = if !owned {
                        tracing::warn!(
                            path = %config_path.display(),
                            "config.toml failed to parse and this directory is not a sen config \
                             home; booting with in-memory defaults without modifying the file"
                        );
                        false
                    } else {
                        match fs::rename(&config_path, &backup_path).await {
                            Ok(()) => true,
                            Err(rename_err) => {
                                tracing::warn!(
                                    error = %rename_err,
                                    backup = %backup_path.display(),
                                    "could not back up corrupted config; leaving original in place"
                                );
                                false
                            }
                        }
                    };
                    tracing::warn!(
                        error = %err,
                        path = %config_path.display(),
                        backup_created = backup_ok,
                        "Failed to deserialize config file. \
                         Booting with defaults so the desktop can render Settings."
                    );
                    let mut config = Config {
                        config_path: config_path.clone(),
                        workspace_dir: workspace_dir.clone(),
                        ..Config::default()
                    };
                    if backup_ok {
                        if let Err(save_err) = config.save().await {
                            tracing::warn!(
                                error = %save_err,
                                "Could not persist replacement default config; running with in-memory defaults"
                            );
                        }
                    } else {
                        tracing::warn!(
                            "corrupted config could not be backed up; refusing to overwrite it with defaults \
                             (fix or remove the file manually to persist settings again)"
                        );
                    }
                    config.apply_env_overrides_tracked();
                    if let Err(verr) = config.validate() {
                        tracing::warn!(error = %verr, "Default config validation warning (non-fatal)");
                    }
                    return Ok(config);
                }
            };

            config.autonomy.ensure_default_auto_approve();
            config.cost.merge_default_prices();

            let migration_applied = config.migrate_legacy_low_caps();

            if let Ok(de) = toml::de::Deserializer::parse(&contents) {
                const RETIRED_KEYS: &[&str] = &["node_transport"];
                let mut ignored_keys: Vec<String> = Vec::new();
                let _: Result<Config, _> =
                    serde_ignored::deserialize(de, |path| ignored_keys.push(path.to_string()));
                for key in ignored_keys {
                    if RETIRED_KEYS
                        .iter()
                        .any(|r| key == *r || key.starts_with(&format!("{r}.")))
                    {
                        tracing::debug!("Retired config key ignored: \"{key}\"");
                        continue;
                    }
                    tracing::warn!(
                        "Unknown config key ignored: \"{key}\". Check config.toml for typos or deprecated options.",
                    );
                }
            }

            config.config_path = config_path.clone();
            config.workspace_dir = workspace_dir;
            let store = crate::security::SecretStore::new(&sen_dir, config.secrets.encrypt);
            decrypt_optional_secret(&store, &mut config.api_key, "config.api_key")?;
            decrypt_optional_secret(
                &store,
                &mut config.composio.api_key,
                "config.composio.api_key",
            )?;
            if let Some(ref mut pinggy) = config.tunnel.pinggy {
                decrypt_optional_secret(&store, &mut pinggy.token, "config.tunnel.pinggy.token")?;
            }
            decrypt_optional_secret(
                &store,
                &mut config.microsoft365.client_secret,
                "config.microsoft365.client_secret",
            )?;

            decrypt_optional_secret(
                &store,
                &mut config.web_search.brave_api_key,
                "config.web_search.brave_api_key",
            )?;

            decrypt_optional_secret(
                &store,
                &mut config.storage.provider.config.db_url,
                "config.storage.provider.config.db_url",
            )?;

            for agent in config.agents.values_mut() {
                decrypt_optional_secret(&store, &mut agent.api_key, "config.agents.*.api_key")?;
            }

            for provider in config.model_providers.values_mut() {
                decrypt_optional_secret(
                    &store,
                    &mut provider.api_key,
                    "config.model_providers.*.api_key",
                )?;
            }

            if let Some(ref mut openai) = config.tts.openai {
                decrypt_optional_secret(&store, &mut openai.api_key, "config.tts.openai.api_key")?;
            }
            if let Some(ref mut elevenlabs) = config.tts.elevenlabs {
                decrypt_optional_secret(
                    &store,
                    &mut elevenlabs.api_key,
                    "config.tts.elevenlabs.api_key",
                )?;
            }
            if let Some(ref mut google) = config.tts.google {
                decrypt_optional_secret(&store, &mut google.api_key, "config.tts.google.api_key")?;
            }

            decrypt_optional_secret(
                &store,
                &mut config.transcription.api_key,
                "config.transcription.api_key",
            )?;
            if let Some(ref mut openai) = config.transcription.openai {
                decrypt_optional_secret(
                    &store,
                    &mut openai.api_key,
                    "config.transcription.openai.api_key",
                )?;
            }
            if let Some(ref mut deepgram) = config.transcription.deepgram {
                decrypt_optional_secret(
                    &store,
                    &mut deepgram.api_key,
                    "config.transcription.deepgram.api_key",
                )?;
            }
            if let Some(ref mut assemblyai) = config.transcription.assemblyai {
                decrypt_optional_secret(
                    &store,
                    &mut assemblyai.api_key,
                    "config.transcription.assemblyai.api_key",
                )?;
            }
            if let Some(ref mut google) = config.transcription.google {
                decrypt_optional_secret(
                    &store,
                    &mut google.api_key,
                    "config.transcription.google.api_key",
                )?;
            }
            if let Some(ref mut local) = config.transcription.local_whisper {
                decrypt_optional_secret(
                    &store,
                    &mut local.bearer_token,
                    "config.transcription.local_whisper.bearer_token",
                )?;
            }

            #[cfg(feature = "channel-nostr")]
            if let Some(ref mut ns) = config.channels_config.nostr {
                decrypt_secret(
                    &store,
                    &mut ns.private_key,
                    "config.channels_config.nostr.private_key",
                )?;
            }
            if let Some(ref mut fs) = config.channels_config.feishu {
                decrypt_secret(
                    &store,
                    &mut fs.app_secret,
                    "config.channels_config.feishu.app_secret",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut fs.encrypt_key,
                    "config.channels_config.feishu.encrypt_key",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut fs.verification_token,
                    "config.channels_config.feishu.verification_token",
                )?;
            }

            if let Some(ref mut tg) = config.channels_config.telegram {
                decrypt_secret(
                    &store,
                    &mut tg.bot_token,
                    "config.channels_config.telegram.bot_token",
                )?;
            }
            if let Some(ref mut dc) = config.channels_config.discord {
                decrypt_secret(
                    &store,
                    &mut dc.bot_token,
                    "config.channels_config.discord.bot_token",
                )?;
            }
            if let Some(ref mut sl) = config.channels_config.slack {
                decrypt_secret(
                    &store,
                    &mut sl.bot_token,
                    "config.channels_config.slack.bot_token",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut sl.app_token,
                    "config.channels_config.slack.app_token",
                )?;
            }
            if let Some(ref mut mm) = config.channels_config.mattermost {
                decrypt_secret(
                    &store,
                    &mut mm.bot_token,
                    "config.channels_config.mattermost.bot_token",
                )?;
            }
            if let Some(ref mut mx) = config.channels_config.matrix {
                decrypt_secret(
                    &store,
                    &mut mx.access_token,
                    "config.channels_config.matrix.access_token",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut mx.recovery_key,
                    "config.channels_config.matrix.recovery_key",
                )?;
            }
            if let Some(ref mut wa) = config.channels_config.whatsapp {
                decrypt_optional_secret(
                    &store,
                    &mut wa.access_token,
                    "config.channels_config.whatsapp.access_token",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut wa.app_secret,
                    "config.channels_config.whatsapp.app_secret",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut wa.verify_token,
                    "config.channels_config.whatsapp.verify_token",
                )?;
            }
            if let Some(ref mut lq) = config.channels_config.linq {
                decrypt_secret(
                    &store,
                    &mut lq.api_token,
                    "config.channels_config.linq.api_token",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut lq.signing_secret,
                    "config.channels_config.linq.signing_secret",
                )?;
            }
            if let Some(ref mut wt) = config.channels_config.wati {
                decrypt_secret(
                    &store,
                    &mut wt.api_token,
                    "config.channels_config.wati.api_token",
                )?;
            }
            if let Some(ref mut nc) = config.channels_config.nextcloud_talk {
                decrypt_secret(
                    &store,
                    &mut nc.app_token,
                    "config.channels_config.nextcloud_talk.app_token",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut nc.webhook_secret,
                    "config.channels_config.nextcloud_talk.webhook_secret",
                )?;
            }
            if let Some(ref mut em) = config.channels_config.email {
                decrypt_secret(
                    &store,
                    &mut em.password,
                    "config.channels_config.email.password",
                )?;
            }
            if let Some(ref mut gp) = config.channels_config.gmail_push {
                decrypt_secret(
                    &store,
                    &mut gp.oauth_token,
                    "config.channels_config.gmail_push.oauth_token",
                )?;
            }
            if let Some(ref mut irc) = config.channels_config.irc {
                decrypt_optional_secret(
                    &store,
                    &mut irc.server_password,
                    "config.channels_config.irc.server_password",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut irc.nickserv_password,
                    "config.channels_config.irc.nickserv_password",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut irc.sasl_password,
                    "config.channels_config.irc.sasl_password",
                )?;
            }
            if let Some(ref mut lk) = config.channels_config.lark {
                decrypt_secret(
                    &store,
                    &mut lk.app_secret,
                    "config.channels_config.lark.app_secret",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut lk.encrypt_key,
                    "config.channels_config.lark.encrypt_key",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut lk.verification_token,
                    "config.channels_config.lark.verification_token",
                )?;
            }
            if let Some(ref mut fs) = config.channels_config.feishu {
                decrypt_secret(
                    &store,
                    &mut fs.app_secret,
                    "config.channels_config.feishu.app_secret",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut fs.encrypt_key,
                    "config.channels_config.feishu.encrypt_key",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut fs.verification_token,
                    "config.channels_config.feishu.verification_token",
                )?;
            }
            if let Some(ref mut dt) = config.channels_config.dingtalk {
                decrypt_secret(
                    &store,
                    &mut dt.client_secret,
                    "config.channels_config.dingtalk.client_secret",
                )?;
            }
            if let Some(ref mut wc) = config.channels_config.wecom {
                decrypt_secret(
                    &store,
                    &mut wc.webhook_key,
                    "config.channels_config.wecom.webhook_key",
                )?;
            }
            if let Some(ref mut qq) = config.channels_config.qq {
                decrypt_secret(
                    &store,
                    &mut qq.app_secret,
                    "config.channels_config.qq.app_secret",
                )?;
            }
            if let Some(ref mut wh) = config.channels_config.webhook {
                decrypt_optional_secret(
                    &store,
                    &mut wh.secret,
                    "config.channels_config.webhook.secret",
                )?;
            }
            if let Some(ref mut ct) = config.channels_config.telnyx {
                decrypt_secret(
                    &store,
                    &mut ct.api_key,
                    "config.channels_config.telnyx.api_key",
                )?;
                decrypt_optional_secret(
                    &store,
                    &mut ct.webhook_secret,
                    "config.channels_config.telnyx.webhook_secret",
                )?;
            }

            for token in &mut config.gateway.paired_tokens {
                decrypt_secret(&store, token, "config.gateway.paired_tokens[]")?;
            }

            decrypt_optional_secret(
                &store,
                &mut config.security.nevis.client_secret,
                "config.security.nevis.client_secret",
            )?;

            if !config.notion.api_key.is_empty() {
                decrypt_secret(&store, &mut config.notion.api_key, "config.notion.api_key")?;
            }

            if !config.jira.api_token.is_empty() {
                decrypt_secret(&store, &mut config.jira.api_token, "config.jira.api_token")?;
            }

            config.apply_env_overrides_tracked();
            if let Err(err) = config.validate() {
                tracing::warn!(
                    error = %err,
                    "config validation surfaced an issue; continuing with loaded config so the desktop can boot. \
                     Open Settings to fix any field highlighted in the warning."
                );
            }

            if migration_applied {
                if let Err(err) = config.save().await {
                    tracing::warn!(
                        target: "config.migration",
                        error = %err,
                        "Failed to persist migrated config; values are lifted in-memory only and the migration will retry on next load."
                    );
                }
            }

            tracing::info!(
                path = %config.config_path.display(),
                workspace = %config.workspace_dir.display(),
                source = resolution_source.as_str(),
                initialized = true,
                "Config loaded"
            );
            Ok(config)
        } else {
            let mut config = Config {
                config_path: config_path.clone(),
                workspace_dir,
                ..Config::default()
            };
            if let Err(err) = config.save().await {
                tracing::warn!(
                    error = %err,
                    path = %config_path.display(),
                    "Could not write initial config file; running with in-memory defaults so the desktop can still launch"
                );
            }

            #[cfg(unix)]
            {
                use std::{fs::Permissions, os::unix::fs::PermissionsExt};
                let _ = fs::set_permissions(&config_path, Permissions::from_mode(0o600)).await;
            }

            config.apply_env_overrides_tracked();
            if let Err(err) = config.validate() {
                tracing::warn!(
                    error = %err,
                    "default config validation surfaced an issue (non-fatal); desktop will still launch"
                );
            }
            tracing::info!(
                path = %config.config_path.display(),
                workspace = %config.workspace_dir.display(),
                source = resolution_source.as_str(),
                initialized = true,
                "Config loaded"
            );
            Ok(config)
        }
    }

    pub fn load_or_init_sync() -> Self {
        let Ok((default_sen_dir, default_workspace_dir)) = default_config_and_workspace_dirs()
        else {
            return Config::default();
        };

        let (sen_dir, workspace_dir) =
            resolve_runtime_config_dirs_sync(&default_sen_dir, &default_workspace_dir);

        let config_path = sen_dir.join("config.toml");

        if std::fs::create_dir_all(&sen_dir).is_err() {
            tracing::warn!("Failed to create config directory: {}", sen_dir.display());
        }
        if std::fs::create_dir_all(&workspace_dir).is_err() {
            tracing::warn!(
                "Failed to create workspace directory: {}",
                workspace_dir.display()
            );
        }

        let mut config = if config_path.exists() {
            match std::fs::read_to_string(&config_path) {
                Ok(contents) => match toml::from_str::<Config>(&contents) {
                    Ok(mut c) => {
                        c.config_path = config_path.clone();
                        c.workspace_dir = workspace_dir.clone();
                        c
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse config at {}, using defaults: {}",
                            config_path.display(),
                            e
                        );
                        Config::default()
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        "Failed to read config at {}, using defaults: {}",
                        config_path.display(),
                        e
                    );
                    Config::default()
                }
            }
        } else {
            Config::default()
        };

        config.config_path = config_path;
        config.workspace_dir = workspace_dir;

        config.cost.merge_default_prices();
        config.migrate_legacy_low_caps();

        config.apply_env_overrides_tracked();
        if let Err(e) = config.validate() {
            tracing::warn!("Config validation failed: {}", e);
        }

        tracing::info!(
            "Config loaded (sync path) config_path={}",
            config.config_path.display()
        );

        config
    }

    fn lookup_model_provider_profile(
        &self,
        provider_name: &str,
    ) -> Option<(String, ModelProviderConfig)> {
        let needle = provider_name.trim();
        if needle.is_empty() {
            return None;
        }

        self.model_providers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(needle))
            .map(|(name, profile)| (name.clone(), profile.clone()))
    }

    pub fn model_vision_capability(&self, provider_name: &str, model: &str) -> Option<bool> {
        let needle = provider_name.trim();
        if needle.is_empty() {
            return None;
        }
        self.model_providers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(needle))
            .and_then(|(_, profile)| profile.explicit_vision_for_model(model))
    }

    fn apply_named_model_provider_profile(&mut self) {
        let Some(current_provider) = self.default_provider.clone() else {
            return;
        };

        let Some((_profile_key, profile)) = self.lookup_model_provider_profile(&current_provider)
        else {
            return;
        };

        let base_url = profile
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);

        if self
            .api_url
            .as_deref()
            .map(str::trim)
            .is_none_or(|value| value.is_empty())
        {
            if let Some(base_url) = base_url.as_ref() {
                self.api_url = Some(base_url.clone());
            }
        }

        if self.api_path.is_none() {
            if let Some(ref path) = profile.api_path {
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    self.api_path = Some(trimmed.to_string());
                }
            }
        }

        if self.provider_max_tokens.is_none() {
            if let Some(max_tokens) = profile.max_tokens {
                self.provider_max_tokens = Some(max_tokens);
            }
        }

        if profile.requires_openai_auth
            && self
                .api_key
                .as_deref()
                .map(str::trim)
                .is_none_or(|value| value.is_empty())
        {
            let codex_key = std::env::var("OPENAI_API_KEY")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .or_else(read_codex_openai_api_key);
            if let Some(codex_key) = codex_key {
                self.api_key = Some(codex_key);
            }
        }

        if self
            .api_key
            .as_deref()
            .map(str::trim)
            .is_none_or(|value| value.is_empty())
        {
            if let Some(profile_key_value) = profile
                .api_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                self.api_key = Some(profile_key_value.to_string());
            }
        }

    }

    fn migrate_legacy_low_caps(&mut self) -> bool {
        let mut changed = false;
        if self.autonomy.max_actions_per_hour > 0 && self.autonomy.max_actions_per_hour <= 100 {
            tracing::warn!(
                target: "config.migration",
                stale = self.autonomy.max_actions_per_hour,
                "autonomy.max_actions_per_hour was set to a legacy low value; resetting to 0 (disabled). \
                 Set a value > 100 explicitly if you want a hard per-hour cap."
            );
            self.autonomy.max_actions_per_hour = 0;
            changed = true;
        }

        if self.agent.max_tool_iterations < 500 {
            let new_value = default_agent_max_tool_iterations();
            tracing::warn!(
                target: "config.migration",
                stale = self.agent.max_tool_iterations,
                new = new_value,
                "agent.max_tool_iterations was set to a legacy low value; lifting to current default. \
                 Set a value >= 500 explicitly if you really want a tight cap."
            );
            self.agent.max_tool_iterations = new_value;
            changed = true;
        }

        if self.agent_runtime.max_tool_iterations < 500 {
            let new_value = self.agent.max_tool_iterations.min(u32::MAX as usize) as u32;
            tracing::warn!(
                target: "config.migration",
                stale = self.agent_runtime.max_tool_iterations,
                new = new_value,
                "agent_runtime.max_tool_iterations was set to a legacy low value; lifting to current default."
            );
            self.agent_runtime.max_tool_iterations = new_value;
            changed = true;
        }
        changed
    }

    pub fn validate(&self) -> Result<()> {

        let aggregate = crate::config::validator::ConfigValidator::new(self).run();
        aggregate.log_summary();
        if !aggregate.is_ok() {
            return aggregate.into_result();
        }

        if self.tunnel.provider.trim() == "openvpn" {
            let openvpn = self.tunnel.openvpn.as_ref().ok_or_else(|| {
                anyhow::anyhow!("tunnel.provider='openvpn' requires [tunnel.openvpn]")
            })?;

            if openvpn.config_file.trim().is_empty() {
                anyhow::bail!("tunnel.openvpn.config_file must not be empty");
            }
            if openvpn.connect_timeout_secs == 0 {
                anyhow::bail!("tunnel.openvpn.connect_timeout_secs must be greater than 0");
            }
        }

        if self.gateway.host.trim().is_empty() {
            anyhow::bail!("gateway.host must not be empty");
        }
        if let Some(ref prefix) = self.gateway.path_prefix {

            if !prefix.is_empty() {
                if !prefix.starts_with('/') {
                    anyhow::bail!("gateway.path_prefix must start with '/'");
                }
                if prefix.ends_with('/') {
                    anyhow::bail!("gateway.path_prefix must not end with '/' (including bare '/')");
                }

                if let Some(bad) = prefix.chars().find(|c| {
                    !matches!(c, '/' | '-' | '_' | '.' | '~'
                        | 'a'..='z' | 'A'..='Z' | '0'..='9'
                        | '!' | '$' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' | ';' | '='
                        | ':' | '@')
                }) {
                    anyhow::bail!(
                        "gateway.path_prefix contains invalid character '{bad}'; \
                         only unreserved and sub-delim URI characters are allowed"
                    );
                }
            }
        }

        for (i, env_name) in self.autonomy.shell_env_passthrough.iter().enumerate() {
            if !is_valid_env_var_name(env_name) {
                anyhow::bail!(
                    "autonomy.shell_env_passthrough[{i}] is invalid ({env_name}); expected [A-Za-z_][A-Za-z0-9_]*"
                );
            }
        }

        if self.security.otp.challenge_max_attempts == 0 {
            anyhow::bail!("security.otp.challenge_max_attempts must be greater than 0");
        }
        if self.security.otp.token_ttl_secs == 0 {
            anyhow::bail!("security.otp.token_ttl_secs must be greater than 0");
        }
        if self.security.otp.cache_valid_secs == 0 {
            anyhow::bail!("security.otp.cache_valid_secs must be greater than 0");
        }
        if self.security.otp.cache_valid_secs < self.security.otp.token_ttl_secs {
            anyhow::bail!(
                "security.otp.cache_valid_secs must be greater than or equal to security.otp.token_ttl_secs"
            );
        }
        if self.security.otp.challenge_max_attempts == 0 {
            anyhow::bail!("security.otp.challenge_max_attempts must be greater than 0");
        }
        for (i, action) in self.security.otp.gated_actions.iter().enumerate() {
            let normalized = action.trim();
            if normalized.is_empty() {
                anyhow::bail!("security.otp.gated_actions[{i}] must not be empty");
            }
            if !normalized
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                anyhow::bail!(
                    "security.otp.gated_actions[{i}] contains invalid characters: {normalized}"
                );
            }
        }
        DomainMatcher::new(
            &self.security.otp.gated_domains,
            &self.security.otp.gated_domain_categories,
        )
        .with_context(
            || "Invalid security.otp.gated_domains or security.otp.gated_domain_categories",
        )?;
        if self.security.estop.state_file.trim().is_empty() {
            anyhow::bail!("security.estop.state_file must not be empty");
        }

        if self.scheduler.max_concurrent == 0 {
            anyhow::bail!("scheduler.max_concurrent must be greater than 0");
        }
        if self.scheduler.max_tasks == 0 {
            anyhow::bail!("scheduler.max_tasks must be greater than 0");
        }

        for (i, route) in self.model_routes.iter().enumerate() {
            if route.hint.trim().is_empty() {
                anyhow::bail!("model_routes[{i}].hint must not be empty");
            }
            if route.provider.trim().is_empty() {
                anyhow::bail!("model_routes[{i}].provider must not be empty");
            }
            if route.model.trim().is_empty() {
                anyhow::bail!("model_routes[{i}].model must not be empty");
            }
        }

        for (profile_key, profile) in &self.model_providers {
            let profile_name = profile_key.trim();
            if profile_name.is_empty() {
                anyhow::bail!("model_providers contains an empty profile name");
            }

            let has_name = profile
                .name
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty());
            let has_base_url = profile
                .base_url
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty());

            if !has_name && !has_base_url {
                anyhow::bail!(
                    "model_providers.{profile_name} must define at least one of `name` or `base_url`"
                );
            }

            if let Some(base_url) = profile.base_url.as_deref().map(str::trim) {
                if !base_url.is_empty() {
                    let parsed = reqwest::Url::parse(base_url).with_context(|| {
                        format!("model_providers.{profile_name}.base_url is not a valid URL")
                    })?;
                    if !matches!(parsed.scheme(), "http" | "https") {
                        anyhow::bail!(
                            "model_providers.{profile_name}.base_url must use http/https"
                        );
                    }
                }
            }

            if let Some(wire_api) = profile.wire_api.as_deref().map(str::trim) {
                if !wire_api.is_empty() && normalize_wire_api(wire_api).is_none() {
                    anyhow::bail!(
                        "model_providers.{profile_name}.wire_api must be one of: responses, chat_completions"
                    );
                }
            }

            for header in &profile.custom_headers {
                let trimmed = header.name.trim();
                if trimmed.is_empty() {
                    tracing::warn!(
                        provider = profile_name,
                        "model_providers.{profile_name}.custom_headers contains entry with empty name; will be ignored at runtime"
                    );
                    continue;
                }
                if !is_valid_http_header_name(trimmed) {
                    tracing::warn!(
                        provider = profile_name,
                        header_name = trimmed,
                        "model_providers.{profile_name}.custom_headers: name contains characters outside RFC 7230 token; entry will be skipped at runtime"
                    );
                }
                if is_disallowed_custom_header(trimmed) {
                    tracing::warn!(
                        provider = profile_name,
                        header_name = trimmed,
                        "model_providers.{profile_name}.custom_headers: '{trimmed}' is managed by the provider itself and will be skipped at runtime"
                    );
                }
                if !is_valid_http_header_value(&header.value) {
                    tracing::warn!(
                        provider = profile_name,
                        header_name = trimmed,
                        "model_providers.{profile_name}.custom_headers: value contains CR/LF for '{trimmed}'; entry will be skipped at runtime"
                    );
                }
            }
        }

        if self
            .default_provider
            .as_deref()
            .is_some_and(|provider| provider.trim().eq_ignore_ascii_case("ollama"))
            && self
                .default_model
                .as_deref()
                .is_some_and(|model| model.trim().ends_with(":cloud"))
        {
            if is_local_ollama_endpoint(self.api_url.as_deref()) {
                anyhow::bail!(
                    "default_model uses ':cloud' with provider 'ollama', but api_url is local or unset. Set api_url to a remote Ollama endpoint (for example https://ollama.com)."
                );
            }

            if !has_ollama_cloud_credential(self.api_key.as_deref()) {
                anyhow::bail!(
                    "default_model uses ':cloud' with provider 'ollama', but no API key is configured. Set api_key or OLLAMA_API_KEY."
                );
            }
        }

        if self.microsoft365.enabled {
            let tenant = self
                .microsoft365
                .tenant_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if tenant.is_none() {
                anyhow::bail!(
                    "microsoft365.tenant_id must not be empty when microsoft365 is enabled"
                );
            }
            let client = self
                .microsoft365
                .client_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if client.is_none() {
                anyhow::bail!(
                    "microsoft365.client_id must not be empty when microsoft365 is enabled"
                );
            }
            let flow = self.microsoft365.auth_flow.trim();
            if flow != "client_credentials" && flow != "device_code" {
                anyhow::bail!(
                    "microsoft365.auth_flow must be 'client_credentials' or 'device_code'"
                );
            }
            if flow == "client_credentials"
                && self
                    .microsoft365
                    .client_secret
                    .as_deref()
                    .map_or(true, |s| s.trim().is_empty())
            {
                anyhow::bail!(
                    "microsoft365.client_secret must not be empty when auth_flow is 'client_credentials'"
                );
            }
        }

        {
            let kind = self.runtime.kind.trim();
            match kind {
                "native" | "docker" => {}
                "wasm" | "cloudflare" => {
                    tracing::warn!(
                        "runtime.kind='{kind}' is experimental and not fully supported yet"
                    );
                }
                other => {
                    anyhow::bail!("runtime.kind must be one of: native, docker (got '{other}')");
                }
            }
        }

        {
            let known = [
                "none",
                "noop",
                "log",
                "verbose",
                "prometheus",
                "otel",
                "opentelemetry",
                "otlp",
            ];
            for token in self.observability.backend.split(',').map(str::trim) {
                if token.is_empty() {
                    continue;
                }
                if !known.contains(&token) {
                    anyhow::bail!(
                        "observability.backend contains unknown backend '{token}'; \
                         known backends: {}",
                        known.join(", ")
                    );
                }
            }
        }

        if self.peripherals.enabled {
            for (i, board_cfg) in self.peripherals.boards.iter().enumerate() {
                if board_cfg.board.trim().is_empty() {
                    anyhow::bail!("peripherals.boards[{i}].board must not be empty");
                }
                let transport = board_cfg.transport.trim();
                if !matches!(transport, "serial" | "native" | "websocket") {
                    anyhow::bail!(
                        "peripherals.boards[{i}].transport must be one of: serial, native, websocket (got '{transport}')"
                    );
                }
                if transport == "serial"
                    && board_cfg
                        .path
                        .as_deref()
                        .map_or(true, |p| p.trim().is_empty())
                {
                    anyhow::bail!(
                        "peripherals.boards[{i}].path must not be empty when transport is 'serial'"
                    );
                }
            }
        }

        if self.mcp.enabled {
            validate_mcp_config(&self.mcp)?;
        }

        if self.knowledge.enabled {
            if self.knowledge.max_nodes == 0 {
                anyhow::bail!("knowledge.max_nodes must be greater than 0");
            }
            if self.knowledge.db_path.trim().is_empty() {
                anyhow::bail!("knowledge.db_path must not be empty");
            }
        }

        let mut seen_gws_services = std::collections::HashSet::new();
        for (i, service) in self.google_workspace.allowed_services.iter().enumerate() {
            let normalized = service.trim();
            if normalized.is_empty() {
                anyhow::bail!("google_workspace.allowed_services[{i}] must not be empty");
            }
            if !normalized
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
            {
                anyhow::bail!(
                    "google_workspace.allowed_services[{i}] contains invalid characters: {normalized}"
                );
            }
            if !seen_gws_services.insert(normalized.to_string()) {
                anyhow::bail!(
                    "google_workspace.allowed_services contains duplicate entry: {normalized}"
                );
            }
        }

        let effective_services: std::collections::HashSet<&str> =
            if self.google_workspace.allowed_services.is_empty() {
                DEFAULT_GWS_SERVICES.iter().copied().collect()
            } else {
                self.google_workspace
                    .allowed_services
                    .iter()
                    .map(|s| s.trim())
                    .collect()
            };

        let mut seen_gws_operations = std::collections::HashSet::new();
        for (i, operation) in self.google_workspace.allowed_operations.iter().enumerate() {
            let service = operation.service.trim();
            let resource = operation.resource.trim();

            if service.is_empty() {
                anyhow::bail!("google_workspace.allowed_operations[{i}].service must not be empty");
            }
            if resource.is_empty() {
                anyhow::bail!(
                    "google_workspace.allowed_operations[{i}].resource must not be empty"
                );
            }

            if !effective_services.contains(service) {
                anyhow::bail!(
                    "google_workspace.allowed_operations[{i}].service '{service}' is not in the \
                     effective allowed_services; this entry can never match at runtime"
                );
            }
            if !service
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
            {
                anyhow::bail!(
                    "google_workspace.allowed_operations[{i}].service contains invalid characters: {service}"
                );
            }
            if !resource
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
            {
                anyhow::bail!(
                    "google_workspace.allowed_operations[{i}].resource contains invalid characters: {resource}"
                );
            }

            if let Some(ref sub_resource) = operation.sub_resource {
                let sub = sub_resource.trim();
                if sub.is_empty() {
                    anyhow::bail!(
                        "google_workspace.allowed_operations[{i}].sub_resource must not be empty when present"
                    );
                }
                if !sub
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
                {
                    anyhow::bail!(
                        "google_workspace.allowed_operations[{i}].sub_resource contains invalid characters: {sub}"
                    );
                }
            }

            if operation.methods.is_empty() {
                anyhow::bail!("google_workspace.allowed_operations[{i}].methods must not be empty");
            }

            let mut seen_methods = std::collections::HashSet::new();
            for (j, method) in operation.methods.iter().enumerate() {
                let normalized = method.trim();
                if normalized.is_empty() {
                    anyhow::bail!(
                        "google_workspace.allowed_operations[{i}].methods[{j}] must not be empty"
                    );
                }
                if !normalized
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
                {
                    anyhow::bail!(
                        "google_workspace.allowed_operations[{i}].methods[{j}] contains invalid characters: {normalized}"
                    );
                }
                if !seen_methods.insert(normalized.to_string()) {
                    anyhow::bail!(
                        "google_workspace.allowed_operations[{i}].methods contains duplicate entry: {normalized}"
                    );
                }
            }

            let sub_key = operation
                .sub_resource
                .as_deref()
                .map(str::trim)
                .unwrap_or("");
            let operation_key = format!("{service}:{resource}:{sub_key}");
            if !seen_gws_operations.insert(operation_key.clone()) {
                anyhow::bail!(
                    "google_workspace.allowed_operations contains duplicate service/resource/sub_resource entry: {operation_key}"
                );
            }
        }

        if self.project_intel.enabled {
            let lang = &self.project_intel.default_language;
            if !["en", "de", "fr", "it"].contains(&lang.as_str()) {
                anyhow::bail!(
                    "project_intel.default_language must be one of: en, de, fr, it (got '{lang}')"
                );
            }
            let sens = &self.project_intel.risk_sensitivity;
            if !["low", "medium", "high"].contains(&sens.as_str()) {
                anyhow::bail!(
                    "project_intel.risk_sensitivity must be one of: low, medium, high (got '{sens}')"
                );
            }
            if let Some(ref tpl_dir) = self.project_intel.templates_dir {
                let path = std::path::Path::new(tpl_dir);
                if !path.exists() {
                    anyhow::bail!("project_intel.templates_dir path does not exist: {tpl_dir}");
                }
            }
        }

        self.cloud_ops.validate()?;

        if self.notion.enabled {
            if self.notion.database_id.trim().is_empty() {
                anyhow::bail!("notion.database_id must not be empty when notion.enabled = true");
            }
            if self.notion.poll_interval_secs == 0 {
                anyhow::bail!("notion.poll_interval_secs must be greater than 0");
            }
            if self.notion.max_concurrent == 0 {
                anyhow::bail!("notion.max_concurrent must be greater than 0");
            }
            if self.notion.status_property.trim().is_empty() {
                anyhow::bail!("notion.status_property must not be empty");
            }
            if self.notion.input_property.trim().is_empty() {
                anyhow::bail!("notion.input_property must not be empty");
            }
            if self.notion.result_property.trim().is_empty() {
                anyhow::bail!("notion.result_property must not be empty");
            }
        }

        if let Some(ref pinggy) = self.tunnel.pinggy {
            if let Some(ref region) = pinggy.region {
                let r = region.trim().to_ascii_lowercase();
                if !r.is_empty() && !matches!(r.as_str(), "us" | "eu" | "ap" | "br" | "au") {
                    anyhow::bail!(
                        "tunnel.pinggy.region must be one of: us, eu, ap, br, au (or omitted for auto)"
                    );
                }
            }
        }

        if self.jira.enabled {
            if self.jira.base_url.trim().is_empty() {
                anyhow::bail!("jira.base_url must not be empty when jira.enabled = true");
            }
            if self.jira.email.trim().is_empty() {
                anyhow::bail!("jira.email must not be empty when jira.enabled = true");
            }
            if self.jira.api_token.trim().is_empty()
                && std::env::var("JIRA_API_TOKEN")
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
            {
                anyhow::bail!(
                    "jira.api_token must be set (or JIRA_API_TOKEN env var) when jira.enabled = true"
                );
            }
            let valid_actions = ["get_ticket", "search_tickets", "comment_ticket"];
            for action in &self.jira.allowed_actions {
                if !valid_actions.contains(&action.as_str()) {
                    anyhow::bail!(
                        "jira.allowed_actions contains unknown action: '{}'. \
                         Valid: get_ticket, search_tickets, comment_ticket",
                        action
                    );
                }
            }
        }

        if let Err(msg) = self.security.nevis.validate() {
            anyhow::bail!("security.nevis: {msg}");
        }

        const MAX_DELEGATE_TIMEOUT_SECS: u64 = 3600;
        for (name, agent) in &self.agents {
            if let Some(timeout) = agent.timeout_secs {
                if timeout == 0 {
                    anyhow::bail!("agents.{name}.timeout_secs must be greater than 0");
                }
                if timeout > MAX_DELEGATE_TIMEOUT_SECS {
                    anyhow::bail!(
                        "agents.{name}.timeout_secs exceeds max {MAX_DELEGATE_TIMEOUT_SECS}"
                    );
                }
            }
            if let Some(timeout) = agent.agentic_timeout_secs {
                if timeout == 0 {
                    anyhow::bail!("agents.{name}.agentic_timeout_secs must be greater than 0");
                }
                if timeout > MAX_DELEGATE_TIMEOUT_SECS {
                    anyhow::bail!(
                        "agents.{name}.agentic_timeout_secs exceeds max {MAX_DELEGATE_TIMEOUT_SECS}"
                    );
                }
            }
        }

        {
            let dp = self.transcription.default_provider.trim();
            match dp {
                "groq" | "openai" | "deepgram" | "assemblyai" | "google" | "local_whisper" => {}
                other => {
                    anyhow::bail!(
                        "transcription.default_provider must be one of: groq, openai, deepgram, assemblyai, google, local_whisper (got '{other}')"
                    );
                }
            }
        }

        if self.delegate.timeout_secs == 0 {
            anyhow::bail!("delegate.timeout_secs must be greater than 0");
        }
        if self.delegate.agentic_timeout_secs == 0 {
            anyhow::bail!("delegate.agentic_timeout_secs must be greater than 0");
        }

        for (name, agent) in &self.agents {
            if let Some(t) = agent.timeout_secs {
                if t == 0 {
                    anyhow::bail!("agents.{name}.timeout_secs must be greater than 0");
                }
            }
            if let Some(t) = agent.agentic_timeout_secs {
                if t == 0 {
                    anyhow::bail!("agents.{name}.agentic_timeout_secs must be greater than 0");
                }
            }
        }

        Ok(())
    }

    pub fn apply_env_overrides_tracked(&mut self) {
        let pristine = match toml::Value::try_from(&*self) {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "failed to capture pre-env config baseline; env overrides may be persisted by save()"
                );
                self.apply_env_overrides();
                return;
            }
        };
        self.apply_env_overrides();
        match toml::Value::try_from(&*self) {
            Ok(env_applied) => {
                self.env_override_baseline = Some(std::sync::Arc::new(EnvOverrideBaseline {
                    pristine,
                    env_applied,
                }));
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "failed to capture post-env config baseline; env overrides may be persisted by save()"
                );
            }
        }
    }

    pub fn apply_env_overrides(&mut self) {

        if self.default_provider.is_none() {
            if let Ok(provider) = std::env::var("SEN_PROVIDER") {
                if !provider.is_empty() {
                    self.default_provider = Some(provider);
                }
            } else if let Ok(provider) =
                std::env::var("SEN_MODEL_PROVIDER").or_else(|_| std::env::var("MODEL_PROVIDER"))
            {
                if !provider.is_empty() {
                    self.default_provider = Some(provider);
                }
            } else if let Ok(provider) = std::env::var("PROVIDER") {

                if should_apply_legacy_provider(&self.default_provider) && !provider.is_empty() {
                    self.default_provider = Some(provider);
                }
            }
        }

        let senweaver_like = self
            .default_provider
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case("senweaver") || p.eq_ignore_ascii_case("sw"));

        if self.api_key.is_none() {
            if senweaver_like {
                if let Ok(key) = std::env::var("SENWEAVER_API_KEY") {
                    if !key.is_empty() {
                        self.api_key = Some(key);
                    }
                }
                if let Ok(base_url) = std::env::var("SENWEAVER_BASE_URL") {
                    if !base_url.trim().is_empty() {
                        self.api_url = Some(base_url.trim().to_string());
                    }
                }
            } else if let Ok(key) =
                std::env::var("SEN_API_KEY").or_else(|_| std::env::var("API_KEY"))
            {
                if !key.is_empty() {
                    self.api_key = Some(key);
                }
            }

            if self.api_key.is_none() {
                if self.default_provider.as_deref().is_some_and(is_glm_alias) {
                    if let Ok(key) = std::env::var("GLM_API_KEY") {
                        if !key.is_empty() {
                            self.api_key = Some(key);
                        }
                    }
                }
                if self.default_provider.as_deref().is_some_and(is_zai_alias) {
                    if let Ok(key) = std::env::var("ZAI_API_KEY") {
                        if !key.is_empty() {
                            self.api_key = Some(key);
                        }
                    }
                }
            }
        }

        if self.api_url.is_none() {
            if senweaver_like {
                if let Ok(base_url) = std::env::var("SENWEAVER_BASE_URL") {
                    if !base_url.trim().is_empty() {
                        self.api_url = Some(base_url.trim().to_string());
                    }
                }
            }
        }

        if self.default_model.is_none() {
            if let Ok(model) = std::env::var("SEN_MODEL").or_else(|_| std::env::var("MODEL")) {
                if !model.is_empty() {
                    self.default_model = Some(model);
                }
            }
        }

        if let Ok(timeout_str) = std::env::var("SEN_PROVIDER_TIMEOUT_SECS") {
            if let Ok(timeout_secs) = timeout_str.parse::<u64>() {
                if timeout_secs > 0 {
                    self.provider_timeout_secs = timeout_secs;
                }
            }
        }

        if let Ok(raw) = std::env::var("SEN_EXTRA_HEADERS") {
            for header in parse_extra_headers_env(&raw) {
                self.extra_headers.insert(header.0, header.1);
            }
        }

        self.apply_named_model_provider_profile();

        if let Some(workspace) = crate::util::get_runtime_var("SEN_WORKSPACE") {
            if !workspace.is_empty() {
                let expanded = expand_tilde_path(&workspace);
                let (_, workspace_dir) = resolve_config_dir_for_workspace(&expanded);
                self.workspace_dir = workspace_dir;
            }
        }

        if let Some(raw) = crate::util::get_runtime_var("SEN_MAX_ITERATIONS") {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                match trimmed.parse::<usize>() {
                    Ok(n) if n > 0 => {
                        self.agent.max_tool_iterations = n;
                        self.agent_runtime.max_tool_iterations =
                            n.min(u32::MAX as usize) as u32;
                    }
                    _ => tracing::warn!(
                        "Ignoring invalid SEN_MAX_ITERATIONS (must be a positive integer)"
                    ),
                }
            }
        }

        if let Some(raw) = crate::util::get_runtime_var("SEN_LOCALE") {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                self.locale = Some(crate::i18n::normalize_locale_public(trimmed));
            }
        }

        if let Ok(flag) = std::env::var("SEN_OPEN_SKILLS_ENABLED") {
            if !flag.trim().is_empty() {
                match flag.trim().to_ascii_lowercase().as_str() {
                    "1" | "true" | "yes" | "on" => self.skills.open_skills_enabled = true,
                    "0" | "false" | "no" | "off" => self.skills.open_skills_enabled = false,
                    _ => tracing::warn!(
                        "Ignoring invalid SEN_OPEN_SKILLS_ENABLED (valid: 1|0|true|false|yes|no|on|off)"
                    ),
                }
            }
        }

        if self.skills.open_skills_dir.is_none() {
            if let Ok(path) = std::env::var("SEN_OPEN_SKILLS_DIR") {
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    self.skills.open_skills_dir = Some(trimmed.to_string());
                }
            }
        }

        if let Ok(flag) = std::env::var("SEN_SKILLS_ALLOW_SCRIPTS") {
            if !flag.trim().is_empty() {
                match flag.trim().to_ascii_lowercase().as_str() {
                    "1" | "true" | "yes" | "on" => self.skills.allow_scripts = true,
                    "0" | "false" | "no" | "off" => self.skills.allow_scripts = false,
                    _ => tracing::warn!(
                        "Ignoring invalid SEN_SKILLS_ALLOW_SCRIPTS (valid: 1|0|true|false|yes|no|on|off)"
                    ),
                }
            }
        }

        if let Ok(mode) = std::env::var("SEN_SKILLS_PROMPT_MODE") {
            if !mode.trim().is_empty() {
                if let Some(parsed) = parse_skills_prompt_injection_mode(&mode) {
                    self.skills.prompt_injection_mode = parsed;
                } else {
                    tracing::warn!("Ignoring invalid SEN_SKILLS_PROMPT_MODE (valid: full|compact)");
                }
            }
        }

        if let Ok(port_str) = std::env::var("SEN_GATEWAY_PORT").or_else(|_| std::env::var("PORT")) {
            if let Ok(port) = port_str.parse::<u16>() {
                self.gateway.port = port;
            }
        }

        if let Ok(host) = std::env::var("SEN_GATEWAY_HOST").or_else(|_| std::env::var("HOST")) {
            if !host.is_empty() {
                self.gateway.host = host;
            }
        }

        if let Ok(val) = std::env::var("SEN_ALLOW_PUBLIC_BIND") {
            self.gateway.allow_public_bind = val == "1" || val.eq_ignore_ascii_case("true");
        }

        if let Ok(val) = std::env::var("SEN_REQUIRE_PAIRING") {
            self.gateway.require_pairing = val == "1" || val.eq_ignore_ascii_case("true");
        }

        if let Ok(temp_str) = std::env::var("SEN_TEMPERATURE") {
            match temp_str.parse::<f64>() {
                Ok(temp) if TEMPERATURE_RANGE.contains(&temp) => {
                    self.default_temperature = temp;
                }
                Ok(temp) => {
                    tracing::warn!(
                        "Ignoring SEN_TEMPERATURE={temp}: \
                         value out of range (expected {}..={})",
                        TEMPERATURE_RANGE.start(),
                        TEMPERATURE_RANGE.end()
                    );
                }
                Err(_) => {
                    tracing::warn!("Ignoring SEN_TEMPERATURE={temp_str:?}: not a valid number");
                }
            }
        }

        if let Ok(flag) =
            std::env::var("SEN_REASONING_ENABLED").or_else(|_| std::env::var("REASONING_ENABLED"))
        {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.runtime.reasoning_enabled = Some(true),
                "0" | "false" | "no" | "off" => self.runtime.reasoning_enabled = Some(false),
                _ => {}
            }
        }

        if let Ok(raw) = std::env::var("SEN_REASONING_EFFORT")
            .or_else(|_| std::env::var("REASONING_EFFORT"))
            .or_else(|_| std::env::var("SEN_CODEX_REASONING_EFFORT"))
        {
            match normalize_reasoning_effort(&raw) {
                Ok(effort) => self.runtime.reasoning_effort = Some(effort),
                Err(message) => tracing::warn!("Ignoring reasoning effort env override: {message}"),
            }
        }

        if let Ok(enabled) =
            std::env::var("SEN_WEB_SEARCH_ENABLED").or_else(|_| std::env::var("WEB_SEARCH_ENABLED"))
        {
            self.web_search.enabled = enabled == "1" || enabled.eq_ignore_ascii_case("true");
        }

        if let Ok(provider) = std::env::var("SEN_WEB_SEARCH_PROVIDER")
            .or_else(|_| std::env::var("WEB_SEARCH_PROVIDER"))
        {
            let provider = provider.trim();
            if !provider.is_empty() {
                self.web_search.provider = provider.to_string();
            }
        }

        if self.web_search.brave_api_key.is_none() {
            if let Ok(api_key) =
                std::env::var("SEN_BRAVE_API_KEY").or_else(|_| std::env::var("BRAVE_API_KEY"))
            {
                let api_key = api_key.trim();
                if !api_key.is_empty() {
                    self.web_search.brave_api_key = Some(api_key.to_string());
                }
            }
        }

        if self.web_search.searxng_instance_url.is_none() {
            if let Ok(instance_url) = std::env::var("SEN_SEARXNG_INSTANCE_URL")
                .or_else(|_| std::env::var("SEARXNG_INSTANCE_URL"))
            {
                let instance_url = instance_url.trim();
                if !instance_url.is_empty() {
                    self.web_search.searxng_instance_url = Some(instance_url.to_string());
                }
            }
        }

        if let Ok(max_results) = std::env::var("SEN_WEB_SEARCH_MAX_RESULTS")
            .or_else(|_| std::env::var("WEB_SEARCH_MAX_RESULTS"))
        {
            if let Ok(max_results) = max_results.parse::<usize>() {
                if (1..=10).contains(&max_results) {
                    self.web_search.max_results = max_results;
                }
            }
        }

        if let Ok(timeout_secs) = std::env::var("SEN_WEB_SEARCH_TIMEOUT_SECS")
            .or_else(|_| std::env::var("WEB_SEARCH_TIMEOUT_SECS"))
        {
            if let Ok(timeout_secs) = timeout_secs.parse::<u64>() {
                if timeout_secs > 0 {
                    self.web_search.timeout_secs = timeout_secs;
                }
            }
        }

        if let Ok(provider) = std::env::var("SEN_STORAGE_PROVIDER") {
            let provider = provider.trim();
            if !provider.is_empty() {
                self.storage.provider.config.provider = provider.to_string();
            }
        }

        if self.storage.provider.config.db_url.is_none() {
            if let Ok(db_url) = std::env::var("SEN_STORAGE_DB_URL") {
                let db_url = db_url.trim();
                if !db_url.is_empty() {
                    self.storage.provider.config.db_url = Some(db_url.to_string());
                }
            }
        }

        if let Ok(timeout_secs) = std::env::var("SEN_STORAGE_CONNECT_TIMEOUT_SECS") {
            if let Ok(timeout_secs) = timeout_secs.parse::<u64>() {
                if timeout_secs > 0 {
                    self.storage.provider.config.connect_timeout_secs = Some(timeout_secs);
                }
            }
        }

        let explicit_proxy_enabled = std::env::var("SEN_PROXY_ENABLED")
            .ok()
            .as_deref()
            .and_then(parse_proxy_enabled);
        if let Some(enabled) = explicit_proxy_enabled {
            self.proxy.enabled = enabled;
        }

        let mut proxy_url_overridden = false;
        if let Ok(proxy_url) =
            std::env::var("SEN_HTTP_PROXY").or_else(|_| std::env::var("HTTP_PROXY"))
        {
            self.proxy.http_proxy = normalize_proxy_url_option(Some(&proxy_url));
            proxy_url_overridden = true;
        }
        if let Ok(proxy_url) =
            std::env::var("SEN_HTTPS_PROXY").or_else(|_| std::env::var("HTTPS_PROXY"))
        {
            self.proxy.https_proxy = normalize_proxy_url_option(Some(&proxy_url));
            proxy_url_overridden = true;
        }
        if let Ok(proxy_url) =
            std::env::var("SEN_ALL_PROXY").or_else(|_| std::env::var("ALL_PROXY"))
        {
            self.proxy.all_proxy = normalize_proxy_url_option(Some(&proxy_url));
            proxy_url_overridden = true;
        }
        if let Ok(no_proxy) = std::env::var("SEN_NO_PROXY").or_else(|_| std::env::var("NO_PROXY")) {
            self.proxy.no_proxy = normalize_no_proxy_list(vec![no_proxy]);
        }

        if explicit_proxy_enabled.is_none()
            && proxy_url_overridden
            && self.proxy.has_any_proxy_url()
        {
            self.proxy.enabled = true;
        }

        if let Ok(scope_raw) = std::env::var("SEN_PROXY_SCOPE") {
            if let Some(scope) = parse_proxy_scope(&scope_raw) {
                self.proxy.scope = scope;
            } else {
                tracing::warn!(
                    scope = %scope_raw,
                    "Ignoring invalid SEN_PROXY_SCOPE (valid: environment|sen|services)"
                );
            }
        }

        if let Ok(services_raw) = std::env::var("SEN_PROXY_SERVICES") {
            self.proxy.services = normalize_service_list(vec![services_raw]);
        }

        if let Some(detect) = std::env::var("SEN_PROXY_SYSTEM_DETECT")
            .ok()
            .as_deref()
            .and_then(parse_proxy_enabled)
        {
            self.proxy.system_detect = detect;
        }

        if let Err(error) = self.proxy.validate() {
            tracing::warn!("Invalid proxy configuration ignored: {error}");
            self.proxy.enabled = false;
        }

        if self.proxy.enabled && self.proxy.scope == ProxyScope::Environment {
            self.proxy.apply_to_process_env();
        }

        crate::services::proxy::runtime::ProxyRuntime::global().replace(self.proxy.clone());
    }

    async fn resolve_config_path_for_save(&self) -> Result<PathBuf> {
        if self
            .config_path
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty())
        {
            return Ok(self.config_path.clone());
        }

        let (default_sen_dir, default_workspace_dir) = default_config_and_workspace_dirs()?;
        let (sen_dir, _workspace_dir, source) =
            resolve_runtime_config_dirs(&default_sen_dir, &default_workspace_dir).await?;
        let file_name = self
            .config_path
            .file_name()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| std::ffi::OsStr::new("config.toml"));
        let resolved = sen_dir.join(file_name);
        tracing::warn!(
            path = %self.config_path.display(),
            resolved = %resolved.display(),
            source = source.as_str(),
            "Config path missing parent directory; resolving from runtime environment"
        );
        Ok(resolved)
    }

    pub async fn save(&self) -> Result<()> {

        let mut config_to_save = self.clone();
        if let Some(baseline) = self.env_override_baseline.clone() {
            match toml::Value::try_from(&config_to_save) {
                Ok(current) => {
                    if let Some(disk_form) = strip_env_overrides(
                        Some(&baseline.pristine),
                        Some(&baseline.env_applied),
                        current,
                    ) {
                        match disk_form.try_into::<Config>() {
                            Ok(mut rebuilt) => {
                                rebuilt.workspace_dir = config_to_save.workspace_dir.clone();
                                rebuilt.config_path = config_to_save.config_path.clone();
                                rebuilt.env_override_baseline = Some(baseline);
                                config_to_save = rebuilt;
                            }
                            Err(err) => tracing::warn!(
                                error = %err,
                                "failed to rebuild disk-form config after stripping env overrides; saving in-memory form"
                            ),
                        }
                    }
                }
                Err(err) => tracing::warn!(
                    error = %err,
                    "failed to serialize config for env-override stripping; saving in-memory form"
                ),
            }
        }
        let config_path = self.resolve_config_path_for_save().await?;
        let sen_dir = config_path
            .parent()
            .context("Config path must have a parent directory")?;
        let store = crate::security::SecretStore::new(sen_dir, self.secrets.encrypt);

        encrypt_optional_secret(&store, &mut config_to_save.api_key, "config.api_key")?;
        encrypt_optional_secret(
            &store,
            &mut config_to_save.composio.api_key,
            "config.composio.api_key",
        )?;
        if let Some(ref mut pinggy) = config_to_save.tunnel.pinggy {
            encrypt_optional_secret(&store, &mut pinggy.token, "config.tunnel.pinggy.token")?;
        }
        encrypt_optional_secret(
            &store,
            &mut config_to_save.microsoft365.client_secret,
            "config.microsoft365.client_secret",
        )?;

        encrypt_optional_secret(
            &store,
            &mut config_to_save.web_search.brave_api_key,
            "config.web_search.brave_api_key",
        )?;

        encrypt_optional_secret(
            &store,
            &mut config_to_save.storage.provider.config.db_url,
            "config.storage.provider.config.db_url",
        )?;

        for agent in config_to_save.agents.values_mut() {
            encrypt_optional_secret(&store, &mut agent.api_key, "config.agents.*.api_key")?;
        }

        for provider in config_to_save.model_providers.values_mut() {
            encrypt_optional_secret(
                &store,
                &mut provider.api_key,
                "config.model_providers.*.api_key",
            )?;
        }

        if let Some(ref mut openai) = config_to_save.tts.openai {
            encrypt_optional_secret(&store, &mut openai.api_key, "config.tts.openai.api_key")?;
        }
        if let Some(ref mut elevenlabs) = config_to_save.tts.elevenlabs {
            encrypt_optional_secret(
                &store,
                &mut elevenlabs.api_key,
                "config.tts.elevenlabs.api_key",
            )?;
        }
        if let Some(ref mut google) = config_to_save.tts.google {
            encrypt_optional_secret(&store, &mut google.api_key, "config.tts.google.api_key")?;
        }

        encrypt_optional_secret(
            &store,
            &mut config_to_save.transcription.api_key,
            "config.transcription.api_key",
        )?;
        if let Some(ref mut openai) = config_to_save.transcription.openai {
            encrypt_optional_secret(
                &store,
                &mut openai.api_key,
                "config.transcription.openai.api_key",
            )?;
        }
        if let Some(ref mut deepgram) = config_to_save.transcription.deepgram {
            encrypt_optional_secret(
                &store,
                &mut deepgram.api_key,
                "config.transcription.deepgram.api_key",
            )?;
        }
        if let Some(ref mut assemblyai) = config_to_save.transcription.assemblyai {
            encrypt_optional_secret(
                &store,
                &mut assemblyai.api_key,
                "config.transcription.assemblyai.api_key",
            )?;
        }
        if let Some(ref mut google) = config_to_save.transcription.google {
            encrypt_optional_secret(
                &store,
                &mut google.api_key,
                "config.transcription.google.api_key",
            )?;
        }
        if let Some(ref mut local) = config_to_save.transcription.local_whisper {
            encrypt_optional_secret(
                &store,
                &mut local.bearer_token,
                "config.transcription.local_whisper.bearer_token",
            )?;
        }

        #[cfg(feature = "channel-nostr")]
        if let Some(ref mut ns) = config_to_save.channels_config.nostr {
            encrypt_secret(
                &store,
                &mut ns.private_key,
                "config.channels_config.nostr.private_key",
            )?;
        }
        if let Some(ref mut fs) = config_to_save.channels_config.feishu {
            encrypt_secret(
                &store,
                &mut fs.app_secret,
                "config.channels_config.feishu.app_secret",
            )?;
            encrypt_optional_secret(
                &store,
                &mut fs.encrypt_key,
                "config.channels_config.feishu.encrypt_key",
            )?;
            encrypt_optional_secret(
                &store,
                &mut fs.verification_token,
                "config.channels_config.feishu.verification_token",
            )?;
        }

        if let Some(ref mut tg) = config_to_save.channels_config.telegram {
            encrypt_secret(
                &store,
                &mut tg.bot_token,
                "config.channels_config.telegram.bot_token",
            )?;
        }
        if let Some(ref mut dc) = config_to_save.channels_config.discord {
            encrypt_secret(
                &store,
                &mut dc.bot_token,
                "config.channels_config.discord.bot_token",
            )?;
        }
        if let Some(ref mut sl) = config_to_save.channels_config.slack {
            encrypt_secret(
                &store,
                &mut sl.bot_token,
                "config.channels_config.slack.bot_token",
            )?;
            encrypt_optional_secret(
                &store,
                &mut sl.app_token,
                "config.channels_config.slack.app_token",
            )?;
        }
        if let Some(ref mut mm) = config_to_save.channels_config.mattermost {
            encrypt_secret(
                &store,
                &mut mm.bot_token,
                "config.channels_config.mattermost.bot_token",
            )?;
        }
        if let Some(ref mut mx) = config_to_save.channels_config.matrix {
            encrypt_secret(
                &store,
                &mut mx.access_token,
                "config.channels_config.matrix.access_token",
            )?;
            encrypt_optional_secret(
                &store,
                &mut mx.recovery_key,
                "config.channels_config.matrix.recovery_key",
            )?;
        }
        if let Some(ref mut wa) = config_to_save.channels_config.whatsapp {
            encrypt_optional_secret(
                &store,
                &mut wa.access_token,
                "config.channels_config.whatsapp.access_token",
            )?;
            encrypt_optional_secret(
                &store,
                &mut wa.app_secret,
                "config.channels_config.whatsapp.app_secret",
            )?;
            encrypt_optional_secret(
                &store,
                &mut wa.verify_token,
                "config.channels_config.whatsapp.verify_token",
            )?;
        }
        if let Some(ref mut lq) = config_to_save.channels_config.linq {
            encrypt_secret(
                &store,
                &mut lq.api_token,
                "config.channels_config.linq.api_token",
            )?;
            encrypt_optional_secret(
                &store,
                &mut lq.signing_secret,
                "config.channels_config.linq.signing_secret",
            )?;
        }
        if let Some(ref mut wt) = config_to_save.channels_config.wati {
            encrypt_secret(
                &store,
                &mut wt.api_token,
                "config.channels_config.wati.api_token",
            )?;
        }
        if let Some(ref mut nc) = config_to_save.channels_config.nextcloud_talk {
            encrypt_secret(
                &store,
                &mut nc.app_token,
                "config.channels_config.nextcloud_talk.app_token",
            )?;
            encrypt_optional_secret(
                &store,
                &mut nc.webhook_secret,
                "config.channels_config.nextcloud_talk.webhook_secret",
            )?;
        }
        if let Some(ref mut em) = config_to_save.channels_config.email {
            encrypt_secret(
                &store,
                &mut em.password,
                "config.channels_config.email.password",
            )?;
        }
        if let Some(ref mut gp) = config_to_save.channels_config.gmail_push {
            encrypt_secret(
                &store,
                &mut gp.oauth_token,
                "config.channels_config.gmail_push.oauth_token",
            )?;
        }
        if let Some(ref mut irc) = config_to_save.channels_config.irc {
            encrypt_optional_secret(
                &store,
                &mut irc.server_password,
                "config.channels_config.irc.server_password",
            )?;
            encrypt_optional_secret(
                &store,
                &mut irc.nickserv_password,
                "config.channels_config.irc.nickserv_password",
            )?;
            encrypt_optional_secret(
                &store,
                &mut irc.sasl_password,
                "config.channels_config.irc.sasl_password",
            )?;
        }
        if let Some(ref mut lk) = config_to_save.channels_config.lark {
            encrypt_secret(
                &store,
                &mut lk.app_secret,
                "config.channels_config.lark.app_secret",
            )?;
            encrypt_optional_secret(
                &store,
                &mut lk.encrypt_key,
                "config.channels_config.lark.encrypt_key",
            )?;
            encrypt_optional_secret(
                &store,
                &mut lk.verification_token,
                "config.channels_config.lark.verification_token",
            )?;
        }
        if let Some(ref mut fs) = config_to_save.channels_config.feishu {
            encrypt_secret(
                &store,
                &mut fs.app_secret,
                "config.channels_config.feishu.app_secret",
            )?;
            encrypt_optional_secret(
                &store,
                &mut fs.encrypt_key,
                "config.channels_config.feishu.encrypt_key",
            )?;
            encrypt_optional_secret(
                &store,
                &mut fs.verification_token,
                "config.channels_config.feishu.verification_token",
            )?;
        }
        if let Some(ref mut dt) = config_to_save.channels_config.dingtalk {
            encrypt_secret(
                &store,
                &mut dt.client_secret,
                "config.channels_config.dingtalk.client_secret",
            )?;
        }
        if let Some(ref mut wc) = config_to_save.channels_config.wecom {
            encrypt_secret(
                &store,
                &mut wc.webhook_key,
                "config.channels_config.wecom.webhook_key",
            )?;
        }
        if let Some(ref mut qq) = config_to_save.channels_config.qq {
            encrypt_secret(
                &store,
                &mut qq.app_secret,
                "config.channels_config.qq.app_secret",
            )?;
        }
        if let Some(ref mut wh) = config_to_save.channels_config.webhook {
            encrypt_optional_secret(
                &store,
                &mut wh.secret,
                "config.channels_config.webhook.secret",
            )?;
        }
        if let Some(ref mut ct) = config_to_save.channels_config.telnyx {
            encrypt_secret(
                &store,
                &mut ct.api_key,
                "config.channels_config.telnyx.api_key",
            )?;
            encrypt_optional_secret(
                &store,
                &mut ct.webhook_secret,
                "config.channels_config.telnyx.webhook_secret",
            )?;
        }

        for token in &mut config_to_save.gateway.paired_tokens {
            encrypt_secret(&store, token, "config.gateway.paired_tokens[]")?;
        }

        encrypt_optional_secret(
            &store,
            &mut config_to_save.security.nevis.client_secret,
            "config.security.nevis.client_secret",
        )?;

        if !config_to_save.notion.api_key.is_empty() {
            encrypt_secret(
                &store,
                &mut config_to_save.notion.api_key,
                "config.notion.api_key",
            )?;
        }

        if !config_to_save.jira.api_token.is_empty() {
            encrypt_secret(
                &store,
                &mut config_to_save.jira.api_token,
                "config.jira.api_token",
            )?;
        }

        let toml_str =
            toml::to_string_pretty(&config_to_save).context("Failed to serialize config")?;

        let parent_dir = config_path
            .parent()
            .context("Config path must have a parent directory")?;

        fs::create_dir_all(parent_dir).await.with_context(|| {
            format!(
                "Failed to create config directory: {}",
                parent_dir.display()
            )
        })?;

        // Serialize concurrent writers (CLI + desktop + gateway may all persist
        // config.toml) with a cross-process advisory lock file so they cannot
        // clobber each other's read-modify-write with a last-writer-wins overwrite.
        let _config_write_lock = ConfigWriteLock::acquire(&config_path).await;

        let file_name = config_path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("config.toml");
        let temp_path = parent_dir.join(format!(".{file_name}.tmp-{}", uuid::Uuid::new_v4()));
        let backup_path = parent_dir.join(format!("{file_name}.bak"));

        let mut temp_file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .await
            .with_context(|| {
                format!(
                    "Failed to create temporary config file: {}",
                    temp_path.display()
                )
            })?;
        temp_file
            .write_all(toml_str.as_bytes())
            .await
            .context("Failed to write temporary config contents")?;
        temp_file
            .sync_all()
            .await
            .context("Failed to fsync temporary config file")?;
        drop(temp_file);

        let had_existing_config = config_path.exists();
        if had_existing_config {
            fs::copy(&config_path, &backup_path)
                .await
                .with_context(|| {
                    format!(
                        "Failed to create config backup before atomic replace: {}",
                        backup_path.display()
                    )
                })?;
        }

        if let Err(e) = fs::rename(&temp_path, &config_path).await {
            let _ = fs::remove_file(&temp_path).await;
            if had_existing_config && backup_path.exists() {
                fs::copy(&backup_path, &config_path)
                    .await
                    .context("Failed to restore config backup")?;
            }
            anyhow::bail!("Failed to atomically replace config file: {e}");
        }

        #[cfg(unix)]
        {
            use std::{fs::Permissions, os::unix::fs::PermissionsExt};
            if let Err(err) = fs::set_permissions(&config_path, Permissions::from_mode(0o600)).await
            {
                tracing::warn!(
                    "Failed to harden config permissions to 0600 at {}: {}",
                    config_path.display(),
                    err
                );
            }
        }

        // Stamp the dir as sen-owned so future resolution never mistakes a
        // neighbouring project's config.toml for ours (and vice versa).
        let marker_path = parent_dir.join(SEN_CONFIG_DIR_MARKER);
        if !marker_path.exists() {
            if let Err(err) = fs::write(
                &marker_path,
                b"This directory is a SenWeaverCoding config home. Do not remove.\n",
            )
            .await
            {
                tracing::debug!(
                    error = %err,
                    path = %marker_path.display(),
                    "could not write sen config-dir marker"
                );
            }
        }

        sync_directory(parent_dir).await?;

        if had_existing_config {
            let _ = fs::remove_file(&backup_path).await;
        }

        Ok(())
    }
}

#[allow(clippy::unused_async)]
async fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let dir = File::open(path)
            .await
            .with_context(|| format!("Failed to open directory for fsync: {}", path.display()))?;
        dir.sync_all()
            .await
            .with_context(|| format!("Failed to fsync directory metadata: {}", path.display()))?;
        Ok(())
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

        let _ = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .and_then(|dir| dir.sync_all());
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(())
    }
}

