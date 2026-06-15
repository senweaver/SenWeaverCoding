// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
pub mod domain;
pub mod hot_reload;
pub mod live;

pub fn sniff_gateway_isolated() -> bool {
    fn config_dir() -> Option<std::path::PathBuf> {
        if let Ok(dir) = std::env::var("SEN_CONFIG_DIR") {
            let trimmed = dir.trim();
            if !trimmed.is_empty() {
                return Some(std::path::PathBuf::from(trimmed));
            }
        }
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
        Some(std::path::PathBuf::from(home).join(".senweavercoding"))
    }
    let Some(path) = config_dir().map(|d| d.join("config.toml")) else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(value) = text.parse::<toml::Table>() else {
        return false;
    };
    value
        .get("gateway")
        .and_then(|g| g.get("isolated"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false)
}
pub mod redact;
pub mod schema;
pub mod traits;
pub mod validator;
pub mod workspace;
pub use schema::{
    AgentConfig, AssemblyAiSttConfig, AuditConfig, AutonomyConfig, BackupConfig,
    BrowserComputerUseConfig, BrowserConfig, BuddyConfig, BuiltinHooksConfig, ChannelsConfig,
    ClassificationRule, ClaudeCodeConfig, ClaudeCodeRunnerConfig, CloudOpsConfig, CodexCliConfig,
    ComposioConfig, Config, CostConfig, CronConfig, CronJobDecl,
    CronScheduleDecl, CustomToolDef, CustomToolsConfig, DEFAULT_GWS_SERVICES,
    DataRetentionConfig, DeepgramSttConfig,
    DelegateAgentConfig, DelegateToolConfig, DiscordConfig, DockerRuntimeConfig, EdgeTtsConfig,
    ElevenLabsTtsConfig, EmbeddingRouteConfig, EstopConfig, FeishuConfig, GatewayConfig,
    GeminiCliConfig, GoogleSttConfig, GoogleTtsConfig, GoogleWorkspaceAllowedOperation,
    GoogleWorkspaceConfig, HandsConfig, HardwareConfig, HardwareTransport, HeartbeatConfig,
    HooksConfig,
    HttpRequestConfig, IMessageConfig, IdentityConfig, ImageGenConfig, ImageProviderDalleConfig,
    ImageProviderFluxConfig, ImageProviderImagenConfig, ImageProviderStabilityConfig, JiraConfig,
    KnowledgeConfig, LarkConfig, LinkEnricherConfig, LinkedInConfig, LinkedInContentConfig,
    LinkedInImageConfig, LocalWhisperConfig, MatrixConfig, McpConfig, McpServerConfig,
    McpTransport, MediaPipelineConfig, MemoryConfig, MemoryPolicyConfig, Microsoft365Config,
    ModelProviderConfig, ModelRouteConfig, MultimodalConfig, NextcloudTalkConfig,
    DEFAULT_MODEL_TYPE, MODEL_TYPES, is_known_model_type, sanitize_model_types,
    NodesConfig,
    NotionConfig, ObservabilityConfig, OpenAiSttConfig, OpenAiTtsConfig, OpenCodeCliConfig,
    OpenVpnTunnelConfig, OtpConfig, OtpMethod, PacingConfig, PeripheralBoardConfig,
    PeripheralsConfig, PipelineConfig, PiperTtsConfig, PluginsConfig, ProjectIntelConfig,
    ProxyConfig, ProxyScope, QdrantConfig, QueryClassificationConfig, ReliabilityConfig,
    ResourceLimitsConfig, RpcConfig, RpcHttpConfig, RuntimeConfig, SandboxBackend, SandboxConfig,
    SchedulerConfig, SearchMode, SecretsConfig, SecurityConfig, SecurityOpsConfig, ShellToolConfig,
    SkillCreationConfig, SkillImprovementConfig, SkillsConfig, SkillsPromptInjectionMode,
    SlackConfig, SopConfig, StorageConfig, StorageProviderConfig, StorageProviderSection,
    StreamMode, SwarmConfig, SwarmStrategy, TeamsConfig, TelegramConfig, TextBrowserConfig, ToolFilterGroup,
    ToolFilterGroupMode, TranscriptionConfig, TtsConfig, TunnelConfig, VerifiableIntentConfig,
    WasmRuntimeConfig, WebFetchConfig, WebSearchConfig, WebhookConfig, WhatsAppChatPolicy,
    WhatsAppWebMode, WorkspaceConfig, CustomHttpHeader, DISALLOWED_CUSTOM_HEADER_NAMES,
    build_custom_headers_map, default_agent_max_tool_iterations,
    is_disallowed_custom_header, is_valid_http_header_name, is_valid_http_header_value,
};
pub use hot_reload::validators;
pub use hot_reload::{ConfigChangedEvent, LiveConfig, SharedConfig};

pub fn name_and_presence<T: traits::ChannelConfig>(channel: Option<&T>) -> (&'static str, bool) {
    (T::name(), channel.is_some())
}
