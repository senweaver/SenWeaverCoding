// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod acp_server;
pub mod bluesky;
pub mod bridge;
pub mod telnyx;
pub mod cli;
pub mod dingtalk;
pub mod discord;
pub mod email_channel;
pub mod gmail_push;
pub mod imessage;
pub mod irc;
#[cfg(feature = "channel-lark")]
pub mod lark;
pub mod linq;
mod memory_keys;
#[cfg(feature = "channel-matrix")]
pub mod matrix;
pub mod mattermost;
pub mod pipeline;
pub mod mochat;
pub mod nextcloud_talk;
#[cfg(feature = "channel-nostr")]
pub mod nostr;
pub mod notion;
pub mod qq;
pub mod reddit;
pub mod session;

pub mod shared;
pub mod signal;
pub mod slack;
pub mod telegram;
pub mod traits;
pub mod twitter;
pub mod voice;
pub mod wati;
pub mod webhook;
pub mod wecom;
pub mod whatsapp;

pub use bluesky::BlueskyChannel;
pub use telnyx::{TelnyxChannel, TelnyxConfig};
pub use cli::CliChannel;
pub use dingtalk::DingTalkChannel;
pub use discord::DiscordChannel;
pub use discord::history::DiscordHistoryChannel;
pub use email_channel::EmailChannel;
pub use gmail_push::GmailPushChannel;
pub use imessage::IMessageChannel;
pub use irc::IrcChannel;
#[cfg(feature = "channel-lark")]
pub use lark::LarkChannel;
pub use linq::LinqChannel;
#[cfg(feature = "channel-matrix")]
pub use matrix::MatrixChannel;
pub use mattermost::MattermostChannel;
pub use mochat::MochatChannel;
pub use nextcloud_talk::NextcloudTalkChannel;
#[cfg(feature = "channel-nostr")]
pub use nostr::NostrChannel;
pub use notion::NotionChannel;
pub use qq::QQChannel;
pub use reddit::RedditChannel;
pub use signal::SignalChannel;
pub use slack::SlackChannel;
pub use telegram::TelegramChannel;
pub use traits::{Channel, SendMessage};
pub use pipeline::tts::{TtsManager, TtsProvider};

pub use twitter::TwitterChannel;
pub use voice::call::{VoiceCallChannel, VoiceCallConfig};
#[cfg(feature = "voice-wake")]
pub use voice::wake::VoiceWakeChannel;
pub use wati::WatiChannel;
pub use webhook::WebhookChannel;
pub use wecom::WeComChannel;
pub use whatsapp::WhatsAppChannel;
#[cfg(feature = "whatsapp-web")]
pub use whatsapp::web::WhatsAppWebChannel;

use crate::approval::ApprovalManager;
pub use crate::channels::bridge::agent::{AgentLoopCore, TurnEvent};
use crate::config::Config;
use crate::memory::{self, Memory};
use crate::observability::{self, Observer};
use crate::providers::{self, ChatMessage, Provider};
use crate::runtime;
use crate::security::{AutonomyLevel, SecurityPolicy};
use crate::tools::{self, Tool};
use crate::util::truncate_with_ellipsis;
use anyhow::{Context, Result};
use std::sync::atomic::Ordering;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;
use tokio_util::sync::CancellationToken;

type ConversationHistoryMap = Arc<Mutex<HashMap<String, Vec<ChatMessage>>>>;

type PendingNewSessionSet = Arc<Mutex<HashSet<String>>>;

const MAX_CHANNEL_HISTORY: usize = 50;

const BOOTSTRAP_MAX_CHARS: usize = 20_000;

const MIN_CHANNEL_MESSAGE_TIMEOUT_SECS: u64 = 30;

const CHANNEL_MESSAGE_TIMEOUT_SCALE_CAP: u64 = 4;
const CHANNEL_MAX_IN_FLIGHT_MESSAGES: usize = 64;
const MODEL_CACHE_FILE: &str = "models_cache.json";
const MODEL_CACHE_PREVIEW_LIMIT: usize = 10;
const MEMORY_CONTEXT_MAX_ENTRIES: usize = 4;
const MEMORY_CONTEXT_ENTRY_MAX_CHARS: usize = 800;
const MEMORY_CONTEXT_MAX_CHARS: usize = 4_000;
const CHANNEL_HISTORY_COMPACT_KEEP_MESSAGES: usize = 12;
const CHANNEL_HISTORY_COMPACT_CONTENT_CHARS: usize = 600;

const PROACTIVE_CONTEXT_BUDGET_CHARS: usize = 400_000;

type ProviderCacheMap = Arc<Mutex<HashMap<String, Arc<dyn Provider>>>>;
type RouteSelectionMap = Arc<Mutex<HashMap<String, ChannelRouteSelection>>>;

fn effective_channel_message_timeout_secs(configured: u64) -> u64 {
    configured.max(MIN_CHANNEL_MESSAGE_TIMEOUT_SECS)
}

fn channel_message_timeout_budget_secs(
    message_timeout_secs: u64,
    max_tool_iterations: usize,
) -> u64 {
    channel_message_timeout_budget_secs_with_cap(
        message_timeout_secs,
        max_tool_iterations,
        CHANNEL_MESSAGE_TIMEOUT_SCALE_CAP,
    )
}

fn channel_message_timeout_budget_secs_with_cap(
    message_timeout_secs: u64,
    max_tool_iterations: usize,
    scale_cap: u64,
) -> u64 {
    let iterations = max_tool_iterations.max(1) as u64;
    let scale = iterations.min(scale_cap);
    message_timeout_secs.saturating_mul(scale)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChannelRouteSelection {
    provider: String,
    model: String,

    api_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChannelRuntimeCommand {
    ShowProviders,
    SetProvider(String),
    ShowModel,
    SetModel(String),
    ShowConfig,
    NewSession,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ModelCacheState {
    entries: Vec<ModelCacheEntry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ModelCacheEntry {
    provider: String,
    models: Vec<String>,
}

#[derive(Debug, Clone)]
struct ChannelRuntimeDefaults {
    default_provider: String,
    model: String,
    temperature: f64,
    api_key: Option<String>,
    api_url: Option<String>,
    reliability: crate::config::ReliabilityConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ConfigFileStamp {
    modified: SystemTime,
    len: u64,
}

#[derive(Debug, Clone)]
struct RuntimeConfigState {
    defaults: ChannelRuntimeDefaults,
    last_applied_stamp: Option<ConfigFileStamp>,
}

fn runtime_config_store() -> &'static Mutex<HashMap<PathBuf, RuntimeConfigState>> {
    static STORE: OnceLock<Mutex<HashMap<PathBuf, RuntimeConfigState>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

const SYSTEMD_STATUS_ARGS: [&str; 3] = ["--user", "is-active", "sen.service"];
const SYSTEMD_RESTART_ARGS: [&str; 3] = ["--user", "restart", "sen.service"];
const OPENRC_STATUS_ARGS: [&str; 2] = ["sen", "status"];
const OPENRC_RESTART_ARGS: [&str; 2] = ["sen", "restart"];

#[derive(Clone, Copy)]
#[allow(clippy::struct_excessive_bools)]
struct InterruptOnNewMessageConfig {
    telegram: bool,
    slack: bool,
    discord: bool,
    mattermost: bool,
    matrix: bool,
}

impl InterruptOnNewMessageConfig {
    fn enabled_for_channel(self, channel: &str) -> bool {
        match channel {
            "telegram" => self.telegram,
            "slack" => self.slack,
            "discord" => self.discord,
            "mattermost" => self.mattermost,
            "matrix" => self.matrix,
            _ => false,
        }
    }
}

fn interrupt_on_new_message_from_config(config: &Config) -> InterruptOnNewMessageConfig {
    let cc = &config.channels_config;
    InterruptOnNewMessageConfig {
        telegram: cc
            .telegram
            .as_ref()
            .is_some_and(|c| c.interrupt_on_new_message),
        slack: cc
            .slack
            .as_ref()
            .is_some_and(|c| c.interrupt_on_new_message),
        discord: cc
            .discord
            .as_ref()
            .is_some_and(|c| c.interrupt_on_new_message),
        mattermost: cc
            .mattermost
            .as_ref()
            .is_some_and(|c| c.interrupt_on_new_message),
        matrix: cc
            .matrix
            .as_ref()
            .is_some_and(|c| c.interrupt_on_new_message),
    }
}

fn sanitize_channel_outbound_response(raw: &str, show_tool_calls: bool) -> String {
    if show_tool_calls {
        return raw.to_string();
    }
    let mut cleaned = strip_tool_call_tags(raw);
    cleaned = strip_tool_result_content(&cleaned);
    cleaned = strip_tool_summary_prefix(&cleaned);
    if cleaned.trim().is_empty() {
        "Done.".to_string()
    } else {
        cleaned
    }
}

fn capture_daemon_command_output(cmd: &mut std::process::Command) -> Option<String> {
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    cmd.output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
}

fn channel_background_service_status() -> Option<&'static str> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    if capture_daemon_command_output(
        crate::util::hidden_sync_command("systemctl").args(SYSTEMD_STATUS_ARGS),
    )
    .map(|out| out.trim() == "active")
    .unwrap_or(false)
    {
        return Some("systemd: active");
    }
    if capture_daemon_command_output(
        crate::util::hidden_sync_command("rc-service").args(OPENRC_STATUS_ARGS),
    )
    .map(|out| out.contains("started"))
    .unwrap_or(false)
    {
        return Some("openrc: started");
    }
    Some("not running")
}

fn print_channel_service_restart_hints() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let systemd_cmd = format!(
        "systemctl {}",
        SYSTEMD_RESTART_ARGS.join(" ")
    );
    let openrc_cmd = format!(
        "rc-service {}",
        OPENRC_RESTART_ARGS.join(" ")
    );
    println!("Service restart: {systemd_cmd}");
    println!("Service restart: {openrc_cmd}");
}

#[derive(Clone)]
#[allow(dead_code)]
struct ChannelCostTrackingState {
    tracker: Arc<crate::cost::CostTracker>,
    prices: Arc<HashMap<String, crate::config::schema::ModelPricing>>,
}

#[derive(Clone)]
#[allow(dead_code)]
struct ChannelRuntimeContext {
    channels_by_name: Arc<HashMap<String, Arc<dyn Channel>>>,
    provider: Arc<dyn Provider>,
    default_provider: Arc<String>,
    prompt_config: Arc<crate::config::Config>,
    memory: Arc<dyn Memory>,
    tools_registry: Arc<Vec<Box<dyn Tool>>>,
    observer: Arc<dyn Observer>,
    system_prompt: Arc<String>,
    model: Arc<String>,
    temperature: f64,
    auto_save_memory: bool,
    max_tool_iterations: usize,
    min_relevance_score: f64,
    conversation_histories: ConversationHistoryMap,
    pending_new_sessions: PendingNewSessionSet,
    provider_cache: ProviderCacheMap,
    route_overrides: RouteSelectionMap,
    api_key: Option<String>,
    api_url: Option<String>,
    reliability: Arc<crate::config::ReliabilityConfig>,
    provider_runtime_options: providers::ProviderRuntimeOptions,
    workspace_dir: Arc<PathBuf>,
    message_timeout_secs: u64,
    interrupt_on_new_message: InterruptOnNewMessageConfig,
    multimodal: crate::config::MultimodalConfig,
    media_pipeline: crate::config::MediaPipelineConfig,
    transcription_config: crate::config::TranscriptionConfig,
    hooks: Option<Arc<crate::hooks::HookRunner>>,
    non_cli_excluded_tools: Arc<Vec<String>>,
    autonomy_level: AutonomyLevel,
    tool_call_dedup_exempt: Arc<Vec<String>>,
    model_routes: Arc<Vec<crate::config::ModelRouteConfig>>,
    query_classification: crate::config::QueryClassificationConfig,
    ack_reactions: bool,
    show_tool_calls: bool,
    session_store: Option<Arc<session::store::SessionStore>>,

    approval_manager: Arc<ApprovalManager>,
    activated_tools: Option<std::sync::Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>>,
    cost_tracking: Option<ChannelCostTrackingState>,
    pacing: crate::config::PacingConfig,
    debouncer: Arc<pipeline::debounce::MessageDebouncer>,

    rbac_engine: Option<Arc<crate::security::rbac::RbacEngine>>,
}

#[derive(Clone)]
struct InFlightSenderTaskState {
    task_id: u64,
    cancellation: CancellationToken,
    completion: Arc<InFlightTaskCompletion>,
}

struct InFlightTaskCompletion {
    done: AtomicBool,
    notify: tokio::sync::Notify,
}

impl InFlightTaskCompletion {
    fn new() -> Self {
        Self {
            done: AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        }
    }

    fn mark_done(&self) {
        self.done.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    async fn wait(&self) {
        if self.done.load(Ordering::Acquire) {
            return;
        }
        self.notify.notified().await;
    }
}

use crate::channels::memory_keys::conversation_history_key;

use crate::channels::pipeline::tool_tag_stripper::strip_tool_call_tags;

fn channel_delivery_instructions(channel_name: &str) -> Option<&'static str> {
    match channel_name {
        "matrix" => Some(
            "When responding on Matrix:\n\
             - Use Markdown formatting (bold, italic, code blocks)\n\
             - Be concise and direct\n\
             - When you receive a [Voice message], the user spoke to you. Respond naturally as in conversation.\n\
             - Your text reply will automatically be converted to audio and sent back as a voice message.\n",
        ),
        "telegram" => Some(
            "When responding on Telegram:\n\
             - Include media markers for files or URLs that should be sent as attachments\n\
             - Use **bold** for key terms, section titles, and important info (renders as <b>)\n\
             - Use *italic* for emphasis (renders as <i>)\n\
             - Use `backticks` for inline code, commands, or technical terms\n\
             - Use triple backticks for code blocks\n\
             - Use emoji naturally to add personality  -  but don't overdo it\n\
             - Be concise and direct. Skip filler phrases like 'Great question!' or 'Certainly!'\n\
             - Structure longer answers with bold headers, not raw markdown ## headers\n\
             - For media attachments use markers: [IMAGE:<path-or-url>], [DOCUMENT:<path-or-url>], [VIDEO:<path-or-url>], [AUDIO:<path-or-url>], or [VOICE:<path-or-url>]\n\
             - Keep normal text outside markers and never wrap markers in code fences.\n\
             - Use tool results silently: answer the latest user message directly, and do not narrate delayed/internal tool execution bookkeeping.",
        ),
        "qq" => Some(
            "When responding on QQ:\n\
             - Use Markdown formatting\n\
             - Be concise and direct\n\
             - For media attachments use markers: [IMAGE:<path-or-url>], [DOCUMENT:<path-or-url>], \
               [VIDEO:<path-or-url>], [VOICE:<path-or-url>]\n\
             - Voice supports .wav, .mp3, .silk formats only. Other audio formats use [DOCUMENT:]\n\
             - Keep normal text outside markers and never wrap markers in code fences.\n",
        ),
        _ => None,
    }
}

fn build_channel_system_prompt(
    base_prompt: &str,
    channel_name: &str,
    reply_target: &str,
) -> String {
    let mut prompt = base_prompt.to_string();

    {
        let now = chrono::Local::now();
        let fresh = format!(
            "## Current Date & Time\n\n{} ({})\n",
            now.format("%Y-%m-%d %H:%M:%S"),
            now.format("%Z"),
        );
        if let Some(start) = prompt.find("## Current Date & Time\n\n") {

            let rest = &prompt[start + 24..];
            let section_end = rest
                .find("\n## ")
                .map(|i| start + 24 + i)
                .unwrap_or(prompt.len());
            prompt.replace_range(start..section_end, fresh.trim_end());
        }
    }

    if let Some(instructions) = channel_delivery_instructions(channel_name) {
        if prompt.is_empty() {
            prompt = instructions.to_string();
        } else {
            prompt = format!("{prompt}\n\n{instructions}");
        }
    }

    if !reply_target.is_empty() {
        let context = format!(
            "\n\nChannel context: You are currently responding on channel={channel_name}, \
             reply_target={reply_target}. When scheduling delayed messages or reminders \
             via cron_add for this conversation, use delivery={{\"mode\":\"announce\",\
             \"channel\":\"{channel_name}\",\"to\":\"{reply_target}\"}} so the message \
             reaches the user."
        );
        prompt.push_str(&context);
    }

    prompt
}

fn normalize_cached_channel_turns(turns: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let mut normalized = Vec::with_capacity(turns.len());
    let mut expecting_user = true;

    for turn in turns {
        match (expecting_user, turn.role.as_str()) {
            (true, "user") => {
                normalized.push(turn);
                expecting_user = false;
            }
            (false, "assistant") => {
                normalized.push(turn);
                expecting_user = true;
            }

            (false, "user") | (true, "assistant") => {
                if let Some(last_turn) = normalized.last_mut() {
                    if !turn.content.is_empty() {
                        if !last_turn.content.is_empty() {
                            last_turn.content.push_str("\n\n");
                        }
                        last_turn.content.push_str(&turn.content);
                    }
                }
            }
            _ => {}
        }
    }

    normalized
}

fn strip_tool_result_content(text: &str) -> String {
    static TOOL_RESULT_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?s)<tool_result[^>]*>.*?</tool_result>")
            .expect("tool_result strip regex must compile")
    });

    let cleaned = TOOL_RESULT_RE.replace_all(text, "");
    let cleaned = cleaned.trim();

    if cleaned == "[Tool results]" || cleaned.is_empty() {
        return String::new();
    }

    cleaned.to_string()
}

