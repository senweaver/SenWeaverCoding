// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
pub mod domain;
pub mod file_watch;
pub mod hot_reload;
pub mod live;

pub mod redact;
pub mod schema;
pub mod traits;
pub mod validator;
pub mod workspace;
pub use schema::{
    AgentConfig, AssemblyAiSttConfig, AuditConfig, AutonomyConfig, BackupConfig,
    BrowserConfig, BuddyConfig, BuiltinHooksConfig, ChannelsConfig,
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
    ModelProviderConfig, ModelRouteConfig, MqttConfig, MultimodalConfig, NextcloudTalkConfig,
    DEFAULT_MODEL_TYPE, MODEL_TYPES, classify_model_type, is_known_model_type, sanitize_model_types,
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
    sniff_gateway_isolated,
};
pub use hot_reload::{ConfigChangedEvent, LiveConfig, SharedConfig};

pub(crate) use schema::{
    api_format_to_wire_api, apply_desktop_permission_mode_to_autonomy,
    desktop_permission_blocks_tool, normalize_desktop_permission_mode, normalize_wire_api,
    permission_mode_allows_auto_approve, provider_requires_openai_auth, wire_api_to_api_format,
};

pub fn name_and_presence<T: traits::ChannelConfig>(channel: Option<&T>) -> (&'static str, bool) {
    (T::name(), channel.is_some())
}