fn strip_tool_summary_prefix(text: &str) -> String {
    if let Some(rest) = text.strip_prefix("[Used tools:") {

        if let Some(bracket_end) = rest.find(']') {
            let after_bracket = &rest[bracket_end + 1..];
            let trimmed = after_bracket.trim_start_matches('\n');
            if trimmed.is_empty() {
                return String::new();
            }
            return trimmed.to_string();
        }
    }
    text.to_string()
}

fn supports_runtime_model_switch(channel_name: &str) -> bool {
    matches!(channel_name, "telegram" | "discord" | "matrix" | "slack")
}

fn parse_runtime_command(channel_name: &str, content: &str) -> Option<ChannelRuntimeCommand> {
    let trimmed = content.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    let mut parts = trimmed.split_whitespace();
    let command_token = parts.next()?;
    let base_command = command_token
        .split('@')
        .next()
        .unwrap_or(command_token)
        .to_ascii_lowercase();

    match base_command.as_str() {

        "/new" => Some(ChannelRuntimeCommand::NewSession),

        "/models" if supports_runtime_model_switch(channel_name) => {
            if let Some(provider) = parts.next() {
                Some(ChannelRuntimeCommand::SetProvider(
                    provider.trim().to_string(),
                ))
            } else {
                Some(ChannelRuntimeCommand::ShowProviders)
            }
        }
        "/model" if supports_runtime_model_switch(channel_name) => {
            let model = parts.collect::<Vec<_>>().join(" ").trim().to_string();
            if model.is_empty() {
                Some(ChannelRuntimeCommand::ShowModel)
            } else {
                Some(ChannelRuntimeCommand::SetModel(model))
            }
        }
        "/config" if supports_runtime_model_switch(channel_name) => {
            Some(ChannelRuntimeCommand::ShowConfig)
        }
        _ => None,
    }
}

fn resolve_provider_alias(name: &str) -> Option<String> {
    let candidate = name.trim();
    if candidate.is_empty() {
        return None;
    }

    let providers_list = providers::list_providers();
    for provider in providers_list {
        if provider.name.eq_ignore_ascii_case(candidate)
            || provider
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(candidate))
        {
            return Some(provider.name.to_string());
        }
    }

    None
}

fn resolved_default_provider(config: &Config) -> String {
    config
        .default_provider
        .clone()
        .unwrap_or_else(|| "openrouter".to_string())
}

fn resolved_default_model(config: &Config) -> String {
    match crate::providers::resolve_default_model(config) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(target = "config", "no_model_configured (channels runtime): {e}");
            String::new()
        }
    }
}

fn runtime_defaults_from_config(config: &Config) -> ChannelRuntimeDefaults {
    ChannelRuntimeDefaults {
        default_provider: resolved_default_provider(config),
        model: resolved_default_model(config),
        temperature: config.default_temperature,
        api_key: config.api_key.clone(),
        api_url: config.api_url.clone(),
        reliability: config.reliability.clone(),
    }
}

fn runtime_config_path(ctx: &ChannelRuntimeContext) -> Option<PathBuf> {
    ctx.provider_runtime_options
        .sen_dir
        .as_ref()
        .map(|dir| dir.join("config.toml"))
}

fn runtime_defaults_snapshot(ctx: &ChannelRuntimeContext) -> ChannelRuntimeDefaults {
    if let Some(config_path) = runtime_config_path(ctx) {
        let store = runtime_config_store()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(state) = store.get(&config_path) {
            return state.defaults.clone();
        }
    }

    ChannelRuntimeDefaults {
        default_provider: ctx.default_provider.as_str().to_string(),
        model: ctx.model.as_str().to_string(),
        temperature: ctx.temperature,
        api_key: ctx.api_key.clone(),
        api_url: ctx.api_url.clone(),
        reliability: (*ctx.reliability).clone(),
    }
}

async fn config_file_stamp(path: &Path) -> Option<ConfigFileStamp> {
    let metadata = tokio::fs::metadata(path).await.ok()?;
    let modified = metadata.modified().ok()?;
    Some(ConfigFileStamp {
        modified,
        len: metadata.len(),
    })
}

fn decrypt_optional_secret_for_runtime_reload(
    store: &crate::security::SecretStore,
    value: &mut Option<String>,
    field_name: &str,
) -> Result<()> {
    if let Some(raw) = value.clone() {
        if crate::security::SecretStore::is_encrypted(&raw) {
            *value = Some(
                store
                    .decrypt(&raw)
                    .with_context(|| format!("Failed to decrypt {field_name}"))?,
            );
        }
    }
    Ok(())
}

async fn load_runtime_defaults_from_config_file(path: &Path) -> Result<ChannelRuntimeDefaults> {
    let contents = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let path_owned = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<ChannelRuntimeDefaults> {
        let mut parsed: Config = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse {}", path_owned.display()))?;
        parsed.config_path = path_owned.clone();

        if let Some(sen_dir) = path_owned.parent() {
            let store = crate::security::SecretStore::new(sen_dir, parsed.secrets.encrypt);
            decrypt_optional_secret_for_runtime_reload(
                &store,
                &mut parsed.api_key,
                "config.api_key",
            )?;

            if let Some(ref mut openai) = parsed.tts.openai {
                decrypt_optional_secret_for_runtime_reload(
                    &store,
                    &mut openai.api_key,
                    "config.tts.openai.api_key",
                )?;
            }
            if let Some(ref mut elevenlabs) = parsed.tts.elevenlabs {
                decrypt_optional_secret_for_runtime_reload(
                    &store,
                    &mut elevenlabs.api_key,
                    "config.tts.elevenlabs.api_key",
                )?;
            }
            if let Some(ref mut google) = parsed.tts.google {
                decrypt_optional_secret_for_runtime_reload(
                    &store,
                    &mut google.api_key,
                    "config.tts.google.api_key",
                )?;
            }
        }

        parsed.apply_env_overrides();
        Ok(runtime_defaults_from_config(&parsed))
    })
    .await
    .map_err(|e| anyhow::anyhow!("config reload task failed: {e}"))?
}

async fn maybe_apply_runtime_config_update(ctx: &ChannelRuntimeContext) -> Result<()> {
    let Some(config_path) = runtime_config_path(ctx) else {
        return Ok(());
    };

    let Some(stamp) = config_file_stamp(&config_path).await else {
        return Ok(());
    };

    {
        let store = runtime_config_store()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(state) = store.get(&config_path) {
            if state.last_applied_stamp == Some(stamp) {
                return Ok(());
            }
        }
    }

    let next_defaults = load_runtime_defaults_from_config_file(&config_path).await?;
    let next_default_provider = create_resilient_provider_nonblocking(
        &next_defaults.default_provider,
        next_defaults.api_key.clone(),
        next_defaults.api_url.clone(),
        next_defaults.reliability.clone(),
        ctx.provider_runtime_options.clone(),
    )
    .await?;
    let next_default_provider: Arc<dyn Provider> = Arc::from(next_default_provider);

    if let Err(err) = next_default_provider.warmup().await {
        if crate::providers::reliable::is_non_retryable(&err) {
            tracing::warn!(
                provider = %next_defaults.default_provider,
                model = %next_defaults.model,
                "Rejecting config reload: model not available (non-retryable): {err}"
            );
            return Ok(());
        }
        tracing::warn!(
            provider = %next_defaults.default_provider,
            "Provider warmup failed after config reload (retryable, applying anyway): {err}"
        );
    }

    {
        let mut cache = ctx.provider_cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.clear();
        cache.insert(
            next_defaults.default_provider.clone(),
            Arc::clone(&next_default_provider),
        );
    }

    {
        let mut store = runtime_config_store()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        store.insert(
            config_path.clone(),
            RuntimeConfigState {
                defaults: next_defaults.clone(),
                last_applied_stamp: Some(stamp),
            },
        );
    }

    tracing::info!(
        path = %config_path.display(),
        provider = %next_defaults.default_provider,
        model = %next_defaults.model,
        temperature = next_defaults.temperature,
        "Applied updated channel runtime config from disk"
    );

    Ok(())
}

fn default_route_selection(ctx: &ChannelRuntimeContext) -> ChannelRouteSelection {
    let defaults = runtime_defaults_snapshot(ctx);
    ChannelRouteSelection {
        provider: defaults.default_provider,
        model: defaults.model,
        api_key: None,
    }
}

fn get_route_selection(ctx: &ChannelRuntimeContext, sender_key: &str) -> ChannelRouteSelection {
    ctx.route_overrides
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(sender_key)
        .cloned()
        .unwrap_or_else(|| default_route_selection(ctx))
}

fn set_route_selection(ctx: &ChannelRuntimeContext, sender_key: &str, next: ChannelRouteSelection) {
    let default_route = default_route_selection(ctx);
    let mut routes = ctx
        .route_overrides
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if next == default_route {
        routes.remove(sender_key);
    } else {
        routes.insert(sender_key.to_string(), next);
    }
}

fn clear_sender_history(ctx: &ChannelRuntimeContext, sender_key: &str) {
    ctx.conversation_histories
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(sender_key);
}

fn mark_sender_for_new_session(ctx: &ChannelRuntimeContext, sender_key: &str) {
    ctx.pending_new_sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(sender_key.to_string());
}

fn take_pending_new_session(ctx: &ChannelRuntimeContext, sender_key: &str) -> bool {
    ctx.pending_new_sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(sender_key)
}

fn replace_available_skills_section(base_prompt: &str, refreshed_skills: &str) -> String {
    const SKILLS_HEADER: &str = "## Available Skills\n\n";
    const SKILLS_END: &str = "</available_skills>";
    const WORKSPACE_HEADER: &str = "## Workspace\n\n";

    if let Some(start) = base_prompt.find(SKILLS_HEADER) {
        if let Some(rel_end) = base_prompt[start..].find(SKILLS_END) {
            let end = start + rel_end + SKILLS_END.len();
            let tail = base_prompt[end..]
                .strip_prefix("\n\n")
                .unwrap_or(&base_prompt[end..]);

            let mut refreshed = String::with_capacity(
                base_prompt.len().saturating_sub(end.saturating_sub(start))
                    + refreshed_skills.len()
                    + 2,
            );
            refreshed.push_str(&base_prompt[..start]);
            if !refreshed_skills.is_empty() {
                refreshed.push_str(refreshed_skills);
                refreshed.push_str("\n\n");
            }
            refreshed.push_str(tail);
            return refreshed;
        }
    }

    if refreshed_skills.is_empty() {
        return base_prompt.to_string();
    }

    if let Some(workspace_start) = base_prompt.find(WORKSPACE_HEADER) {
        let mut refreshed = String::with_capacity(base_prompt.len() + refreshed_skills.len() + 2);
        refreshed.push_str(&base_prompt[..workspace_start]);
        refreshed.push_str(refreshed_skills);
        refreshed.push_str("\n\n");
        refreshed.push_str(&base_prompt[workspace_start..]);
        return refreshed;
    }

    format!("{base_prompt}\n\n{refreshed_skills}")
}

fn refreshed_new_session_system_prompt(ctx: &ChannelRuntimeContext) -> String {
    let refreshed_skills = crate::skills::skills_to_prompt_with_mode(
        &crate::skills::load_skills_with_config(
            ctx.workspace_dir.as_ref(),
            ctx.prompt_config.as_ref(),
        ),
        ctx.workspace_dir.as_ref(),
        ctx.prompt_config.skills.prompt_injection_mode,
    );
    replace_available_skills_section(ctx.system_prompt.as_str(), &refreshed_skills)
}

fn compact_sender_history(ctx: &ChannelRuntimeContext, sender_key: &str) -> bool {
    let mut histories = ctx
        .conversation_histories
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let Some(turns) = histories.get_mut(sender_key) else {
        return false;
    };

    if turns.is_empty() {
        return false;
    }

    let keep_from = turns
        .len()
        .saturating_sub(CHANNEL_HISTORY_COMPACT_KEEP_MESSAGES);
    let mut compacted = normalize_cached_channel_turns(turns[keep_from..].to_vec());

    for turn in &mut compacted {
        if turn.content.chars().count() > CHANNEL_HISTORY_COMPACT_CONTENT_CHARS {
            turn.content =
                truncate_with_ellipsis(&turn.content, CHANNEL_HISTORY_COMPACT_CONTENT_CHARS);
        }
    }

    if compacted.is_empty() {
        turns.clear();
        return false;
    }

    *turns = compacted;
    true
}

fn proactive_trim_turns(turns: &mut Vec<ChatMessage>, budget: usize) -> usize {
    let total_chars: usize = turns.iter().map(|t| t.content.chars().count()).sum();
    if total_chars <= budget || turns.len() <= 1 {
        return 0;
    }

    let mut excess = total_chars.saturating_sub(budget);
    let mut drop_count = 0;

    while excess > 0 && drop_count < turns.len().saturating_sub(1) {
        excess = excess.saturating_sub(turns[drop_count].content.chars().count());
        drop_count += 1;
    }

    if drop_count > 0 {
        turns.drain(..drop_count);
    }
    drop_count
}

async fn append_sender_turn(ctx: &ChannelRuntimeContext, sender_key: &str, turn: ChatMessage) {

    if let Some(ref store) = ctx.session_store {
        let store_arc = Arc::clone(store);
        let sender_key_owned = sender_key.to_string();
        let turn_for_disk = turn.clone();
        let join = tokio::task::spawn_blocking(move || {
            store_arc.append(&sender_key_owned, &turn_for_disk)
        })
        .await;
        match join {
            Ok(Err(e)) => {
                tracing::warn!("Failed to persist session turn: {e}");
            }
            Err(e) => {
                tracing::warn!("Failed to persist session turn (join): {e}");
            }
            _ => {}
        }
    }

    let max_history = {
        let configured = ctx.prompt_config.agent.max_history_messages;
        if configured > 0 {
            configured
        } else {
            MAX_CHANNEL_HISTORY
        }
    };

    let mut histories = ctx
        .conversation_histories
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let turns = histories.entry(sender_key.to_string()).or_default();
    turns.push(turn);
    while turns.len() > max_history {
        turns.remove(0);
    }
}

async fn rollback_orphan_user_turn(
    ctx: &ChannelRuntimeContext,
    sender_key: &str,
    expected_content: &str,
) -> bool {
    let store_clone = {
        let mut histories = ctx
            .conversation_histories
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let Some(turns) = histories.get_mut(sender_key) else {
            return false;
        };

        let should_pop = turns
            .last()
            .is_some_and(|turn| turn.role == "user" && turn.content == expected_content);
        if !should_pop {
            return false;
        }

        turns.pop();
        if turns.is_empty() {
            histories.remove(sender_key);
        }

        ctx.session_store.as_ref().map(Arc::clone)
    };

    if let Some(store_arc) = store_clone {
        let sender_key_owned = sender_key.to_string();
        let join = tokio::task::spawn_blocking(move || store_arc.remove_last(&sender_key_owned))
            .await;
        match join {
            Ok(Err(e)) => {
                tracing::warn!("Failed to rollback session store entry: {e}");
            }
            Err(e) => {
                tracing::warn!("Failed to rollback session store entry (join): {e}");
            }
            _ => {}
        }
    }

    true
}

fn should_rollback_failed_user_turn(error: &anyhow::Error) -> bool {
    if error
        .downcast_ref::<providers::ProviderCapabilityError>()
        .is_some_and(|capability| capability.capability.eq_ignore_ascii_case("vision"))
    {
        return true;
    }

    crate::providers::reliable::is_non_retryable(error)
}

fn should_skip_memory_context_entry(key: &str, content: &str) -> bool {
    if memory::is_assistant_autosave_key(key) {
        return true;
    }

    if memory::should_skip_autosave_content(content) {
        return true;
    }

    if key.trim().to_ascii_lowercase().ends_with("_history") {
        return true;
    }

    if content.contains("[IMAGE:") {
        return true;
    }

    if content.contains("<tool_result") {
        return true;
    }

    content.chars().count() > MEMORY_CONTEXT_MAX_CHARS
}

fn is_context_window_overflow_error(err: &anyhow::Error) -> bool {
    let lower = err.to_string().to_lowercase();
    [
        "exceeds the context window",
        "context window of this model",
        "maximum context length",
        "context length exceeded",
        "too many tokens",
        "token limit exceeded",
        "prompt is too long",
        "input is too long",
    ]
    .iter()
    .any(|hint| lower.contains(hint))
}

fn load_cached_model_preview(workspace_dir: &Path, provider_name: &str) -> Vec<String> {
    let cache_path = workspace_dir.join("state").join(MODEL_CACHE_FILE);
    let Ok(raw) = std::fs::read_to_string(cache_path) else {
        return Vec::new();
    };
    let Ok(state) = serde_json::from_str::<ModelCacheState>(&raw) else {
        return Vec::new();
    };

    state
        .entries
        .into_iter()
        .find(|entry| entry.provider == provider_name)
        .map(|entry| {
            entry
                .models
                .into_iter()
                .take(MODEL_CACHE_PREVIEW_LIMIT)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn provider_cache_key(provider_name: &str, route_api_key: Option<&str>) -> String {
    match route_api_key {
        Some(key) => {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            key.hash(&mut hasher);
            format!("{provider_name}@{:x}", hasher.finish())
        }
        None => provider_name.to_string(),
    }
}

async fn get_or_create_provider(
    ctx: &ChannelRuntimeContext,
    provider_name: &str,
    route_api_key: Option<&str>,
) -> anyhow::Result<Arc<dyn Provider>> {
    let cache_key = provider_cache_key(provider_name, route_api_key);

    if let Some(existing) = ctx
        .provider_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&cache_key)
        .cloned()
    {
        return Ok(existing);
    }

    if route_api_key.is_none() && provider_name == ctx.default_provider.as_str() {
        return Ok(Arc::clone(&ctx.provider));
    }

    let defaults = runtime_defaults_snapshot(ctx);
    let api_url = if provider_name == defaults.default_provider.as_str() {
        defaults.api_url.as_deref()
    } else {
        None
    };

    let effective_api_key = route_api_key
        .map(ToString::to_string)
        .or_else(|| ctx.api_key.clone());

    let provider = create_resilient_provider_nonblocking(
        provider_name,
        effective_api_key,
        api_url.map(ToString::to_string),
        ctx.reliability.as_ref().clone(),
        ctx.provider_runtime_options.clone(),
    )
    .await?;
    let provider: Arc<dyn Provider> = Arc::from(provider);

    if let Err(err) = provider.warmup().await {
        tracing::warn!(provider = provider_name, "Provider warmup failed: {err}");
    }

    let mut cache = ctx.provider_cache.lock().unwrap_or_else(|e| e.into_inner());
    let cached = cache
        .entry(cache_key)
        .or_insert_with(|| Arc::clone(&provider));
    Ok(Arc::clone(cached))
}

async fn create_resilient_provider_nonblocking(
    provider_name: &str,
    api_key: Option<String>,
    api_url: Option<String>,
    reliability: crate::config::ReliabilityConfig,
    provider_runtime_options: providers::ProviderRuntimeOptions,
) -> anyhow::Result<Box<dyn Provider>> {
    let provider_name = provider_name.to_string();
    tokio::task::spawn_blocking(move || {
        providers::create_resilient_provider_with_options(
            &provider_name,
            api_key.as_deref(),
            api_url.as_deref(),
            &reliability,
            &provider_runtime_options,
        )
    })
    .await
    .context("failed to join provider initialization task")?
}

fn build_models_help_response(
    current: &ChannelRouteSelection,
    workspace_dir: &Path,
    model_routes: &[crate::config::ModelRouteConfig],
) -> String {
    let mut response = String::new();
    let _ = writeln!(
        response,
        "Current provider: `{}`\nCurrent model: `{}`",
        current.provider, current.model
    );
    response.push_str("\nSwitch model with `/model <model-id>` or `/model <hint>`.\n");

    if !model_routes.is_empty() {
        response.push_str("\nConfigured model routes:\n");
        for route in model_routes {
            let _ = writeln!(
                response,
                "  `{}` → {} ({})",
                route.hint, route.model, route.provider
            );
        }
    }

    let cached_models = load_cached_model_preview(workspace_dir, &current.provider);
    if cached_models.is_empty() {
        let _ = writeln!(
            response,
            "\nNo cached model list found for `{}`. Ask the operator to run `sen models refresh --provider {}`.",
            current.provider, current.provider
        );
    } else {
        let _ = writeln!(
            response,
            "\nCached model IDs (top {}):",
            cached_models.len()
        );
        for model in cached_models {
            let _ = writeln!(response, "- `{model}`");
        }
    }

    response
}

fn build_providers_help_response(current: &ChannelRouteSelection) -> String {
    let mut response = String::new();
    let _ = writeln!(
        response,
        "Current provider: `{}`\nCurrent model: `{}`",
        current.provider, current.model
    );
    response.push_str("\nSwitch provider with `/models <provider>`.\n");
    response.push_str("Switch model with `/model <model-id>`.\n\n");
    response.push_str("Available providers:\n");
    for provider in providers::list_providers() {
        if provider.aliases.is_empty() {
            let _ = writeln!(response, "- {}", provider.name);
        } else {
            let _ = writeln!(
                response,
                "- {} (aliases: {})",
                provider.name,
                provider.aliases.join(", ")
            );
        }
    }
    response
}

fn build_config_text_response(
    current: &ChannelRouteSelection,
    _workspace_dir: &Path,
    model_routes: &[crate::config::ModelRouteConfig],
) -> String {
    let mut resp = String::new();
    let _ = writeln!(
        resp,
        "Current provider: `{}`\nCurrent model: `{}`",
        current.provider, current.model
    );
    resp.push_str("\nAvailable providers:\n");
    for p in providers::list_providers() {
        let _ = writeln!(resp, "- `{}`", p.name);
    }
    if !model_routes.is_empty() {
        resp.push_str("\nConfigured model routes:\n");
        for route in model_routes {
            let _ = writeln!(
                resp,
                "  `{}` -> {} ({})",
                route.hint, route.model, route.provider
            );
        }
    }
    resp.push_str(
        "\nUse `/models <provider>` to switch provider.\nUse `/model <model-id>` to switch model.",
    );
    resp
}

const BLOCK_KIT_PREFIX: &str = "__SEN_BLOCK_KIT__";

fn build_config_block_kit(
    current: &ChannelRouteSelection,
    workspace_dir: &Path,
    model_routes: &[crate::config::ModelRouteConfig],
) -> String {
    let provider_options: Vec<serde_json::Value> = providers::list_providers()
        .iter()
        .map(|p| {
            serde_json::json!({
                "text": { "type": "plain_text", "text": p.display_name },
                "value": p.name
            })
        })
        .collect();

    let mut model_options: Vec<serde_json::Value> = model_routes
        .iter()
        .map(|r| {
            let label = if r.hint.is_empty() {
                r.model.clone()
            } else {
                format!("{} ({})", r.model, r.hint)
            };
            serde_json::json!({
                "text": { "type": "plain_text", "text": label },
                "value": r.model
            })
        })
        .collect();

    let cached = load_cached_model_preview(workspace_dir, &current.provider);
    for model_id in cached {
        if !model_options.iter().any(|o| {
            o.get("value")
                .and_then(|v| v.as_str())
                .is_some_and(|v| v == model_id)
        }) {
            model_options.push(serde_json::json!({
                "text": { "type": "plain_text", "text": model_id },
                "value": model_id
            }));
        }
    }

    if !model_options.iter().any(|o| {
        o.get("value")
            .and_then(|v| v.as_str())
            .is_some_and(|v| v == current.model)
    }) {
        model_options.insert(
            0,
            serde_json::json!({
                "text": { "type": "plain_text", "text": &current.model },
                "value": &current.model
            }),
        );
    }

    let initial_provider = provider_options
        .iter()
        .find(|o| {
            o.get("value")
                .and_then(|v| v.as_str())
                .is_some_and(|v| v == current.provider)
        })
        .cloned();

    let initial_model = model_options
        .iter()
        .find(|o| {
            o.get("value")
                .and_then(|v| v.as_str())
                .is_some_and(|v| v == current.model)
        })
        .cloned();

    let mut provider_select = serde_json::json!({
        "type": "static_select",
        "action_id": "sen_config_provider",
        "placeholder": { "type": "plain_text", "text": "Select provider" },
        "options": provider_options
    });
    if let Some(init) = initial_provider {
        provider_select["initial_option"] = init;
    }

    let mut model_select = serde_json::json!({
        "type": "static_select",
        "action_id": "sen_config_model",
        "placeholder": { "type": "plain_text", "text": "Select model" },
        "options": model_options
    });
    if let Some(init) = initial_model {
        model_select["initial_option"] = init;
    }

    let blocks = serde_json::json!([
        {
            "type": "section",
            "text": {
                "type": "mrkdwn",
                "text": format!(
                    "*Model Configuration*\nCurrent: `{}` / `{}`",
                    current.provider, current.model
                )
            }
        },
        {
            "type": "section",
            "block_id": "config_provider_block",
            "text": { "type": "mrkdwn", "text": "*Provider*" },
            "accessory": provider_select
        },
        {
            "type": "section",
            "block_id": "config_model_block",
            "text": { "type": "mrkdwn", "text": "*Model*" },
            "accessory": model_select
        }
    ]);

    blocks.to_string()
}

async fn handle_runtime_command_if_needed(
    ctx: &ChannelRuntimeContext,
    msg: &traits::ChannelMessage,
    target_channel: Option<&Arc<dyn Channel>>,
) -> bool {
    let Some(command) = parse_runtime_command(&msg.channel, &msg.content) else {
        return false;
    };

    let Some(channel) = target_channel else {
        return true;
    };

    let sender_key = conversation_history_key(msg);
    let mut current = get_route_selection(ctx, &sender_key);

    let response = match command {
        ChannelRuntimeCommand::ShowProviders => build_providers_help_response(&current),
        ChannelRuntimeCommand::SetProvider(raw_provider) => {
            match resolve_provider_alias(&raw_provider) {
                Some(provider_name) => {
                    match get_or_create_provider(ctx, &provider_name, None).await {
                        Ok(_) => {
                            if provider_name != current.provider {
                                current.provider = provider_name.clone();
                                set_route_selection(ctx, &sender_key, current.clone());
                            }

                            format!(
                                "Provider switched to `{provider_name}` for this sender session. Current model is `{}`.\nUse `/model <model-id>` to set a provider-compatible model.",
                                current.model
                            )
                        }
                        Err(err) => {
                            let safe_err = providers::sanitize_api_error(&err.to_string());
                            format!(
                                "Failed to initialize provider `{provider_name}`. Route unchanged.\nDetails: {safe_err}"
                            )
                        }
                    }
                }
                None => format!(
                    "Unknown provider `{raw_provider}`. Use `/models` to list valid providers."
                ),
            }
        }
        ChannelRuntimeCommand::ShowModel => {
            let current = current.clone();
            let workspace_dir = Arc::clone(&ctx.workspace_dir);
            let model_routes = Arc::clone(&ctx.model_routes);
            tokio::task::spawn_blocking(move || {
                build_models_help_response(&current, workspace_dir.as_path(), model_routes.as_slice())
            })
            .await
            .unwrap_or_default()
        }
        ChannelRuntimeCommand::SetModel(raw_model) => {
            let model = raw_model.trim().trim_matches('`').to_string();
            if model.is_empty() {
                "Model ID cannot be empty. Use `/model <model-id>`.".to_string()
            } else {

                if let Some(route) = ctx.model_routes.iter().find(|r| {
                    r.model.eq_ignore_ascii_case(&model) || r.hint.eq_ignore_ascii_case(&model)
                }) {
                    current.provider = route.provider.clone();
                    current.model = route.model.clone();
                    current.api_key = route.api_key.clone();
                } else {
                    current.model = model.clone();
                }
                set_route_selection(ctx, &sender_key, current.clone());

                format!(
                    "Model switched to `{}` (provider: `{}`). Context preserved.",
                    current.model, current.provider
                )
            }
        }
        ChannelRuntimeCommand::ShowConfig => {
            let current = current.clone();
            let workspace_dir = Arc::clone(&ctx.workspace_dir);
            let model_routes = Arc::clone(&ctx.model_routes);
            let is_slack = msg.channel == "slack";
            tokio::task::spawn_blocking(move || {
                if is_slack {
                    let blocks_json = build_config_block_kit(
                        &current,
                        workspace_dir.as_path(),
                        model_routes.as_slice(),
                    );
                    format!("__SEN_BLOCK_KIT__{blocks_json}")
                } else {
                    build_config_text_response(
                        &current,
                        workspace_dir.as_path(),
                        model_routes.as_slice(),
                    )
                }
            })
            .await
            .unwrap_or_default()
        }
        ChannelRuntimeCommand::NewSession => {
            clear_sender_history(ctx, &sender_key);
            if let Some(store) = ctx.session_store.as_ref().map(Arc::clone) {
                let key = sender_key.clone();
                match tokio::task::spawn_blocking(move || store.delete_session(&key)).await {
                    Ok(Err(e)) => {
                        tracing::warn!("Failed to delete persisted session for {sender_key}: {e}");
                    }
                    Err(e) => {
                        tracing::warn!("delete_session task failed for {sender_key}: {e}");
                    }
                    Ok(Ok(_)) => {}
                }
            }
            mark_sender_for_new_session(ctx, &sender_key);
            "Conversation history cleared. Starting fresh.".to_string()
        }
    };

    if let Err(err) = channel
        .send(&SendMessage::new(response, &msg.reply_target).in_thread(msg.thread_ts.clone()))
        .await
    {
        tracing::warn!(
            "Failed to send runtime command response on {}: {err}",
            channel.name()
        );
    }

    true
}

pub fn build_system_prompt(
    workspace_dir: &std::path::Path,
    model: &str,
    tool_descs: &[(&str, &str)],
    skills: &[crate::skills::Skill],
    identity: Option<&crate::config::IdentityConfig>,
    bootstrap_max_chars: Option<usize>,
) -> String {
    build_system_prompt_with_mode_and_autonomy(
        workspace_dir,
        model,
        tool_descs,
        skills,
        identity,
        bootstrap_max_chars,
        None,
        false,
        crate::config::SkillsPromptInjectionMode::default(),
        false,
        0,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_system_prompt_with_mode_and_autonomy(
    workspace_dir: &std::path::Path,
    model: &str,
    tool_descs: &[(&str, &str)],
    skills: &[crate::skills::Skill],
    identity: Option<&crate::config::IdentityConfig>,
    bootstrap_max_chars: Option<usize>,
    autonomy: Option<&crate::config::AutonomyConfig>,
    native_tools: bool,
    skills_prompt_mode: crate::config::SkillsPromptInjectionMode,
    _compact_context: bool,
    max_system_prompt_chars: usize,
    agent_config: Option<&crate::config::AgentConfig>,
    coding_mode_label: Option<&str>,
) -> String {
    use crate::agent::prompt::{PromptContext, SystemPromptBuilder};

    let autonomy_level = autonomy
        .map(|a| a.level)
        .unwrap_or(AutonomyLevel::Supervised);

    let directives: &[crate::config::schema::GlobalDirective] = agent_config
        .map(|c| c.global_directives.as_slice())
        .unwrap_or(&[]);

    let ctx = PromptContext {
        workspace_dir,
        model_name: model,
        tools: &[],
        allowed_tool_names: None,
        skills,
        skills_prompt_mode,
        identity_config: identity,
        dispatcher_instructions: "",
        tool_descriptions: None,
        security_summary: None,
        autonomy_level,
        global_directives: directives,
        coding_mode_label,
    };

    let builder = SystemPromptBuilder::with_defaults();
    let mut prompt = builder.build(&ctx).unwrap_or_default();

    if !native_tools && !tool_descs.is_empty() {
        prompt.push_str("## Available Tools\n\n");
        for (name, desc) in tool_descs {
            let _ = std::fmt::Write::write_fmt(
                &mut prompt,
                format_args!("- **{}**: {}\n", name, desc),
            );
        }
        prompt.push('\n');
    }

    let char_limit = if max_system_prompt_chars > 0 {
        max_system_prompt_chars
    } else if let Some(bmc) = bootstrap_max_chars {
        bmc
    } else {
        BOOTSTRAP_MAX_CHARS
    };

    if char_limit > 0 && prompt.chars().count() > char_limit {
        truncate_with_ellipsis(&prompt, char_limit)
    } else {
        prompt
    }
}

pub async fn start_channels(config: Config) -> anyhow::Result<()> {
    let workspace_dir = config.workspace_dir.clone();

    let security = Arc::new(SecurityPolicy::from_config(&config.autonomy, &workspace_dir));

    let rt_adapter: Arc<dyn runtime::RuntimeAdapter> =
        Arc::new(runtime::NativeRuntime::new());
    let skills_for_tools = crate::skills::load_skills_with_config(&workspace_dir, &config);
    let mut tools_vec =
        tools::default_tools_with_runtime(Arc::clone(&security), Arc::clone(&rt_adapter));
    tools::register_skill_tools(&mut tools_vec, &skills_for_tools, Arc::clone(&security));
    let tools_registry: Arc<Vec<Box<dyn Tool>>> = Arc::new(tools_vec);

    let mem = memory::create_memory_with_storage_and_routes_async(
        config.memory.clone(),
        config.embedding_routes.clone(),
        Some(config.storage.provider.config.clone()),
        workspace_dir.clone(),
        config.api_key.clone(),
    )
    .await
    .context("Failed to initialise memory backend")?;
    let mem_arc: Arc<dyn Memory> = Arc::from(mem);

    let observer = observability::create_observer(&config.observability);
    let observer: Arc<dyn Observer> = Arc::from(observer);

    let default_provider_name = resolved_default_provider(&config);
    let resolved_runtime_provider_name =
        providers::resolve_runtime_provider_name(&default_provider_name, &config);
    let default_model = resolved_default_model(&config);
    let provider_opts = providers::provider_runtime_options_from_config(&config);
    let provider = create_resilient_provider_nonblocking(
        &resolved_runtime_provider_name,
        config.api_key.clone(),
        config.api_url.clone(),
        config.reliability.clone(),
        provider_opts.clone(),
    )
    .await
    .context("Failed to create provider")?;

    let hooks: Option<Arc<crate::hooks::HookRunner>> = if config.hooks.enabled {
        let mut runner = crate::hooks::HookRunner::new();
        runner.register(Box::new(
            crate::hooks::builtin::command_logger::CommandLoggerHook::new(),
        ));
        runner.register(Box::new(
            crate::hooks::builtin::webhook_audit::WebhookAuditHook::new(
                crate::config::schema::WebhookAuditConfig::default(),
            )
            .expect("default WebhookAuditConfig has empty URL and must construct successfully"),
        ));
        Some(Arc::new(runner))
    } else {
        None
    };

    let rbac_engine: Option<Arc<crate::security::rbac::RbacEngine>> =
        if config.rbac.enabled {
            Some(Arc::new(crate::security::rbac::RbacEngine::new(
                config.rbac.clone(),
                &workspace_dir,
            )))
        } else {
            None
        };

    let approval_manager = Arc::new({
        let audit_path = config
            .config_path
            .parent()
            .map(|p| p.join("approval_audit.jsonl"));
        let mut mgr = ApprovalManager::from_config(&config.autonomy);
        if let Some(p) = audit_path {
            mgr = mgr.with_audit_log_path(p);
        }
        mgr
    });

    let skills_for_prompt = crate::skills::load_skills_with_config(&workspace_dir, &config);
    let bootstrap_max_chars = if config.agent.compact_context {
        Some(6000usize)
    } else {
        None
    };
    let system_prompt = build_system_prompt_with_mode_and_autonomy(
        &workspace_dir,
        &default_model,
        &[],
        &skills_for_prompt,
        Some(&config.identity),
        bootstrap_max_chars,
        Some(&config.autonomy),
        false,
        config.skills.prompt_injection_mode,
        config.agent.compact_context,
        config.agent.max_system_prompt_chars,
        Some(&config.agent),
        None,
    );

    let mut channels_map: HashMap<String, Arc<dyn Channel>> = HashMap::new();

    if config.channels_config.cli {
        let ch = Arc::new(CliChannel::new());
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    #[cfg(feature = "channel-telegram")]
    if let Some(ref cfg) = config.channels_config.telegram {
        let ch = Arc::new(TelegramChannel::new(
            cfg.bot_token.clone(),
            cfg.allowed_users.clone(),
            false,
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    #[cfg(feature = "channel-slack")]
    if let Some(ref cfg) = config.channels_config.slack {
        let ch = Arc::new(SlackChannel::new(
            cfg.bot_token.clone(),
            cfg.app_token.clone(),
            cfg.channel_id.clone(),
            cfg.channel_ids.clone(),
            cfg.allowed_users.clone(),
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    #[cfg(feature = "channel-discord")]
    if let Some(ref cfg) = config.channels_config.discord {
        let ch = Arc::new(DiscordChannel::new(
            cfg.bot_token.clone(),
            cfg.guild_id.clone(),
            cfg.allowed_users.clone(),
            cfg.listen_to_bots,
            cfg.mention_only,
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    #[cfg(feature = "channel-discord")]
    if let Some(ref cfg) = config.channels_config.discord_history {
        let ch = Arc::new(DiscordHistoryChannel::new(
            cfg.bot_token.clone(),
            cfg.guild_id.clone(),
            cfg.allowed_users.clone(),
            cfg.channel_ids.clone(),
            Arc::clone(&mem_arc),
            cfg.store_dms,
            cfg.respond_to_dms,
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.mattermost {
        let ch = Arc::new(MattermostChannel::new(
            cfg.url.clone(),
            cfg.bot_token.clone(),
            cfg.channel_id.clone(),
            cfg.allowed_users.clone(),
            cfg.thread_replies.unwrap_or(true),
            cfg.mention_only.unwrap_or(false),
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.webhook {
        let ch = Arc::new(WebhookChannel::new(
            cfg.port,
            cfg.listen_path.clone(),
            cfg.send_url.clone(),
            cfg.send_method.clone(),
            cfg.auth_header.clone(),
            cfg.secret.clone(),
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.signal {
        let ch = Arc::new(SignalChannel::new(
            cfg.http_url.clone(),
            cfg.account.clone(),
            cfg.group_id.clone(),
            cfg.allowed_from.clone(),
            cfg.ignore_attachments,
            cfg.ignore_stories,
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.whatsapp {
        let ch = Arc::new(WhatsAppChannel::new(
            cfg.access_token.clone().unwrap_or_default(),
            cfg.phone_number_id.clone().unwrap_or_default(),
            cfg.verify_token.clone().unwrap_or_default(),
            cfg.allowed_numbers.clone(),
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.qq {
        let ch = Arc::new(QQChannel::new(
            cfg.app_id.clone(),
            cfg.app_secret.clone(),
            cfg.allowed_users.clone(),
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    #[cfg(feature = "channel-dingtalk")]
    if let Some(ref cfg) = config.channels_config.dingtalk {
        let ch = Arc::new(DingTalkChannel::new(
            cfg.client_id.clone(),
            cfg.client_secret.clone(),
            cfg.allowed_users.clone(),
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    #[cfg(feature = "channel-wechat")]
    if let Some(ref cfg) = config.channels_config.wecom {
        let ch = Arc::new(WeComChannel::new(
            cfg.webhook_key.clone(),
            cfg.allowed_users.clone(),
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.twitter {
        let ch = Arc::new(TwitterChannel::new(
            cfg.bearer_token.clone(),
            cfg.allowed_users.clone(),
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.bluesky {
        let ch = Arc::new(BlueskyChannel::new(
            cfg.handle.clone(),
            cfg.app_password.clone(),
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.reddit {
        let ch = Arc::new(RedditChannel::new(
            cfg.client_id.clone(),
            cfg.client_secret.clone(),
            cfg.refresh_token.clone(),
            cfg.username.clone(),
            cfg.subreddit.clone(),
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.irc {
        let ch = Arc::new(IrcChannel::new(crate::channels::irc::IrcChannelConfig {
            server: cfg.server.clone(),
            port: cfg.port,
            nickname: cfg.nickname.clone(),
            username: cfg.username.clone(),
            channels: cfg.channels.clone(),
            allowed_users: cfg.allowed_users.clone(),
            server_password: cfg.server_password.clone(),
            nickserv_password: cfg.nickserv_password.clone(),
            sasl_password: cfg.sasl_password.clone(),
            verify_tls: cfg.verify_tls.unwrap_or(true),
        }));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    #[cfg(feature = "channel-email")]
    if let Some(ref cfg) = config.channels_config.email {
        let ch = Arc::new(EmailChannel::new(cfg.clone()));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    #[cfg(feature = "channel-email")]
    if let Some(ref cfg) = config.channels_config.gmail_push {
        let ch = Arc::new(GmailPushChannel::new(cfg.clone()));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.nextcloud_talk {
        let ch = Arc::new(NextcloudTalkChannel::new(
            cfg.base_url.clone(),
            cfg.app_token.clone(),
            "bot".to_string(),
            cfg.allowed_users.clone(),
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.linq {
        let ch = Arc::new(LinqChannel::new(
            cfg.api_token.clone(),
            cfg.from_phone.clone(),
            cfg.allowed_senders.clone(),
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.wati {
        let ch = Arc::new(WatiChannel::new(
            cfg.api_token.clone(),
            cfg.api_url.clone(),
            cfg.tenant_id.clone(),
            cfg.allowed_numbers.clone(),
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.mochat {
        let ch = Arc::new(MochatChannel::new(
            cfg.api_url.clone(),
            cfg.api_token.clone(),
            cfg.allowed_users.clone(),
            cfg.poll_interval_secs,
        ));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.telnyx {
        let ch = Arc::new(TelnyxChannel::new(cfg.clone()));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    if let Some(ref cfg) = config.channels_config.imessage {
        let ch = Arc::new(IMessageChannel::new(cfg.allowed_contacts.clone()));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    #[cfg(feature = "channel-matrix")]
    if let Some(ref cfg) = config.channels_config.matrix {
        let ch = Arc::new(crate::channels::MatrixChannel::new(cfg.clone()));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    #[cfg(feature = "channel-lark")]
    if let Some(ref cfg) = config.channels_config.lark {
        let ch = Arc::new(LarkChannel::from_config(cfg));
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }
    #[cfg(feature = "channel-nostr")]
    if let Some(ref cfg) = config.channels_config.nostr {
        let ch = Arc::new(
            crate::channels::NostrChannel::new(
                &cfg.private_key,
                cfg.relays.clone(),
                &cfg.allowed_pubkeys,
            )
            .await
            .context("Failed to initialize Nostr channel")?,
        );
        channels_map.insert(ch.name().to_string(), ch as Arc<dyn Channel>);
    }

    if channels_map.is_empty() {
        tracing::warn!("No channels configured; start_channels exiting immediately");
        return Ok(());
    }

    let session_store: Option<Arc<session::store::SessionStore>> =
        if config.channels_config.session_persistence {
            let store_workspace = workspace_dir.clone();
            let store = tokio::task::spawn_blocking(move || {
                session::store::SessionStore::new(&store_workspace)
            })
            .await
            .context("Failed to join session store init task")?
            .context("Failed to open session store")?;
            Some(Arc::new(store))
        } else {
            None
        };

    let debouncer = Arc::new(pipeline::debounce::MessageDebouncer::new(
        std::time::Duration::from_millis(config.channels_config.debounce_ms),
    ));

    let ctx = ChannelRuntimeContext {
        channels_by_name: Arc::new(channels_map.clone()),
        provider: Arc::from(provider),
        default_provider: Arc::new(default_provider_name),
        prompt_config: Arc::new(config.clone()),
        memory: Arc::clone(&mem_arc),
        tools_registry: Arc::clone(&tools_registry),
        observer: Arc::clone(&observer),
        system_prompt: Arc::new(system_prompt),
        model: Arc::new(default_model),
        temperature: config.default_temperature,
        auto_save_memory: config.memory.auto_save,
        max_tool_iterations: config.agent.max_tool_iterations,
        min_relevance_score: config.memory.min_relevance_score,
        conversation_histories: Arc::new(Mutex::new(HashMap::new())),
        pending_new_sessions: Arc::new(Mutex::new(HashSet::new())),
        provider_cache: Arc::new(Mutex::new(HashMap::new())),
        route_overrides: Arc::new(Mutex::new(HashMap::new())),
        api_key: config.api_key.clone(),
        api_url: config.api_url.clone(),
        reliability: Arc::new(config.reliability.clone()),
        provider_runtime_options: provider_opts,
        workspace_dir: Arc::new(workspace_dir.clone()),
        message_timeout_secs: effective_channel_message_timeout_secs(
            config.channels_config.message_timeout_secs,
        ),
        interrupt_on_new_message: interrupt_on_new_message_from_config(&config),
        multimodal: config.multimodal.clone(),
        media_pipeline: config.media_pipeline.clone(),
        transcription_config: config.transcription.clone(),
        hooks,
        non_cli_excluded_tools: Arc::new(config.autonomy.non_cli_excluded_tools.clone()),
        autonomy_level: config.autonomy.level,
        tool_call_dedup_exempt: Arc::new(config.agent.tool_call_dedup_exempt.clone()),
        model_routes: Arc::new(config.model_routes.clone()),
        query_classification: config.query_classification.clone(),
        ack_reactions: config.channels_config.ack_reactions,
        show_tool_calls: config.channels_config.show_tool_calls,
        session_store,
        approval_manager,
        activated_tools: None,
        cost_tracking: None,
        pacing: config.pacing.clone(),
        debouncer,
        rbac_engine,
    };

    let in_flight: Arc<Mutex<HashMap<String, InFlightSenderTaskState>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let (msg_tx, mut msg_rx) =
        tokio::sync::mpsc::channel::<traits::ChannelMessage>(CHANNEL_MAX_IN_FLIGHT_MESSAGES);

    for channel in channels_map.values() {
        let ch = Arc::clone(channel);
        let tx = msg_tx.clone();
        runtime::spawn_supervised(
            format!("channel-listener-{}", ch.name()),
            async move {
                if let Err(e) = ch.listen(tx).await {
                    tracing::warn!("Channel listener '{}' stopped: {e}", ch.name());
                }
            },
        );
    }
    drop(msg_tx);

    tracing::info!("Channel runtime started with {} channel(s)", channels_map.len());

    while let Some(msg) = msg_rx.recv().await {
        if let Err(e) = maybe_apply_runtime_config_update(&ctx).await {
            tracing::warn!("Channel runtime config reload skipped: {e}");
        }

        let target_channel = ctx.channels_by_name.get(&msg.channel).cloned();

        if handle_runtime_command_if_needed(&ctx, &msg, target_channel.as_ref()).await {
            continue;
        }

        let sender_key = conversation_history_key(&msg);
        let ctx_clone = ctx.clone();
        let in_flight_clone = Arc::clone(&in_flight);
        let msg_clone = msg.clone();

        if ctx.interrupt_on_new_message.enabled_for_channel(&msg.channel) {
            let prior = {
                let map = in_flight
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                map.get(&sender_key).map(|state| {
                    (
                        state.cancellation.clone(),
                        Arc::clone(&state.completion),
                    )
                })
            };
            if let Some((cancel, completion)) = prior {
                cancel.cancel();
                completion.wait().await;
            }
        }

        let (cancel_token, completion) = {
            let mut map = in_flight
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let next_id = map
                .get(&sender_key)
                .map(|s| s.task_id + 1)
                .unwrap_or(0);
            let cancel = CancellationToken::new();
            let completion = Arc::new(InFlightTaskCompletion::new());
            map.insert(
                sender_key.clone(),
                InFlightSenderTaskState {
                    task_id: next_id,
                    cancellation: cancel.clone(),
                    completion: Arc::clone(&completion),
                },
            );
            (cancel, completion)
        };

        let sender_key_clone = sender_key.clone();
        runtime::spawn_supervised(
            format!("channel-turn-{sender_key}"),
            async move {
                let finish = |in_flight: &Arc<Mutex<HashMap<String, InFlightSenderTaskState>>>,
                              sender_key: &str,
                              completion: &Arc<InFlightTaskCompletion>| {
                    completion.mark_done();
                    in_flight
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(sender_key);
                };

                let channel = ctx_clone.channels_by_name.get(&msg_clone.channel).cloned();
                let route = get_route_selection(&ctx_clone, &sender_key_clone);
                let provider_name = route.provider.as_str();
                let model = route.model.clone();
                let provider = match get_or_create_provider(
                    &ctx_clone,
                    provider_name,
                    route.api_key.as_deref(),
                )
                .await
                {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!("Failed to get provider for channel turn: {e}");
                        finish(&in_flight_clone, &sender_key_clone, &completion);
                        return;
                    }
                };

                compact_sender_history(&ctx_clone, &sender_key_clone);
                {
                    let mut map = ctx_clone
                        .conversation_histories
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if let Some(turns) = map.get_mut(&sender_key_clone) {
                        proactive_trim_turns(turns, PROACTIVE_CONTEXT_BUDGET_CHARS);
                    }
                }

                let new_session = take_pending_new_session(&ctx_clone, &sender_key_clone);

                let mut history = {
                    let map = ctx_clone
                        .conversation_histories
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    map.get(&sender_key_clone).cloned().unwrap_or_default()
                };

                if history.is_empty() || new_session {
                    if new_session {
                        history.clear();
                    }
                    let sys = refreshed_new_session_system_prompt(&ctx_clone);
                    let sys = build_channel_system_prompt(
                        &sys,
                        &msg_clone.channel,
                        &msg_clone.reply_target,
                    );
                    history.insert(0, ChatMessage::system(sys));
                }

                let memory_ctx = build_memory_context(
                    ctx_clone.memory.as_ref(),
                    &msg_clone.content,
                    ctx_clone.min_relevance_score,
                    Some(&sender_key_clone),
                )
                .await;
                let user_content = if memory_ctx.is_empty() {
                    msg_clone.content.clone()
                } else {
                    format!("{memory_ctx}{}", msg_clone.content)
                };

                let user_turn = ChatMessage::user(user_content.clone());
                history.push(user_turn.clone());
                append_sender_turn(&ctx_clone, &sender_key_clone, user_turn).await;

                let timeout_budget = channel_message_timeout_budget_secs(
                    ctx_clone.message_timeout_secs,
                    ctx_clone.max_tool_iterations,
                );

                let mut overflow_retried = false;
                loop {
                    let channel_policy = crate::agent::loop_::policy::PolicyBundle::channel(
                        &msg_clone.channel,
                        Some(msg_clone.reply_target.as_str()),
                        provider.as_ref(),
                        ctx_clone.tools_registry.as_ref(),
                        ctx_clone.observer.as_ref(),
                        provider_name,
                        &model,
                        &ctx_clone.multimodal,
                        &ctx_clone.pacing,
                        &ctx_clone.non_cli_excluded_tools,
                        &ctx_clone.tool_call_dedup_exempt,
                    )
                    .with_temperature(ctx_clone.temperature)
                    .with_silent(false)
                    .with_approval(Some(ctx_clone.approval_manager.as_ref()))
                    .with_max_iterations(ctx_clone.max_tool_iterations)
                    .with_hooks(ctx_clone.hooks.as_deref())
                    .with_activated_tools(ctx_clone.activated_tools.as_ref())
                    .with_rbac(ctx_clone.rbac_engine.as_ref(), None)
                    .with_cancellation(Some(cancel_token.clone()));

                    let run_future = crate::agent::loop_::unified::UnifiedLoop::new(
                        channel_policy,
                    )
                    .run(&mut history);
                    let timed = tokio::time::timeout(
                        std::time::Duration::from_secs(timeout_budget),
                        run_future,
                    )
                    .await;

                    match timed {
                        Ok(Ok(response)) => {
                            let outbound = sanitize_channel_outbound_response(
                                &response,
                                ctx_clone.show_tool_calls,
                            );
                            let assistant_turn = ChatMessage::assistant(response.clone());
                            {
                                let mut map = ctx_clone
                                    .conversation_histories
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner());
                                let turns = map
                                    .entry(sender_key_clone.clone())
                                    .or_default();
                                turns.push(assistant_turn.clone());
                                while turns.len() > MAX_CHANNEL_HISTORY {
                                    turns.remove(0);
                                }
                            }
                            append_sender_turn(&ctx_clone, &sender_key_clone, assistant_turn)
                                .await;

                            if let Some(ch) = channel.clone() {
                                let reply = traits::SendMessage::new(
                                    outbound,
                                    &msg_clone.reply_target,
                                )
                                .in_thread(msg_clone.thread_ts.clone());
                                if let Err(e) = ch.send(&reply).await {
                                    tracing::warn!(
                                        "Failed to send reply on {}: {e}",
                                        msg_clone.channel
                                    );
                                }
                            }

                            finish(&in_flight_clone, &sender_key_clone, &completion);
                            break;
                        }
                        Ok(Err(e)) => {
                            if is_context_window_overflow_error(&e) && !overflow_retried {
                                overflow_retried = true;
                                if compact_sender_history(&ctx_clone, &sender_key_clone) {
                                    history = {
                                        let map = ctx_clone
                                            .conversation_histories
                                            .lock()
                                            .unwrap_or_else(|e| e.into_inner());
                                        map.get(&sender_key_clone)
                                            .cloned()
                                            .unwrap_or_default()
                                    };
                                    proactive_trim_turns(
                                        &mut history,
                                        PROACTIVE_CONTEXT_BUDGET_CHARS,
                                    );
                                    continue;
                                }
                            }

                            tracing::error!("Channel turn failed for {sender_key_clone}: {e}");
                            if should_rollback_failed_user_turn(&e) {
                                rollback_orphan_user_turn(
                                    &ctx_clone,
                                    &sender_key_clone,
                                    &user_content,
                                )
                                .await;
                            }
                            finish(&in_flight_clone, &sender_key_clone, &completion);
                            break;
                        }
                        Err(_) => {
                            tracing::error!(
                                "Channel turn timed out for {sender_key_clone} after {timeout_budget}s"
                            );
                            if should_rollback_failed_user_turn(&anyhow::anyhow!(
                                "channel turn timed out"
                            )) {
                                rollback_orphan_user_turn(
                                    &ctx_clone,
                                    &sender_key_clone,
                                    &user_content,
                                )
                                .await;
                            }
                            finish(&in_flight_clone, &sender_key_clone, &completion);
                            break;
                        }
                    }
                }
            },
        );
    }

    tracing::info!("Channel message stream ended; start_channels returning");
    Ok(())
}

pub async fn doctor_channels(config: Config) -> anyhow::Result<()> {
    #[derive(Debug)]
    enum DoctorStatus {
        NotConfigured,
        Configured { healthy: bool },
    }

    struct ChannelReport {
        type_name: &'static str,
        display_name: &'static str,
        key_hint: &'static str,
        status: DoctorStatus,
    }

    impl ChannelReport {
        fn icon(&self) -> &'static str {
            match &self.status {
                DoctorStatus::NotConfigured => "⚪",
                DoctorStatus::Configured { healthy: true } => "✅",
                DoctorStatus::Configured { healthy: false } => "❌",
            }
        }

        fn status_label(&self) -> String {
            match &self.status {
                DoctorStatus::NotConfigured => "not configured".to_string(),
                DoctorStatus::Configured { healthy: true } => "configured, healthy".to_string(),
                DoctorStatus::Configured { healthy: false } => {
                    "configured, health check failed".to_string()
                }
            }
        }
    }

    let cfg = &config.channels_config;
    let mut reports: Vec<ChannelReport> = Vec::new();

    reports.push(ChannelReport {
        type_name: "cli",
        display_name: "CLI",
        key_hint: "built-in, no credentials required",
        status: if cfg.cli {
            DoctorStatus::Configured { healthy: true }
        } else {
            DoctorStatus::NotConfigured
        },
    });

    #[cfg(feature = "channel-telegram")]
    if let Some(ref tg) = cfg.telegram {
        let ch = Arc::new(TelegramChannel::new(
            tg.bot_token.clone(),
            tg.allowed_users.clone(),
            false,
        ));
        let healthy = ch.health_check().await;
        reports.push(ChannelReport {
            type_name: "telegram",
            display_name: "Telegram",
            key_hint: "bot_token",
            status: DoctorStatus::Configured { healthy },
        });
    } else {
        reports.push(ChannelReport {
            type_name: "telegram",
            display_name: "Telegram",
            key_hint: "bot_token",
            status: DoctorStatus::NotConfigured,
        });
    }

    #[cfg(feature = "channel-slack")]
    if let Some(ref sl) = cfg.slack {
        let ch = Arc::new(SlackChannel::new(
            sl.bot_token.clone(),
            sl.app_token.clone(),
            sl.channel_id.clone(),
            sl.channel_ids.clone(),
            sl.allowed_users.clone(),
        ));
        let healthy = ch.health_check().await;
        reports.push(ChannelReport {
            type_name: "slack",
            display_name: "Slack",
            key_hint: "bot_token, app_token",
            status: DoctorStatus::Configured { healthy },
        });
    } else {
        reports.push(ChannelReport {
            type_name: "slack",
            display_name: "Slack",
            key_hint: "bot_token, app_token",
            status: DoctorStatus::NotConfigured,
        });
    }

    #[cfg(feature = "channel-discord")]
    if let Some(ref dc) = cfg.discord {
        let ch = Arc::new(DiscordChannel::new(
            dc.bot_token.clone(),
            dc.guild_id.clone(),
            dc.allowed_users.clone(),
            dc.listen_to_bots,
            dc.mention_only,
        ));
        let healthy = ch.health_check().await;
        reports.push(ChannelReport {
            type_name: "discord",
            display_name: "Discord",
            key_hint: "bot_token",
            status: DoctorStatus::Configured { healthy },
        });
    } else {
        reports.push(ChannelReport {
            type_name: "discord",
            display_name: "Discord",
            key_hint: "bot_token",
            status: DoctorStatus::NotConfigured,
        });
    }

    if let Some(ref mm) = cfg.mattermost {
        let ch = Arc::new(MattermostChannel::new(
            mm.url.clone(),
            mm.bot_token.clone(),
            mm.channel_id.clone(),
            mm.allowed_users.clone(),
            mm.thread_replies.unwrap_or(true),
            mm.mention_only.unwrap_or(false),
        ));
        let healthy = ch.health_check().await;
        reports.push(ChannelReport {
            type_name: "mattermost",
            display_name: "Mattermost",
            key_hint: "url, bot_token",
            status: DoctorStatus::Configured { healthy },
        });
    } else {
        reports.push(ChannelReport {
            type_name: "mattermost",
            display_name: "Mattermost",
            key_hint: "url, bot_token",
            status: DoctorStatus::NotConfigured,
        });
    }

    if let Some(ref wa) = cfg.whatsapp {
        let ch = Arc::new(WhatsAppChannel::new(
            wa.access_token.clone().unwrap_or_default(),
            wa.phone_number_id.clone().unwrap_or_default(),
            wa.verify_token.clone().unwrap_or_default(),
            wa.allowed_numbers.clone(),
        ));
        let healthy = ch.health_check().await;
        reports.push(ChannelReport {
            type_name: "whatsapp",
            display_name: "WhatsApp",
            key_hint: "access_token, phone_number_id",
            status: DoctorStatus::Configured { healthy },
        });
    } else {
        reports.push(ChannelReport {
            type_name: "whatsapp",
            display_name: "WhatsApp",
            key_hint: "access_token, phone_number_id",
            status: DoctorStatus::NotConfigured,
        });
    }

    if let Some(ref sg) = cfg.signal {
        let ch = Arc::new(SignalChannel::new(
            sg.http_url.clone(),
            sg.account.clone(),
            sg.group_id.clone(),
            sg.allowed_from.clone(),
            sg.ignore_attachments,
            sg.ignore_stories,
        ));
        let healthy = ch.health_check().await;
        reports.push(ChannelReport {
            type_name: "signal",
            display_name: "Signal",
            key_hint: "http_url, account",
            status: DoctorStatus::Configured { healthy },
        });
    } else {
        reports.push(ChannelReport {
            type_name: "signal",
            display_name: "Signal",
            key_hint: "http_url, account",
            status: DoctorStatus::NotConfigured,
        });
    }

    #[cfg(feature = "channel-email")]
    if let Some(ref em) = cfg.email {
        let ch = Arc::new(EmailChannel::new(em.clone()));
        let healthy = ch.health_check().await;
        reports.push(ChannelReport {
            type_name: "email",
            display_name: "Email",
            key_hint: "smtp_host, imap_host, username, password",
            status: DoctorStatus::Configured { healthy },
        });
    } else {
        reports.push(ChannelReport {
            type_name: "email",
            display_name: "Email",
            key_hint: "smtp_host, imap_host, username, password",
            status: DoctorStatus::NotConfigured,
        });
    }

    if let Some(ref wh) = cfg.webhook {
        let ch = Arc::new(WebhookChannel::new(
            wh.port,
            wh.listen_path.clone(),
            wh.send_url.clone(),
            wh.send_method.clone(),
            wh.auth_header.clone(),
            wh.secret.clone(),
        ));
        let healthy = ch.health_check().await;
        reports.push(ChannelReport {
            type_name: "webhook",
            display_name: "Webhook",
            key_hint: "port, listen_path",
            status: DoctorStatus::Configured { healthy },
        });
    } else {
        reports.push(ChannelReport {
            type_name: "webhook",
            display_name: "Webhook",
            key_hint: "port, listen_path",
            status: DoctorStatus::NotConfigured,
        });
    }

    if let Some(ref irc_cfg) = cfg.irc {
        let ch = Arc::new(IrcChannel::new(crate::channels::irc::IrcChannelConfig {
            server: irc_cfg.server.clone(),
            port: irc_cfg.port,
            nickname: irc_cfg.nickname.clone(),
            username: irc_cfg.username.clone(),
            channels: irc_cfg.channels.clone(),
            allowed_users: irc_cfg.allowed_users.clone(),
            server_password: irc_cfg.server_password.clone(),
            nickserv_password: irc_cfg.nickserv_password.clone(),
            sasl_password: irc_cfg.sasl_password.clone(),
            verify_tls: irc_cfg.verify_tls.unwrap_or(true),
        }));
        let healthy = ch.health_check().await;
        reports.push(ChannelReport {
            type_name: "irc",
            display_name: "IRC",
            key_hint: "server, nickname",
            status: DoctorStatus::Configured { healthy },
        });
    } else {
        reports.push(ChannelReport {
            type_name: "irc",
            display_name: "IRC",
            key_hint: "server, nickname",
            status: DoctorStatus::NotConfigured,
        });
    }

    let configured_count = reports
        .iter()
        .filter(|r| matches!(r.status, DoctorStatus::Configured { .. }))
        .count();
    let healthy_count = reports
        .iter()
        .filter(|r| matches!(r.status, DoctorStatus::Configured { healthy: true }))
        .count();
    let unhealthy_count = reports
        .iter()
        .filter(|r| matches!(r.status, DoctorStatus::Configured { healthy: false }))
        .count();

    println!("Channel Health Report");
    println!("{}", "─".repeat(60));
    for report in &reports {
        let name_col = format!("{:<14}", report.display_name);
        let type_col = format!("({:<12})", report.type_name);
        println!(
            "{} {} {}  {}",
            report.icon(),
            name_col,
            type_col,
            report.status_label()
        );
        if matches!(report.status, DoctorStatus::NotConfigured) {
            println!("    Required keys: {}", report.key_hint);
        }
    }
    println!("{}", "─".repeat(60));
    println!(
        "Summary: {} configured, {} healthy, {} with issues",
        configured_count, healthy_count, unhealthy_count
    );
    if unhealthy_count > 0 {
        println!();
        println!("Tip: run `sen channel list` to review current configuration.");
        if let Some(label) = channel_background_service_status() {
            println!("Background service: {label}");
        }
        print_channel_service_restart_hints();
    }

    Ok(())
}

pub async fn handle_command(
    cmd: crate::ChannelCommands,
    config: &Config,
) -> anyhow::Result<()> {
    match cmd {
        crate::ChannelCommands::List => channel_list(config),
        crate::ChannelCommands::Add {
            channel_type,
            config: cfg_json,
        } => channel_add(config, &channel_type, &cfg_json).await,
        crate::ChannelCommands::Remove { name } => channel_remove(config, &name).await,
        crate::ChannelCommands::BindTelegram { identity } => {
            channel_bind_telegram(config, &identity).await
        }
        crate::ChannelCommands::Send {
            message,
            channel_id,
            recipient,
        } => channel_send(config, &channel_id, &recipient, &message).await,
        crate::ChannelCommands::Start | crate::ChannelCommands::Doctor => {

            unreachable!("invariant: ChannelCommands::Start/Doctor are dispatched in main.rs before reaching channel sub-router")
        }
    }
}

fn channel_list(config: &Config) -> anyhow::Result<()> {
    let cfg = &config.channels_config;
    println!("Configured channels:");
    println!("{}", "─".repeat(50));

    let mut any = false;

    macro_rules! report_channel {
        ($label:expr, $opt:expr) => {
            if let Some(_) = $opt {
                println!("  ✅  {}", $label);
                any = true;
            }
        };
    }

    if cfg.cli {
        println!("  ✅  cli              (built-in, always active)");
        any = true;
    }
    report_channel!("telegram", &cfg.telegram);
    report_channel!("slack", &cfg.slack);
    report_channel!("discord", &cfg.discord);
    report_channel!("discord_history", &cfg.discord_history);
    report_channel!("mattermost", &cfg.mattermost);
    report_channel!("whatsapp", &cfg.whatsapp);
    report_channel!("signal", &cfg.signal);
    report_channel!("webhook", &cfg.webhook);
    report_channel!("email", &cfg.email);
    report_channel!("gmail_push", &cfg.gmail_push);
    report_channel!("irc", &cfg.irc);
    report_channel!("qq", &cfg.qq);
    report_channel!("dingtalk", &cfg.dingtalk);
    report_channel!("wecom", &cfg.wecom);
    report_channel!("twitter", &cfg.twitter);
    report_channel!("bluesky", &cfg.bluesky);
    report_channel!("reddit", &cfg.reddit);
    report_channel!("linq", &cfg.linq);
    report_channel!("wati", &cfg.wati);
    report_channel!("nextcloud_talk", &cfg.nextcloud_talk);
    report_channel!("mochat", &cfg.mochat);
    report_channel!("imessage", &cfg.imessage);
    report_channel!("telnyx", &cfg.telnyx);
    #[cfg(feature = "channel-lark")]
    report_channel!("lark", &cfg.lark);
    #[cfg(feature = "channel-matrix")]
    report_channel!("matrix", &cfg.matrix);
    #[cfg(feature = "channel-nostr")]
    report_channel!("nostr", &cfg.nostr);

    if !any {
        println!("  (no channels configured)");
        println!();
        println!(
            "  Add a channel with: sen channel add <type> '{{\"key\":\"value\",...}}'"
        );
    }
    println!("{}", "─".repeat(50));
    Ok(())
}

async fn channel_add(
    config: &Config,
    channel_type: &str,
    cfg_json: &str,
) -> anyhow::Result<()> {
    let config_path = &config.config_path;

    let raw = if config_path.exists() {
        tokio::fs::read_to_string(config_path)
            .await
            .with_context(|| format!("Failed to read {}", config_path.display()))?
    } else {
        String::new()
    };

    let mut doc: toml::Value = if raw.is_empty() {
        toml::Value::Table(toml::map::Map::new())
    } else {
        toml::from_str(&raw).with_context(|| {
            format!("Failed to parse TOML at {}", config_path.display())
        })?
    };

    let json_val: serde_json::Value =
        serde_json::from_str(cfg_json).context("Channel config must be valid JSON")?;
    let channel_toml: toml::Value = json_to_toml_value(json_val)?;

    let root = doc
        .as_table_mut()
        .context("Config TOML must be a table")?;
    let channels_tbl = root
        .entry("channels_config")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .context("[channels_config] must be a table")?;

    channels_tbl.insert(channel_type.to_string(), channel_toml);

    let serialized =
        toml::to_string_pretty(&doc).context("Failed to serialize updated config")?;

    if let Some(parent) = config_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create dir {}", parent.display()))?;
    }
    tokio::fs::write(config_path, serialized)
        .await
        .with_context(|| format!("Failed to write {}", config_path.display()))?;

    println!(
        "✅ Added [channels_config.{}] to {}",
        channel_type,
        config_path.display()
    );
    println!("   Run `sen channel start` to activate the new channel.");
    Ok(())
}

async fn channel_remove(config: &Config, name: &str) -> anyhow::Result<()> {
    let config_path = &config.config_path;

    let raw = tokio::fs::read_to_string(config_path)
        .await
        .with_context(|| format!("Failed to read {}", config_path.display()))?;

    let mut doc: toml::Value =
        toml::from_str(&raw).with_context(|| {
            format!("Failed to parse TOML at {}", config_path.display())
        })?;

    let root = doc
        .as_table_mut()
        .context("Config TOML must be a table")?;

    let channels_tbl = root
        .get_mut("channels_config")
        .and_then(|v| v.as_table_mut());

    match channels_tbl {
        Some(tbl) => {
            if tbl.remove(name).is_some() {
                let serialized = toml::to_string_pretty(&doc)
                    .context("Failed to serialize updated config")?;
                tokio::fs::write(config_path, serialized)
                    .await
                    .with_context(|| {
                        format!("Failed to write {}", config_path.display())
                    })?;
                println!("✅ Removed [channels_config.{}] from config.", name);
            } else {
                println!(
                    "⚠️  Channel '{}' not found in [channels_config]. Nothing removed.",
                    name
                );
            }
        }
        None => {
            println!(
                "⚠️  No [channels_config] section found in {}. Nothing to remove.",
                config_path.display()
            );
        }
    }

    Ok(())
}

async fn channel_bind_telegram(config: &Config, identity: &str) -> anyhow::Result<()> {
    let config_path = &config.config_path;

    let raw = tokio::fs::read_to_string(config_path)
        .await
        .with_context(|| format!("Failed to read {}", config_path.display()))?;

    let mut doc: toml::Value =
        toml::from_str(&raw).with_context(|| {
            format!("Failed to parse TOML at {}", config_path.display())
        })?;

    let root = doc
        .as_table_mut()
        .context("Config TOML must be a table")?;

    let channels_tbl = root
        .get_mut("channels_config")
        .and_then(|v| v.as_table_mut())
        .context(
            "No [channels_config] section found. Configure Telegram first with \
             `sen channel add telegram '{\"bot_token\":\"...\",\"allowed_users\":[]}'`",
        )?;

    let telegram_cfg = channels_tbl
        .get_mut("telegram")
        .and_then(|v| v.as_table_mut())
        .context(
            "No [channels_config.telegram] section found. Add Telegram first with \
             `sen channel add telegram '{\"bot_token\":\"...\",\"allowed_users\":[]}'`",
        )?;

    let allowed = telegram_cfg
        .entry("allowed_users")
        .or_insert_with(|| toml::Value::Array(Vec::new()))
        .as_array_mut()
        .context("allowed_users must be an array")?;

    let identity_val = toml::Value::String(identity.to_string());
    if allowed.contains(&identity_val) {
        println!("ℹ️  Identity '{}' is already in the Telegram allowlist.", identity);
        return Ok(());
    }

    allowed.push(identity_val);

    let serialized =
        toml::to_string_pretty(&doc).context("Failed to serialize updated config")?;
    tokio::fs::write(config_path, serialized)
        .await
        .with_context(|| format!("Failed to write {}", config_path.display()))?;

    println!(
        "✅ Added '{}' to [channels_config.telegram].allowed_users.",
        identity
    );
    Ok(())
}

async fn channel_send(
    config: &Config,
    channel_id: &str,
    recipient: &str,
    message: &str,
) -> anyhow::Result<()> {
    let cfg = &config.channels_config;

    let channel: Arc<dyn Channel> = match channel_id {
        #[cfg(feature = "channel-telegram")]
        "telegram" => {
            let tg = cfg
                .telegram
                .as_ref()
                .context("No [channels_config.telegram] configured")?;
            Arc::new(TelegramChannel::new(
                tg.bot_token.clone(),
                tg.allowed_users.clone(),
                false,
            ))
        }
        #[cfg(feature = "channel-slack")]
        "slack" => {
            let sl = cfg
                .slack
                .as_ref()
                .context("No [channels_config.slack] configured")?;
            Arc::new(SlackChannel::new(
                sl.bot_token.clone(),
                sl.app_token.clone(),
                sl.channel_id.clone(),
                sl.channel_ids.clone(),
                sl.allowed_users.clone(),
            ))
        }
        #[cfg(feature = "channel-discord")]
        "discord" => {
            let dc = cfg
                .discord
                .as_ref()
                .context("No [channels_config.discord] configured")?;
            Arc::new(DiscordChannel::new(
                dc.bot_token.clone(),
                dc.guild_id.clone(),
                dc.allowed_users.clone(),
                dc.listen_to_bots,
                dc.mention_only,
            ))
        }
        "mattermost" => {
            let mm = cfg
                .mattermost
                .as_ref()
                .context("No [channels_config.mattermost] configured")?;
            Arc::new(MattermostChannel::new(
                mm.url.clone(),
                mm.bot_token.clone(),
                mm.channel_id.clone(),
                mm.allowed_users.clone(),
                mm.thread_replies.unwrap_or(true),
                mm.mention_only.unwrap_or(false),
            ))
        }
        "webhook" => {
            let wh = cfg
                .webhook
                .as_ref()
                .context("No [channels_config.webhook] configured")?;
            Arc::new(WebhookChannel::new(
                wh.port,
                wh.listen_path.clone(),
                wh.send_url.clone(),
                wh.send_method.clone(),
                wh.auth_header.clone(),
                wh.secret.clone(),
            ))
        }
        "signal" => {
            let sg = cfg
                .signal
                .as_ref()
                .context("No [channels_config.signal] configured")?;
            Arc::new(SignalChannel::new(
                sg.http_url.clone(),
                sg.account.clone(),
                sg.group_id.clone(),
                sg.allowed_from.clone(),
                sg.ignore_attachments,
                sg.ignore_stories,
            ))
        }
        "whatsapp" => {
            let wa = cfg
                .whatsapp
                .as_ref()
                .context("No [channels_config.whatsapp] configured")?;
            Arc::new(WhatsAppChannel::new(
                wa.access_token.clone().unwrap_or_default(),
                wa.phone_number_id.clone().unwrap_or_default(),
                wa.verify_token.clone().unwrap_or_default(),
                wa.allowed_numbers.clone(),
            ))
        }
        #[cfg(feature = "channel-email")]
        "email" => {
            let em = cfg
                .email
                .as_ref()
                .context("No [channels_config.email] configured")?;
            Arc::new(EmailChannel::new(em.clone()))
        }
        "qq" => {
            let qq = cfg
                .qq
                .as_ref()
                .context("No [channels_config.qq] configured")?;
            Arc::new(QQChannel::new(
                qq.app_id.clone(),
                qq.app_secret.clone(),
                qq.allowed_users.clone(),
            ))
        }
        #[cfg(feature = "channel-dingtalk")]
        "dingtalk" => {
            let dt = cfg
                .dingtalk
                .as_ref()
                .context("No [channels_config.dingtalk] configured")?;
            Arc::new(DingTalkChannel::new(
                dt.client_id.clone(),
                dt.client_secret.clone(),
                dt.allowed_users.clone(),
            ))
        }
        #[cfg(feature = "channel-wechat")]
        "wecom" => {
            let wc = cfg
                .wecom
                .as_ref()
                .context("No [channels_config.wecom] configured")?;
            Arc::new(WeComChannel::new(
                wc.webhook_key.clone(),
                wc.allowed_users.clone(),
            ))
        }
        "twitter" => {
            let tw = cfg
                .twitter
                .as_ref()
                .context("No [channels_config.twitter] configured")?;
            Arc::new(TwitterChannel::new(
                tw.bearer_token.clone(),
                tw.allowed_users.clone(),
            ))
        }
        "bluesky" => {
            let bs = cfg
                .bluesky
                .as_ref()
                .context("No [channels_config.bluesky] configured")?;
            Arc::new(BlueskyChannel::new(
                bs.handle.clone(),
                bs.app_password.clone(),
            ))
        }
        "nextcloud_talk" => {
            let nc = cfg
                .nextcloud_talk
                .as_ref()
                .context("No [channels_config.nextcloud_talk] configured")?;
            Arc::new(NextcloudTalkChannel::new(
                nc.base_url.clone(),
                nc.app_token.clone(),
                "bot".to_string(),
                nc.allowed_users.clone(),
            ))
        }
        "linq" => {
            let lq = cfg
                .linq
                .as_ref()
                .context("No [channels_config.linq] configured")?;
            Arc::new(LinqChannel::new(
                lq.api_token.clone(),
                lq.from_phone.clone(),
                lq.allowed_senders.clone(),
            ))
        }
        "wati" => {
            let wt = cfg
                .wati
                .as_ref()
                .context("No [channels_config.wati] configured")?;
            Arc::new(WatiChannel::new(
                wt.api_token.clone(),
                wt.api_url.clone(),
                wt.tenant_id.clone(),
                wt.allowed_numbers.clone(),
            ))
        }
        other => {
            anyhow::bail!(
                "Unknown channel-id '{}'. Valid values: telegram, slack, discord, \
                 mattermost, webhook, signal, whatsapp, email, qq, dingtalk, wecom, \
                 twitter, bluesky, nextcloud_talk, linq, wati",
                other
            );
        }
    };

    channel
        .send(&SendMessage::new(message, recipient))
        .await
        .with_context(|| {
            format!(
                "Failed to send message via '{}' to '{}'",
                channel_id, recipient
            )
        })?;

    println!(
        "✅ Message sent via {} to {}.",
        channel_id, recipient
    );
    Ok(())
}

fn json_to_toml_value(json: serde_json::Value) -> anyhow::Result<toml::Value> {
    match json {
        serde_json::Value::Null => Ok(toml::Value::String(String::new())),
        serde_json::Value::Bool(b) => Ok(toml::Value::Boolean(b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(toml::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(toml::Value::Float(f))
            } else {
                Ok(toml::Value::String(n.to_string()))
            }
        }
        serde_json::Value::String(s) => Ok(toml::Value::String(s)),
        serde_json::Value::Array(arr) => {
            let items: anyhow::Result<Vec<toml::Value>> =
                arr.into_iter().map(json_to_toml_value).collect();
            Ok(toml::Value::Array(items?))
        }
        serde_json::Value::Object(map) => {
            let mut tbl = toml::map::Map::new();
            for (k, v) in map {
                tbl.insert(k, json_to_toml_value(v)?);
            }
            Ok(toml::Value::Table(tbl))
        }
    }
}

async fn build_memory_context(
    mem: &dyn Memory,
    user_msg: &str,
    min_relevance_score: f64,
    session_id: Option<&str>,
) -> String {
    let mut context = String::new();

    if let Ok(entries) = mem.recall(user_msg, 5, session_id, None, None).await {
        let mut included = 0usize;
        let mut used_chars = 0usize;

        for entry in entries.iter().filter(|e| match e.score {
            Some(score) => score >= min_relevance_score,
            None => true,
        }) {
            if included >= MEMORY_CONTEXT_MAX_ENTRIES {
                break;
            }

            if should_skip_memory_context_entry(&entry.key, &entry.content) {
                continue;
            }

            let entry_content = if entry.content.chars().count() > MEMORY_CONTEXT_ENTRY_MAX_CHARS {
                truncate_with_ellipsis(&entry.content, MEMORY_CONTEXT_ENTRY_MAX_CHARS)
            } else {
                entry.content.clone()
            };

            let line = format!("- {}: {}\n", entry.key, entry_content);
            let line_chars = line.chars().count();
            if used_chars + line_chars > MEMORY_CONTEXT_MAX_CHARS {
                break;
            }

            if included == 0 {
                context.push_str("[Memory context]\n");
            }

            context.push_str(&line);
            used_chars += line_chars;
            included += 1;
        }

        if included > 0 {
            context.push_str("[/Memory context]\n\n");
        }
    }

    context
}
