// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod anthropic;
pub mod azure_openai;
pub mod bedrock;
pub mod claude_code;
pub mod compatible;
pub mod copilot;
pub mod core;
pub mod gemini;
pub mod kilocli;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod reliable;
pub mod router;
pub mod sanitize;
pub mod telnyx;
pub mod traits;
pub use traits::{
    ChatMessage, ChatRequest, ChatResponse, ConversationMessage, Provider, ProviderCapabilityError,
    TokenUsage, ToolCall, ToolResultMessage,
};

use crate::auth::AuthService;
use compatible::{AuthStyle, OpenAiCompatibleProvider};
use reliable::ReliableProvider;
use serde::Deserialize;
use std::path::PathBuf;

const MAX_API_ERROR_CHARS: usize = 500;
const MINIMAX_INTL_BASE_URL: &str = "https://api.minimax.io/v1";
const MINIMAX_CN_BASE_URL: &str = "https://api.minimaxi.com/v1";
const MINIMAX_OAUTH_GLOBAL_TOKEN_ENDPOINT: &str = "https://api.minimax.io/oauth/token";
const MINIMAX_OAUTH_CN_TOKEN_ENDPOINT: &str = "https://api.minimaxi.com/oauth/token";
const MINIMAX_OAUTH_PLACEHOLDER: &str = "minimax-oauth";
const MINIMAX_OAUTH_CN_PLACEHOLDER: &str = "minimax-oauth-cn";
const MINIMAX_OAUTH_TOKEN_ENV: &str = "MINIMAX_OAUTH_TOKEN";
const MINIMAX_API_KEY_ENV: &str = "MINIMAX_API_KEY";
const MINIMAX_OAUTH_REFRESH_TOKEN_ENV: &str = "MINIMAX_OAUTH_REFRESH_TOKEN";
const MINIMAX_OAUTH_REGION_ENV: &str = "MINIMAX_OAUTH_REGION";
const MINIMAX_OAUTH_CLIENT_ID_ENV: &str = "MINIMAX_OAUTH_CLIENT_ID";
const MINIMAX_OAUTH_DEFAULT_CLIENT_ID: &str = "78257093-7e40-4613-99e0-527b14b39113";
const SENWEAVER_BASE_URL: &str = "https://api3.senweaver.com/v1";
const GLM_GLOBAL_BASE_URL: &str = "https://api.z.ai/api/paas/v4";
const GLM_CN_BASE_URL: &str = "https://open.bigmodel.cn/api/paas/v4";
const MOONSHOT_INTL_BASE_URL: &str = "https://api.moonshot.ai/v1";
const MOONSHOT_CN_BASE_URL: &str = "https://api.moonshot.cn/v1";
const QWEN_CN_BASE_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";
const QWEN_INTL_BASE_URL: &str = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1";
const QWEN_US_BASE_URL: &str = "https://dashscope-us.aliyuncs.com/compatible-mode/v1";
const QWEN_OAUTH_BASE_FALLBACK_URL: &str = QWEN_CN_BASE_URL;
const BAILIAN_BASE_URL: &str = "https://coding.dashscope.aliyuncs.com/v1";
const QWEN_OAUTH_TOKEN_ENDPOINT: &str = "https://chat.qwen.ai/api/v1/oauth2/token";
const QWEN_OAUTH_PLACEHOLDER: &str = "qwen-oauth";
const QWEN_OAUTH_TOKEN_ENV: &str = "QWEN_OAUTH_TOKEN";
const QWEN_OAUTH_REFRESH_TOKEN_ENV: &str = "QWEN_OAUTH_REFRESH_TOKEN";
const QWEN_OAUTH_RESOURCE_URL_ENV: &str = "QWEN_OAUTH_RESOURCE_URL";
const QWEN_OAUTH_CLIENT_ID_ENV: &str = "QWEN_OAUTH_CLIENT_ID";
const QWEN_OAUTH_DEFAULT_CLIENT_ID: &str = "f0304373b74a44d2b584a3fb70ca9e56";
const QWEN_OAUTH_CREDENTIAL_FILE: &str = ".qwen/oauth_creds.json";
const ZAI_GLOBAL_BASE_URL: &str = "https://api.z.ai/api/coding/paas/v4";
const ZAI_CN_BASE_URL: &str = "https://open.bigmodel.cn/api/coding/paas/v4";
const QIANFAN_BASE_URL: &str = "https://qianfan.baidubce.com/v2";
const VERCEL_AI_GATEWAY_BASE_URL: &str = "https://ai-gateway.vercel.sh/v1";

pub(crate) fn is_minimax_intl_alias(name: &str) -> bool {
    matches!(
        name,
        "minimax"
            | "minimax-intl"
            | "minimax-io"
            | "minimax-global"
            | "minimax-oauth"
            | "minimax-portal"
            | "minimax-oauth-global"
            | "minimax-portal-global"
    )
}

pub(crate) fn is_minimax_cn_alias(name: &str) -> bool {
    matches!(
        name,
        "minimax-cn" | "minimaxi" | "minimax-oauth-cn" | "minimax-portal-cn"
    )
}

pub(crate) fn is_minimax_alias(name: &str) -> bool {
    is_minimax_intl_alias(name) || is_minimax_cn_alias(name)
}

pub(crate) fn is_glm_global_alias(name: &str) -> bool {
    matches!(name, "glm" | "zhipu" | "glm-global" | "zhipu-global")
}

pub(crate) fn is_glm_cn_alias(name: &str) -> bool {
    matches!(name, "glm-cn" | "zhipu-cn" | "bigmodel")
}

pub(crate) fn is_glm_alias(name: &str) -> bool {
    is_glm_global_alias(name) || is_glm_cn_alias(name)
}

pub(crate) fn is_moonshot_intl_alias(name: &str) -> bool {
    matches!(
        name,
        "moonshot-intl" | "moonshot-global" | "kimi-intl" | "kimi-global"
    )
}

pub(crate) fn is_moonshot_cn_alias(name: &str) -> bool {
    matches!(name, "moonshot" | "kimi" | "moonshot-cn" | "kimi-cn")
}

pub(crate) fn is_moonshot_alias(name: &str) -> bool {
    is_moonshot_intl_alias(name) || is_moonshot_cn_alias(name)
}

pub(crate) fn is_qwen_cn_alias(name: &str) -> bool {
    matches!(name, "qwen" | "dashscope" | "qwen-cn" | "dashscope-cn")
}

pub(crate) fn is_qwen_intl_alias(name: &str) -> bool {
    matches!(
        name,
        "qwen-intl" | "dashscope-intl" | "qwen-international" | "dashscope-international"
    )
}

pub(crate) fn is_qwen_us_alias(name: &str) -> bool {
    matches!(name, "qwen-us" | "dashscope-us")
}

pub(crate) fn is_qwen_oauth_alias(name: &str) -> bool {
    matches!(name, "qwen-code" | "qwen-oauth" | "qwen_oauth")
}

pub(crate) fn is_bailian_alias(name: &str) -> bool {
    matches!(name, "bailian" | "aliyun-bailian" | "aliyun")
}

pub(crate) fn is_qwen_alias(name: &str) -> bool {
    is_qwen_cn_alias(name)
        || is_qwen_intl_alias(name)
        || is_qwen_us_alias(name)
        || is_qwen_oauth_alias(name)
}

pub(crate) fn is_zai_global_alias(name: &str) -> bool {
    matches!(name, "zai" | "z.ai" | "zai-global" | "z.ai-global")
}

pub(crate) fn is_zai_cn_alias(name: &str) -> bool {
    matches!(name, "zai-cn" | "z.ai-cn")
}

pub(crate) fn is_zai_alias(name: &str) -> bool {
    is_zai_global_alias(name) || is_zai_cn_alias(name)
}

pub(crate) fn is_qianfan_alias(name: &str) -> bool {
    matches!(name, "qianfan" | "baidu")
}

fn qianfan_base_url(api_url: Option<&str>) -> String {
    api_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| QIANFAN_BASE_URL.to_string())
}

pub(crate) fn is_doubao_alias(name: &str) -> bool {
    matches!(name, "doubao" | "volcengine" | "ark" | "doubao-cn")
}

#[derive(Clone, Copy, Debug)]
enum MinimaxOauthRegion {
    Global,
    Cn,
}

impl MinimaxOauthRegion {
    fn token_endpoint(self) -> &'static str {
        match self {
            Self::Global => MINIMAX_OAUTH_GLOBAL_TOKEN_ENDPOINT,
            Self::Cn => MINIMAX_OAUTH_CN_TOKEN_ENDPOINT,
        }
    }
}

#[derive(Debug, Deserialize)]
struct MinimaxOauthRefreshResponse {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    base_resp: Option<MinimaxOauthBaseResponse>,
}

#[derive(Debug, Deserialize)]
struct MinimaxOauthBaseResponse {
    #[serde(default)]
    status_msg: Option<String>,
}

#[derive(Clone, Deserialize, Default)]
struct QwenOauthCredentials {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    resource_url: Option<String>,
    #[serde(default)]
    expiry_date: Option<i64>,
}

impl std::fmt::Debug for QwenOauthCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QwenOauthCredentials")
            .field("resource_url", &self.resource_url)
            .field("expiry_date", &self.expiry_date)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Deserialize)]
struct QwenOauthTokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    resource_url: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Clone, Default)]
struct QwenOauthProviderContext {
    credential: Option<String>,
    base_url: Option<String>,
}

impl std::fmt::Debug for QwenOauthProviderContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QwenOauthProviderContext")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

fn read_non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn is_minimax_oauth_placeholder(value: &str) -> bool {
    value.eq_ignore_ascii_case(MINIMAX_OAUTH_PLACEHOLDER)
        || value.eq_ignore_ascii_case(MINIMAX_OAUTH_CN_PLACEHOLDER)
}

fn minimax_oauth_region(name: &str) -> MinimaxOauthRegion {
    if let Some(region) = read_non_empty_env(MINIMAX_OAUTH_REGION_ENV) {
        let normalized = region.to_ascii_lowercase();
        if matches!(normalized.as_str(), "cn" | "china") {
            return MinimaxOauthRegion::Cn;
        }
        if matches!(normalized.as_str(), "global" | "intl" | "international") {
            return MinimaxOauthRegion::Global;
        }
    }

    if is_minimax_cn_alias(name) {
        MinimaxOauthRegion::Cn
    } else {
        MinimaxOauthRegion::Global
    }
}

fn minimax_oauth_client_id() -> String {
    read_non_empty_env(MINIMAX_OAUTH_CLIENT_ID_ENV)
        .unwrap_or_else(|| MINIMAX_OAUTH_DEFAULT_CLIENT_ID.to_string())
}

fn qwen_oauth_client_id() -> String {
    read_non_empty_env(QWEN_OAUTH_CLIENT_ID_ENV)
        .unwrap_or_else(|| QWEN_OAUTH_DEFAULT_CLIENT_ID.to_string())
}

fn qwen_oauth_credentials_file_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .map(|home| home.join(QWEN_OAUTH_CREDENTIAL_FILE))
}

fn normalize_qwen_oauth_base_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };

    let normalized = with_scheme.trim_end_matches('/').to_string();
    if normalized.ends_with("/v1") {
        Some(normalized)
    } else {
        Some(format!("{normalized}/v1"))
    }
}

fn read_qwen_oauth_cached_credentials() -> Option<QwenOauthCredentials> {
    let path = qwen_oauth_credentials_file_path()?;
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<QwenOauthCredentials>(&content).ok()
}

fn normalized_qwen_expiry_millis(raw: i64) -> i64 {
    if raw < 10_000_000_000 {
        raw.saturating_mul(1000)
    } else {
        raw
    }
}

fn qwen_oauth_token_expired(credentials: &QwenOauthCredentials) -> bool {
    let Some(expiry) = credentials.expiry_date else {
        return false;
    };

    let expiry_millis = normalized_qwen_expiry_millis(expiry);
    let now_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX);

    expiry_millis <= now_millis.saturating_add(30_000)
}

fn refresh_qwen_oauth_access_token(refresh_token: &str) -> anyhow::Result<QwenOauthCredentials> {
    let client_id = qwen_oauth_client_id();
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new());

    let response = client
        .post(QWEN_OAUTH_TOKEN_ENDPOINT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id.as_str()),
        ])
        .send()
        .map_err(|error| anyhow::anyhow!("Qwen OAuth refresh request failed: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .unwrap_or_else(|_| "<failed to read Qwen OAuth response body>".to_string());

    let parsed = serde_json::from_str::<QwenOauthTokenResponse>(&body).ok();

    if !status.is_success() {
        let detail = parsed
            .as_ref()
            .and_then(|payload| payload.error_description.as_deref())
            .or_else(|| parsed.as_ref().and_then(|payload| payload.error.as_deref()))
            .filter(|msg| !msg.trim().is_empty())
            .unwrap_or(body.as_str());
        anyhow::bail!("Qwen OAuth refresh failed (HTTP {status}): {detail}");
    }

    let payload =
        parsed.ok_or_else(|| anyhow::anyhow!("Qwen OAuth refresh response is not JSON"))?;

    if let Some(error_code) = payload
        .error
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let detail = payload.error_description.as_deref().unwrap_or(error_code);
        anyhow::bail!("Qwen OAuth refresh failed: {detail}");
    }

    let access_token = payload
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Qwen OAuth refresh response missing access_token"))?
        .to_string();

    let expiry_date = payload.expires_in.and_then(|seconds| {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_secs()).ok())?;
        now_secs
            .checked_add(seconds)
            .and_then(|unix_secs| unix_secs.checked_mul(1000))
    });

    Ok(QwenOauthCredentials {
        access_token: Some(access_token),
        refresh_token: payload
            .refresh_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        resource_url: payload
            .resource_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        expiry_date,
    })
}

fn resolve_qwen_oauth_context(credential_override: Option<&str>) -> QwenOauthProviderContext {
    let override_value = credential_override
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let placeholder_requested = override_value
        .map(|value| value.eq_ignore_ascii_case(QWEN_OAUTH_PLACEHOLDER))
        .unwrap_or(false);

    if let Some(explicit) = override_value {
        if !placeholder_requested {
            return QwenOauthProviderContext {
                credential: Some(explicit.to_string()),
                base_url: None,
            };
        }
    }

    let mut cached = read_qwen_oauth_cached_credentials();

    let env_token = read_non_empty_env(QWEN_OAUTH_TOKEN_ENV);
    let env_refresh_token = read_non_empty_env(QWEN_OAUTH_REFRESH_TOKEN_ENV);
    let env_resource_url = read_non_empty_env(QWEN_OAUTH_RESOURCE_URL_ENV);

    if env_token.is_none() {
        let refresh_token = env_refresh_token.clone().or_else(|| {
            cached
                .as_ref()
                .and_then(|credentials| credentials.refresh_token.clone())
        });

        let should_refresh = cached.as_ref().is_some_and(qwen_oauth_token_expired)
            || cached
                .as_ref()
                .and_then(|credentials| credentials.access_token.as_deref())
                .is_none_or(|value| value.trim().is_empty());

        if should_refresh {
            if let Some(refresh_token) = refresh_token.as_deref() {
                match refresh_qwen_oauth_access_token(refresh_token) {
                    Ok(refreshed) => {
                        cached = Some(refreshed);
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "Qwen OAuth refresh failed");
                    }
                }
            }
        }
    }

    let mut credential = env_token.or_else(|| {
        cached
            .as_ref()
            .and_then(|credentials| credentials.access_token.clone())
    });
    credential = credential
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    if credential.is_none() && !placeholder_requested {
        credential = read_non_empty_env("DASHSCOPE_API_KEY");
    }

    let base_url = env_resource_url
        .as_deref()
        .and_then(normalize_qwen_oauth_base_url)
        .or_else(|| {
            cached
                .as_ref()
                .and_then(|credentials| credentials.resource_url.as_deref())
                .and_then(normalize_qwen_oauth_base_url)
        });

    QwenOauthProviderContext {
        credential,
        base_url,
    }
}

fn resolve_minimax_static_credential() -> Option<String> {
    read_non_empty_env(MINIMAX_OAUTH_TOKEN_ENV).or_else(|| read_non_empty_env(MINIMAX_API_KEY_ENV))
}

fn refresh_minimax_oauth_access_token(name: &str, refresh_token: &str) -> anyhow::Result<String> {
    let region = minimax_oauth_region(name);
    let endpoint = region.token_endpoint();
    let client_id = minimax_oauth_client_id();
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new());

    let response = client
        .post(endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id.as_str()),
        ])
        .send()
        .map_err(|error| anyhow::anyhow!("MiniMax OAuth refresh request failed: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .unwrap_or_else(|_| "<failed to read MiniMax OAuth response body>".to_string());

    let parsed = serde_json::from_str::<MinimaxOauthRefreshResponse>(&body).ok();

    if !status.is_success() {
        let detail = parsed
            .as_ref()
            .and_then(|payload| payload.base_resp.as_ref())
            .and_then(|base| base.status_msg.as_deref())
            .filter(|msg| !msg.trim().is_empty())
            .unwrap_or(body.as_str());
        anyhow::bail!("MiniMax OAuth refresh failed (HTTP {status}): {detail}");
    }

    if let Some(payload) = parsed {
        if let Some(status_text) = payload.status.as_deref() {
            if !status_text.eq_ignore_ascii_case("success") {
                let detail = payload
                    .base_resp
                    .as_ref()
                    .and_then(|base| base.status_msg.as_deref())
                    .unwrap_or(status_text);
                anyhow::bail!("MiniMax OAuth refresh failed: {detail}");
            }
        }

        if let Some(token) = payload
            .access_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            return Ok(token.to_string());
        }
    }

    anyhow::bail!("MiniMax OAuth refresh response missing access_token");
}

fn resolve_minimax_oauth_refresh_token(name: &str) -> Option<String> {
    let refresh_token = read_non_empty_env(MINIMAX_OAUTH_REFRESH_TOKEN_ENV)?;

    match refresh_minimax_oauth_access_token(name, &refresh_token) {
        Ok(token) => Some(token),
        Err(error) => {
            tracing::warn!(provider = name, error = %error, "MiniMax OAuth refresh failed");
            None
        }
    }
}

pub(crate) fn canonical_china_provider_name(name: &str) -> Option<&'static str> {
    if is_qwen_alias(name) {
        Some("qwen")
    } else if is_glm_alias(name) {
        Some("glm")
    } else if is_moonshot_alias(name) {
        Some("moonshot")
    } else if is_minimax_alias(name) {
        Some("minimax")
    } else if is_zai_alias(name) {
        Some("zai")
    } else if is_qianfan_alias(name) {
        Some("qianfan")
    } else if is_doubao_alias(name) {
        Some("doubao")
    } else if is_bailian_alias(name) {
        Some("bailian")
    } else {
        None
    }
}

fn minimax_base_url(name: &str) -> Option<&'static str> {
    if is_minimax_cn_alias(name) {
        Some(MINIMAX_CN_BASE_URL)
    } else if is_minimax_intl_alias(name) {
        Some(MINIMAX_INTL_BASE_URL)
    } else {
        None
    }
}

fn glm_base_url(name: &str) -> Option<&'static str> {
    if is_glm_cn_alias(name) {
        Some(GLM_CN_BASE_URL)
    } else if is_glm_global_alias(name) {
        Some(GLM_GLOBAL_BASE_URL)
    } else {
        None
    }
}

fn moonshot_base_url(name: &str) -> Option<&'static str> {
    if is_moonshot_intl_alias(name) {
        Some(MOONSHOT_INTL_BASE_URL)
    } else if is_moonshot_cn_alias(name) {
        Some(MOONSHOT_CN_BASE_URL)
    } else {
        None
    }
}

fn qwen_base_url(name: &str) -> Option<&'static str> {
    if is_qwen_cn_alias(name) || is_qwen_oauth_alias(name) {
        Some(QWEN_CN_BASE_URL)
    } else if is_qwen_intl_alias(name) {
        Some(QWEN_INTL_BASE_URL)
    } else if is_qwen_us_alias(name) {
        Some(QWEN_US_BASE_URL)
    } else {
        None
    }
}

fn zai_base_url(name: &str) -> Option<&'static str> {
    if is_zai_cn_alias(name) {
        Some(ZAI_CN_BASE_URL)
    } else if is_zai_global_alias(name) {
        Some(ZAI_GLOBAL_BASE_URL)
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub struct ProviderRuntimeOptions {
    pub auth_profile_override: Option<String>,
    pub provider_api_url: Option<String>,
    pub sen_dir: Option<PathBuf>,
    pub secrets_encrypt: bool,
    pub reasoning_enabled: Option<bool>,
    pub reasoning_effort: Option<String>,

    pub provider_timeout_secs: Option<u64>,

    pub extra_headers: std::collections::HashMap<String, String>,

    pub api_path: Option<String>,

    pub provider_max_tokens: Option<u32>,

    pub model_context_windows: std::collections::HashMap<String, u32>,
}

impl Default for ProviderRuntimeOptions {
    fn default() -> Self {
        Self {
            auth_profile_override: None,
            provider_api_url: None,
            sen_dir: None,
            secrets_encrypt: true,
            reasoning_enabled: None,
            reasoning_effort: None,
            provider_timeout_secs: None,
            extra_headers: std::collections::HashMap::new(),
            api_path: None,
            provider_max_tokens: None,
            model_context_windows: std::collections::HashMap::new(),
        }
    }
}

pub fn provider_runtime_options_from_config(
    config: &crate::config::Config,
) -> ProviderRuntimeOptions {
    ProviderRuntimeOptions {
        auth_profile_override: None,
        provider_api_url: config.api_url.clone(),
        sen_dir: config.config_path.parent().map(PathBuf::from),
        secrets_encrypt: config.secrets.encrypt,
        reasoning_enabled: config.runtime.reasoning_enabled,
        reasoning_effort: config.runtime.reasoning_effort.clone(),
        provider_timeout_secs: Some(config.provider_timeout_secs),
        extra_headers: merged_extra_headers_for_config(config),
        api_path: config.api_path.clone(),
        provider_max_tokens: config.provider_max_tokens,
        model_context_windows: config.model_context_windows.clone(),
    }
}

pub fn merged_extra_headers_for_config(
    config: &crate::config::Config,
) -> std::collections::HashMap<String, String> {
    let mut merged: std::collections::HashMap<String, String> =
        std::collections::HashMap::with_capacity(config.extra_headers.len());
    for (name, value) in &config.extra_headers {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            continue;
        }
        if crate::config::is_disallowed_custom_header(trimmed) {
            tracing::warn!(
                header_name = trimmed,
                "global extra_headers entry uses a reserved/disallowed header name; ignoring"
            );
            continue;
        }
        if !crate::config::is_valid_http_header_name(trimmed) {
            tracing::warn!(
                header_name = trimmed,
                "global extra_headers entry has invalid header name; ignoring"
            );
            continue;
        }
        if !crate::config::is_valid_http_header_value(value) {
            tracing::warn!(
                header_name = trimmed,
                "global extra_headers value contains CR/LF; ignoring"
            );
            continue;
        }
        merged.insert(trimmed.to_string(), value.clone());
    }

    if let Some(active_id) = config.default_provider.as_deref() {
        if let Some(profile) = config.model_providers.get(active_id) {
            let profile_headers = crate::config::build_custom_headers_map(&profile.custom_headers);
            for (name, value) in profile_headers {
                merged.insert(name, value);
            }
        }
    }
    merged
}

fn is_secret_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':')
}

fn token_end(input: &str, from: usize) -> usize {
    let mut end = from;
    for (i, c) in input[from..].char_indices() {
        if is_secret_char(c) {
            end = from + i + c.len_utf8();
        } else {
            break;
        }
    }
    end
}

pub fn scrub_secret_patterns(input: &str) -> String {
    const PREFIXES: [&str; 14] = [
        "sk-ant-",
        "sk-",
        "xoxb-",
        "xoxp-",
        "ghp_",
        "gho_",
        "ghu_",
        "github_pat_",
        "AIza",
        "AKIA",
        "glpat-",
        "hf_",
        "r8_",
        "eyJhbG",
    ];

    let mut scrubbed = input.to_string();

    for prefix in PREFIXES {
        let mut search_from = 0;
        loop {
            let Some(rel) = scrubbed[search_from..].find(prefix) else {
                break;
            };

            let start = search_from + rel;
            let content_start = start + prefix.len();
            let end = token_end(&scrubbed, content_start);

            if end == content_start {
                search_from = content_start;
                continue;
            }

            scrubbed.replace_range(start..end, "[REDACTED]");
            search_from = start + "[REDACTED]".len();
        }
    }

    scrubbed
}

pub fn sanitize_api_error(input: &str) -> String {
    let scrubbed = scrub_secret_patterns(input);

    if scrubbed.chars().count() <= MAX_API_ERROR_CHARS {
        return scrubbed;
    }

    let mut end = MAX_API_ERROR_CHARS;
    while end > 0 && !scrubbed.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}...", &scrubbed[..end])
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("{provider} API error ({status}): {sanitized_message}")]
    Http {
        provider: String,
        status: u16,
        body: String,
        sanitized_message: String,
    },

    #[error("{provider} transport error: {source}")]
    Transport {
        provider: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("{provider} response decode error: {source}")]
    Decode {
        provider: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("{0}")]
    Other(String),
}

impl ProviderError {
    #[must_use]
    pub fn http_status(&self) -> Option<u16> {
        match self {
            ProviderError::Http { status, .. } => Some(*status),
            _ => None,
        }
    }

    #[must_use]
    pub fn http_body(&self) -> Option<&str> {
        match self {
            ProviderError::Http { body, .. } => Some(body.as_str()),
            _ => None,
        }
    }

    #[must_use]
    pub fn sanitized_message(&self) -> String {
        match self {
            ProviderError::Http {
                sanitized_message, ..
            } => sanitized_message.clone(),
            other => other.to_string(),
        }
    }

    #[must_use]
    pub fn category(&self) -> crate::services::api::ApiErrorCategory {
        match self {
            ProviderError::Http { status, body, .. } => {
                crate::services::api::categorize_api_error(*status, Some(body.as_str()))
            }
            ProviderError::Transport { .. } => crate::services::api::ApiErrorCategory::NetworkError,
            ProviderError::Decode { .. } => crate::services::api::ApiErrorCategory::Unknown,
            ProviderError::Other(_) => crate::services::api::ApiErrorCategory::Unknown,
        }
    }

    #[must_use]
    pub fn is_auth_error(&self) -> bool {
        matches!(
            self.category(),
            crate::services::api::ApiErrorCategory::AuthError
        )
    }

    #[must_use]
    pub fn is_retryable(&self) -> bool {
        crate::services::api::is_retryable(self.category())
    }
}

pub async fn api_error_structured(
    provider: &str,
    response: reqwest::Response,
) -> ProviderError {
    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<failed to read provider error body>".to_string());
    let sanitized = sanitize_api_error(&body);
    ProviderError::Http {
        provider: provider.to_string(),
        status: status.as_u16(),
        body,
        sanitized_message: sanitized,
    }
}

pub async fn api_error(provider: &str, response: reqwest::Response) -> anyhow::Error {
    anyhow::Error::new(api_error_structured(provider, response).await)
}

fn resolve_provider_credential(name: &str, credential_override: Option<&str>) -> Option<String> {
    let mut minimax_oauth_placeholder_requested = false;

    if let Some(raw_override) = credential_override {
        let trimmed_override = raw_override.trim();
        if !trimmed_override.is_empty() {
            if is_minimax_alias(name) && is_minimax_oauth_placeholder(trimmed_override) {
                minimax_oauth_placeholder_requested = true;
                if let Some(credential) = resolve_minimax_static_credential() {
                    return Some(credential);
                }
                if let Some(credential) = resolve_minimax_oauth_refresh_token(name) {
                    return Some(credential);
                }
            } else if name == "anthropic" || name == "openai" || name == "groq" {

                let env_candidates: &[&str] = match name {
                    "anthropic" => &["ANTHROPIC_OAUTH_TOKEN", "ANTHROPIC_API_KEY"],
                    "openai" => &["OPENAI_API_KEY"],
                    "groq" => &["GROQ_API_KEY"],
                    _ => &[],
                };
                for env_var in env_candidates {
                    if let Ok(val) = std::env::var(env_var) {
                        let trimmed = val.trim().to_string();
                        if !trimmed.is_empty() {
                            return Some(trimmed);
                        }
                    }
                }
                return Some(trimmed_override.to_owned());
            } else {
                return Some(trimmed_override.to_owned());
            }
        }
    }

    let provider_env_candidates: Vec<&str> = match name {
        "senweaver" | "sw" => vec!["SENWEAVER_API_KEY", "API_KEY"],
        "anthropic" => vec!["ANTHROPIC_OAUTH_TOKEN", "ANTHROPIC_API_KEY"],
        "openrouter" => vec!["OPENROUTER_API_KEY"],
        "openai" => vec!["OPENAI_API_KEY"],
        "ollama" => vec!["OLLAMA_API_KEY"],
        "venice" => vec!["VENICE_API_KEY"],
        "groq" => vec!["GROQ_API_KEY"],
        "mistral" => vec!["MISTRAL_API_KEY"],
        "deepseek" => vec!["DEEPSEEK_API_KEY"],
        "xai" | "grok" => vec!["XAI_API_KEY"],
        "together" | "together-ai" => vec!["TOGETHER_API_KEY"],
        "fireworks" | "fireworks-ai" => vec!["FIREWORKS_API_KEY"],
        "novita" => vec!["NOVITA_API_KEY"],
        "perplexity" => vec!["PERPLEXITY_API_KEY"],
        "cohere" => vec!["COHERE_API_KEY"],
        name if is_moonshot_alias(name) => vec!["MOONSHOT_API_KEY"],
        "kimi-code" | "kimi_coding" | "kimi_for_coding" => {
            vec!["KIMI_CODE_API_KEY", "MOONSHOT_API_KEY"]
        }
        name if is_glm_alias(name) => vec!["GLM_API_KEY"],
        name if is_minimax_alias(name) => vec![MINIMAX_OAUTH_TOKEN_ENV, MINIMAX_API_KEY_ENV],

        "bedrock" | "aws-bedrock" => {
            if let Ok(val) = std::env::var("BEDROCK_API_KEY") {
                let trimmed = val.trim().to_string();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
            return None;
        }
        name if is_qianfan_alias(name) => vec!["QIANFAN_API_KEY"],
        name if is_doubao_alias(name) => {
            vec!["ARK_API_KEY", "VOLCENGINE_API_KEY", "DOUBAO_API_KEY"]
        }
        name if is_qwen_alias(name) => vec!["DASHSCOPE_API_KEY"],
        name if is_bailian_alias(name) => vec!["BAILIAN_API_KEY", "DASHSCOPE_API_KEY"],
        name if is_zai_alias(name) => vec!["ZAI_API_KEY"],
        "nvidia" | "nvidia-nim" | "build.nvidia.com" => vec!["NVIDIA_API_KEY"],
        "synthetic" => vec!["SYNTHETIC_API_KEY"],
        "opencode" | "opencode-zen" => vec!["OPENCODE_API_KEY"],
        "opencode-go" => vec!["OPENCODE_GO_API_KEY"],
        "vercel" | "vercel-ai" => vec!["VERCEL_API_KEY"],
        "cloudflare" | "cloudflare-ai" => vec!["CLOUDFLARE_API_KEY"],
        "ovhcloud" | "ovh" => vec!["OVH_AI_ENDPOINTS_ACCESS_TOKEN"],
        "astrai" => vec!["ASTRAI_API_KEY"],
        "avian" => vec!["AVIAN_API_KEY"],
        "deepmyst" | "deep-myst" => vec!["DEEPMYST_API_KEY"],
        "llamacpp" | "llama.cpp" => vec!["LLAMACPP_API_KEY"],
        "sglang" => vec!["SGLANG_API_KEY"],
        "vllm" => vec!["VLLM_API_KEY"],
        "aihubmix" => vec!["AIHUBMIX_API_KEY"],
        "siliconflow" | "silicon-flow" => vec!["SILICONFLOW_API_KEY"],
        "osaurus" => vec!["OSAURUS_API_KEY"],
        "telnyx" => vec!["TELNYX_API_KEY"],
        "azure_openai" | "azure-openai" | "azure" => vec!["AZURE_OPENAI_API_KEY"],
        _ => vec![],
    };

    for env_var in provider_env_candidates {
        if let Ok(value) = std::env::var(env_var) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    if is_minimax_alias(name) {
        if let Some(credential) = resolve_minimax_oauth_refresh_token(name) {
            return Some(credential);
        }
    }

    if minimax_oauth_placeholder_requested && is_minimax_alias(name) {
        return None;
    }

    for env_var in ["SEN_API_KEY", "API_KEY"] {
        if let Ok(value) = std::env::var(env_var) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    None
}

fn inference_allows_missing_credential(primary: &str, default_model: &str) -> bool {
    let base = primary.split(':').next().unwrap_or(primary).trim();
    if base.eq_ignore_ascii_case("ollama") {
        return !default_model.contains(":cloud");
    }
    base.eq_ignore_ascii_case("lmstudio")
        || base.eq_ignore_ascii_case("llamacpp")
        || base.eq_ignore_ascii_case("llama.cpp")
        || base.eq_ignore_ascii_case("sglang")
        || base.eq_ignore_ascii_case("vllm")
        || base.eq_ignore_ascii_case("osaurus")
        || base.eq_ignore_ascii_case("claude-code")
        || base.eq_ignore_ascii_case("gemini-cli")
        || base.eq_ignore_ascii_case("kilocli")
}

pub fn inference_api_credential_available(config: &crate::config::Config) -> bool {
    let primary = config.default_provider.as_deref().unwrap_or("openrouter");
    let default_model = config.default_model.as_deref().unwrap_or("");

    if inference_allows_missing_credential(primary, default_model) {
        return true;
    }

    let global = config.api_key.as_deref();
    let route_key = if !config.model_routes.is_empty() {
        config
            .model_routes
            .iter()
            .find(|r| r.provider.eq_ignore_ascii_case(primary))
            .and_then(|r| r.api_key.as_deref())
            .filter(|k| !k.trim().is_empty())
    } else {
        None
    };
    let effective = route_key.or(global);

    resolve_provider_credential(primary, effective).is_some()
}

fn check_api_key_prefix(provider_name: &str, key: &str) -> Option<&'static str> {

    let likely_provider = if key.starts_with("sk-ant-") {
        Some("anthropic")
    } else if key.starts_with("sk-or-") {
        Some("openrouter")
    } else if key.starts_with("sk-") {
        Some("openai")
    } else if key.starts_with("gsk_") {
        Some("groq")
    } else if key.starts_with("pplx-") {
        Some("perplexity")
    } else if key.starts_with("xai-") {
        Some("xai")
    } else if key.starts_with("nvapi-") {
        Some("nvidia")
    } else if key.starts_with("KEY-") {
        Some("telnyx")
    } else {
        None
    };

    let expected = likely_provider?;

    let matches = match provider_name {
        "anthropic" => expected == "anthropic",
        "openrouter" => expected == "openrouter",
        "openai" => expected == "openai",
        "groq" => expected == "groq",
        "perplexity" => expected == "perplexity",
        "xai" | "grok" => expected == "xai",
        "nvidia" | "nvidia-nim" | "build.nvidia.com" => expected == "nvidia",
        "telnyx" => expected == "telnyx",
        _ => return None,
    };

    if matches { None } else { Some(expected) }
}

fn parse_custom_provider_url(
    raw_url: &str,
    provider_label: &str,
    format_hint: &str,
) -> anyhow::Result<String> {
    let base_url = raw_url.trim();

    if base_url.is_empty() {
        anyhow::bail!("{provider_label} requires a URL. Format: {format_hint}");
    }

    let parsed = reqwest::Url::parse(base_url).map_err(|_| {
        anyhow::anyhow!("{provider_label} requires a valid URL. Format: {format_hint}")
    })?;

    match parsed.scheme() {
        "http" | "https" => Ok(base_url.to_string()),
        _ => anyhow::bail!(
            "{provider_label} requires an http:// or https:// URL. Format: {format_hint}"
        ),
    }
}

pub fn create_provider(name: &str, api_key: Option<&str>) -> anyhow::Result<Box<dyn Provider>> {
    create_provider_with_options(name, api_key, &ProviderRuntimeOptions::default())
}

pub fn create_provider_with_options(
    name: &str,
    api_key: Option<&str>,
    options: &ProviderRuntimeOptions,
) -> anyhow::Result<Box<dyn Provider>> {
    match name {
        "openai-codex" | "openai_codex" | "codex" => Ok(Box::new(
            openai::codex::OpenAiCodexProvider::new(options, api_key)?,
        )),
        _ => create_provider_with_url_and_options(name, api_key, None, options),
    }
}

pub fn create_provider_with_url(
    name: &str,
    api_key: Option<&str>,
    api_url: Option<&str>,
) -> anyhow::Result<Box<dyn Provider>> {
    create_provider_with_url_and_options(name, api_key, api_url, &ProviderRuntimeOptions::default())
}

pub fn create_provider_for_model(
    name: &str,
    model: &str,
    api_key: Option<&str>,
    api_url: Option<&str>,
    options: &ProviderRuntimeOptions,
) -> anyhow::Result<Box<dyn Provider>> {
    if matches!(name, "openai") && openai::responses::model_uses_responses_api(model) {
        return create_provider_with_url_and_options(
            "openai-responses",
            api_key,
            api_url,
            options,
        );
    }
    create_provider_with_url_and_options(name, api_key, api_url, options)
}

#[allow(clippy::too_many_lines)]
pub fn create_provider_with_url_and_options(
    name: &str,
    api_key: Option<&str>,
    api_url: Option<&str>,
    options: &ProviderRuntimeOptions,
) -> anyhow::Result<Box<dyn Provider>> {

    let compat = {
        let timeout = options.provider_timeout_secs;
        let reasoning_effort = options.reasoning_effort.clone();
        let extra_headers = options.extra_headers.clone();
        let api_path = options.api_path.clone();
        let max_tokens = options.provider_max_tokens;
        let context_windows = options.model_context_windows.clone();
        move |p: OpenAiCompatibleProvider| -> Box<dyn Provider> {
            let mut p = p;
            if let Some(t) = timeout {
                p = p.with_timeout_secs(t);
            }
            if let Some(ref effort) = reasoning_effort {
                p = p.with_reasoning_effort(Some(effort.clone()));
            }
            if !extra_headers.is_empty() {
                p = p.with_extra_headers(extra_headers.clone());
            }
            if api_path.is_some() {
                p = p.with_api_path(api_path.clone());
            }
            if let Some(mt) = max_tokens {
                p = p.with_max_tokens(Some(mt));
            }
            if !context_windows.is_empty() {
                p = p.with_model_context_windows(context_windows.clone());
            }
            Box::new(p)
        }
    };

    let qwen_oauth_context = is_qwen_oauth_alias(name).then(|| resolve_qwen_oauth_context(api_key));

    let resolved_credential = if let Some(context) = qwen_oauth_context.as_ref() {
        context.credential.clone()
    } else {
        resolve_provider_credential(name, api_key)
    }
    .map(|v| String::from_utf8(v.into_bytes()).unwrap_or_default());
    #[allow(clippy::option_as_ref_deref)]
    let key = resolved_credential.as_ref().map(String::as_str);

    if let Some(key_value) = key {
        let is_custom = name.starts_with("custom:") || name.starts_with("anthropic-custom:");
        let has_custom_url = api_url.map(str::trim).filter(|u| !u.is_empty()).is_some();
        if !is_custom && !has_custom_url {
            if let Some(likely_provider) = check_api_key_prefix(name, key_value) {
                let visible = &key_value[..key_value.len().min(8)];
                anyhow::bail!(
                    "API key prefix mismatch: key \"{visible}...\" looks like a \
                     {likely_provider} key, but provider \"{name}\" is selected. \
                     Set the correct provider-specific env var or use `-p {likely_provider}`."
                );
            }
        }
    }

    match name {
        "openai-codex" | "openai_codex" | "codex" => {
            let mut codex_options = options.clone();
            codex_options.provider_api_url = api_url
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .or_else(|| options.provider_api_url.clone());
            Ok(Box::new(openai::codex::OpenAiCodexProvider::new(
                &codex_options,
                key,
            )?))
        }

        "openrouter" => {
            let mut p = openrouter::OpenRouterProvider::new(key, options.provider_timeout_secs)
                .with_max_tokens(options.provider_max_tokens);
            if !options.model_context_windows.is_empty() {
                p = p.with_model_context_windows(options.model_context_windows.clone());
            }
            if !options.extra_headers.is_empty() {
                p = p.with_extra_headers(options.extra_headers.clone());
            }
            Ok(Box::new(p))
        }
        "anthropic" => {
            let mut p = anthropic::AnthropicProvider::new(key);
            if let Some(mt) = options.provider_max_tokens {
                p = p.with_max_tokens(mt);
            }
            if !options.extra_headers.is_empty() {
                p = p.with_extra_headers(options.extra_headers.clone());
            }
            Ok(Box::new(p))
        }
        "openai" => {
            let mut p = openai::OpenAiProvider::with_base_url(api_url, key);
            if let Some(mt) = options.provider_max_tokens {
                p = p.with_max_tokens(Some(mt));
            }
            if !options.extra_headers.is_empty() {
                p = p.with_extra_headers(options.extra_headers.clone());
            }
            Ok(Box::new(p))
        }
        "openai-responses" | "openai_responses" | "openai_responses_api" => {
            let mut p = openai::responses::OpenAiResponsesProvider::with_base_url(api_url, key);
            if let Some(mt) = options.provider_max_tokens {
                p = p.with_max_output_tokens(Some(mt));
            }
            if !options.extra_headers.is_empty() {
                p = p.with_extra_headers(options.extra_headers.clone());
            }
            Ok(Box::new(p))
        }

        "ollama" => {
            let env_url = std::env::var("SEN_PROVIDER_URL").ok();

            let api_url = env_url.as_deref().or(api_url);

            Ok(Box::new(ollama::OllamaProvider::new_with_reasoning(
                api_url,
                key,
                options.reasoning_enabled,
            )))
        }
        "gemini" | "google" | "google-gemini" => {
            let state_dir = options.sen_dir.clone().unwrap_or_else(|| {
                directories::UserDirs::new().map_or_else(
                    || PathBuf::from(".senweavercoding"),
                    |dirs| dirs.home_dir().join(".senweavercoding"),
                )
            });
            let auth_service = AuthService::new(&state_dir, options.secrets_encrypt);
            let mut p = gemini::GeminiProvider::new_with_auth(
                key,
                auth_service,
                options.auth_profile_override.clone(),
            );
            if !options.extra_headers.is_empty() {
                p = p.with_extra_headers(options.extra_headers.clone());
            }
            Ok(Box::new(p))
        }
        "telnyx" => Ok(Box::new(telnyx::TelnyxProvider::new(key))),

        "senweaver" | "sw" => {
            let base = std::env::var("SENWEAVER_BASE_URL")
                .unwrap_or_else(|_| SENWEAVER_BASE_URL.to_string());
            Ok(compat(OpenAiCompatibleProvider::new(
                "SenWeaver",
                &base,
                key,
                AuthStyle::Bearer,
            )))
        }
        "venice" => Ok(compat(
            OpenAiCompatibleProvider::new(
                "Venice",
                "https://api.venice.ai",
                key,
                AuthStyle::Bearer,
            )
            .without_native_tools(),
        )),
        "vercel" | "vercel-ai" => Ok(compat(OpenAiCompatibleProvider::new(
            "Vercel AI Gateway",
            VERCEL_AI_GATEWAY_BASE_URL,
            key,
            AuthStyle::Bearer,
        ))),
        "cloudflare" | "cloudflare-ai" => Ok(compat(OpenAiCompatibleProvider::new(
            "Cloudflare AI Gateway",
            "https://gateway.ai.cloudflare.com/v1",
            key,
            AuthStyle::Bearer,
        ))),
        name if moonshot_base_url(name).is_some() => Ok(compat(
            OpenAiCompatibleProvider::new_no_responses_fallback(
                "Moonshot",
                moonshot_base_url(name).expect("checked in guard"),
                key,
                AuthStyle::Bearer,
            ),
        )),
        "kimi-code" | "kimi_coding" | "kimi_for_coding" => {
            Ok(compat(OpenAiCompatibleProvider::new_with_user_agent(
                "Kimi Code",
                "https://api.kimi.com/coding/v1",
                key,
                AuthStyle::Bearer,
                "KimiCLI/0.77",
            )))
        }
        "synthetic" => Ok(compat(OpenAiCompatibleProvider::new(
            "Synthetic",
            "https://api.synthetic.new/openai/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "opencode" | "opencode-zen" => Ok(compat(OpenAiCompatibleProvider::new(
            "OpenCode Zen",
            "https://opencode.ai/zen/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "opencode-go" => Ok(compat(OpenAiCompatibleProvider::new(
            "OpenCode Go",
            "https://opencode.ai/zen/go/v1",
            key,
            AuthStyle::Bearer,
        ))),
        name if zai_base_url(name).is_some() => Ok(compat(OpenAiCompatibleProvider::new(
            "Z.AI",
            zai_base_url(name).expect("checked in guard"),
            key,
            AuthStyle::Bearer,
        ))),
        name if glm_base_url(name).is_some() => {
            Ok(compat(OpenAiCompatibleProvider::new_no_responses_fallback(
                "GLM",
                glm_base_url(name).expect("checked in guard"),
                key,
                AuthStyle::Bearer,
            )))
        }
        name if minimax_base_url(name).is_some() => Ok(compat(
            OpenAiCompatibleProvider::new_merge_system_into_user(
                "MiniMax",
                minimax_base_url(name).expect("checked in guard"),
                key,
                AuthStyle::Bearer,
            ),
        )),
        "azure_openai" | "azure-openai" | "azure" => {
            let resource = std::env::var("AZURE_OPENAI_RESOURCE")
                .unwrap_or_else(|_| "my-resource".to_string());
            let deployment = match std::env::var("AZURE_OPENAI_DEPLOYMENT") {
                Ok(v) if !v.trim().is_empty() => v,
                _ => {
                    anyhow::bail!(
                        "no_model_configured: 未配置 AZURE_OPENAI_DEPLOYMENT 或 model_providers.*.azure_openai_deployment (please set AZURE_OPENAI_DEPLOYMENT env var or add azure_openai_deployment in Provider settings)"
                    );
                }
            };
            let api_version = std::env::var("AZURE_OPENAI_API_VERSION").ok();
            Ok(Box::new(azure_openai::AzureOpenAiProvider::new(
                key,
                &resource,
                &deployment,
                api_version.as_deref(),
            )))
        }
        "bedrock" | "aws-bedrock" => {
            let mut p = if let Some(api_key) = key {
                bedrock::BedrockProvider::with_bearer_token(api_key)
            } else {
                bedrock::BedrockProvider::new()
            };
            if let Some(mt) = options.provider_max_tokens {
                p = p.with_max_tokens(mt);
            }
            Ok(Box::new(p))
        }
        name if is_qwen_oauth_alias(name) => {
            let base_url = api_url
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .or_else(|| {
                    qwen_oauth_context
                        .as_ref()
                        .and_then(|context| context.base_url.clone())
                })
                .unwrap_or_else(|| QWEN_OAUTH_BASE_FALLBACK_URL.to_string());

            Ok(compat(
                OpenAiCompatibleProvider::new_with_user_agent_and_vision(
                    "Qwen Code",
                    &base_url,
                    key,
                    AuthStyle::Bearer,
                    "QwenCode/1.0",
                    true,
                ),
            ))
        }
        name if is_qianfan_alias(name) => {
            let base_url = qianfan_base_url(api_url);
            Ok(compat(OpenAiCompatibleProvider::new(
                "Qianfan",
                &base_url,
                key,
                AuthStyle::Bearer,
            )))
        }
        name if is_doubao_alias(name) => Ok(compat(OpenAiCompatibleProvider::new(
            "Doubao",
            "https://ark.cn-beijing.volces.com/api/v3",
            key,
            AuthStyle::Bearer,
        ))),
        name if is_bailian_alias(name) => Ok(Box::new(
            OpenAiCompatibleProvider::new_with_user_agent_and_vision(
                "Bailian",
                BAILIAN_BASE_URL,
                key,
                AuthStyle::Bearer,
                "SenWeaverCoding/1.0",
                true,
            ),
        )),
        name if qwen_base_url(name).is_some() => {
            Ok(compat(OpenAiCompatibleProvider::new_with_vision(
                "Qwen",
                qwen_base_url(name).expect("checked in guard"),
                key,
                AuthStyle::Bearer,
                true,
            )))
        }

        "groq" => Ok(compat(OpenAiCompatibleProvider::new(
            "Groq",
            "https://api.groq.com/openai/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "mistral" => Ok(compat(OpenAiCompatibleProvider::new(
            "Mistral",
            "https://api.mistral.ai/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "xai" | "grok" => Ok(compat(OpenAiCompatibleProvider::new(
            "xAI",
            "https://api.x.ai",
            key,
            AuthStyle::Bearer,
        ))),
        "deepseek" => Ok(compat(OpenAiCompatibleProvider::new(
            "DeepSeek",
            "https://api.deepseek.com",
            key,
            AuthStyle::Bearer,
        ))),
        "together" | "together-ai" => Ok(compat(OpenAiCompatibleProvider::new(
            "Together AI",
            "https://api.together.xyz",
            key,
            AuthStyle::Bearer,
        ))),
        "fireworks" | "fireworks-ai" => Ok(compat(OpenAiCompatibleProvider::new(
            "Fireworks AI",
            "https://api.fireworks.ai/inference/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "novita" => Ok(compat(OpenAiCompatibleProvider::new(
            "Novita AI",
            "https://api.novita.ai/openai",
            key,
            AuthStyle::Bearer,
        ))),
        "perplexity" => Ok(compat(OpenAiCompatibleProvider::new(
            "Perplexity",
            "https://api.perplexity.ai",
            key,
            AuthStyle::Bearer,
        ))),
        "cohere" => Ok(compat(OpenAiCompatibleProvider::new(
            "Cohere",
            "https://api.cohere.com/compatibility",
            key,
            AuthStyle::Bearer,
        ))),
        "copilot" | "github-copilot" => Ok(Box::new(copilot::CopilotProvider::new(key))),
        "claude-code" => Ok(Box::new(claude_code::ClaudeCodeProvider::new())),
        "gemini-cli" => Ok(Box::new(gemini::cli::GeminiCliProvider::new())),
        "kilocli" | "kilo" => Ok(Box::new(kilocli::KiloCliProvider::new())),
        "lmstudio" | "lm-studio" => {
            let lm_studio_key = key
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("lm-studio");
            Ok(compat(OpenAiCompatibleProvider::new(
                "LM Studio",
                "http://localhost:1234/v1",
                Some(lm_studio_key),
                AuthStyle::Bearer,
            )))
        }
        "llamacpp" | "llama.cpp" => {
            let base_url = api_url
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("http://localhost:8080/v1");
            let llama_cpp_key = key
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("llama.cpp");
            Ok(compat(OpenAiCompatibleProvider::new_with_vision(
                "llama.cpp",
                base_url,
                Some(llama_cpp_key),
                AuthStyle::Bearer,
                true,
            )))
        }
        "sglang" => {
            let base_url = api_url
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("http://localhost:30000/v1");
            Ok(compat(OpenAiCompatibleProvider::new(
                "SGLang",
                base_url,
                key,
                AuthStyle::Bearer,
            )))
        }
        "vllm" => {
            let base_url = api_url
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("http://localhost:8000/v1");
            Ok(compat(OpenAiCompatibleProvider::new(
                "vLLM",
                base_url,
                key,
                AuthStyle::Bearer,
            )))
        }
        "osaurus" => {
            let base_url = api_url
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("http://localhost:1337/v1");
            let osaurus_key = key
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("osaurus");
            Ok(compat(OpenAiCompatibleProvider::new(
                "Osaurus",
                base_url,
                Some(osaurus_key),
                AuthStyle::Bearer,
            )))
        }
        "nvidia" | "nvidia-nim" | "build.nvidia.com" => {
            Ok(compat(OpenAiCompatibleProvider::new_no_responses_fallback(
                "NVIDIA NIM",
                "https://integrate.api.nvidia.com/v1",
                key,
                AuthStyle::Bearer,
            )))
        }

        "astrai" => Ok(compat(OpenAiCompatibleProvider::new(
            "Astrai",
            "https://as-trai.com/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "siliconflow" | "silicon-flow" => Ok(compat(OpenAiCompatibleProvider::new(
            "SiliconFlow",
            "https://api.siliconflow.cn/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "aihubmix" => Ok(compat(OpenAiCompatibleProvider::new(
            "AiHubMix",
            "https://aihubmix.com/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "litellm" | "lite-llm" => {
            let base_url = api_url
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("http://localhost:4000/v1");
            Ok(compat(OpenAiCompatibleProvider::new(
                "LiteLLM",
                base_url,
                key,
                AuthStyle::Bearer,
            )))
        }

        "cerebras" => Ok(compat(OpenAiCompatibleProvider::new(
            "Cerebras",
            "https://api.cerebras.ai/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "sambanova" => Ok(compat(OpenAiCompatibleProvider::new(
            "SambaNova",
            "https://api.sambanova.ai/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "hyperbolic" => Ok(compat(OpenAiCompatibleProvider::new(
            "Hyperbolic",
            "https://api.hyperbolic.xyz/v1",
            key,
            AuthStyle::Bearer,
        ))),

        "deepinfra" | "deep-infra" => Ok(compat(OpenAiCompatibleProvider::new(
            "DeepInfra",
            "https://api.deepinfra.com/v1/openai",
            key,
            AuthStyle::Bearer,
        ))),
        "huggingface" | "hf" => Ok(compat(OpenAiCompatibleProvider::new(
            "Hugging Face",
            "https://router.huggingface.co/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "ai21" | "ai21-labs" => Ok(compat(OpenAiCompatibleProvider::new(
            "AI21 Labs",
            "https://api.ai21.com/studio/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "reka" => Ok(compat(OpenAiCompatibleProvider::new(
            "Reka",
            "https://api.reka.ai/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "baseten" => Ok(compat(OpenAiCompatibleProvider::new(
            "Baseten",
            "https://inference.baseten.co/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "nscale" => Ok(compat(OpenAiCompatibleProvider::new(
            "Nscale",
            "https://inference.api.nscale.com/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "anyscale" => Ok(compat(OpenAiCompatibleProvider::new(
            "Anyscale",
            "https://api.endpoints.anyscale.com/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "nebius" => Ok(compat(OpenAiCompatibleProvider::new(
            "Nebius AI Studio",
            "https://api.studio.nebius.ai/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "friendli" | "friendliai" => Ok(compat(OpenAiCompatibleProvider::new(
            "Friendli AI",
            "https://api.friendli.ai/serverless/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "lepton" | "lepton-ai" => {
            let base_url = api_url
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("https://llama3-1-405b.lepton.run/api/v1");
            Ok(compat(OpenAiCompatibleProvider::new(
                "Lepton AI",
                base_url,
                key,
                AuthStyle::Bearer,
            )))
        }

        "stepfun" | "step" => Ok(compat(OpenAiCompatibleProvider::new(
            "Stepfun",
            "https://api.stepfun.com/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "baichuan" => Ok(compat(OpenAiCompatibleProvider::new(
            "Baichuan",
            "https://api.baichuan-ai.com/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "yi" | "01ai" | "lingyiwanwu" => Ok(compat(OpenAiCompatibleProvider::new(
            "01.AI (Yi)",
            "https://api.lingyiwanwu.com/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "hunyuan" | "tencent" => Ok(compat(OpenAiCompatibleProvider::new(
            "Tencent Hunyuan",
            "https://api.hunyuan.cloud.tencent.com/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "avian" => Ok(compat(OpenAiCompatibleProvider::new(
            "Avian",
            "https://api.avian.io/v1",
            key,
            AuthStyle::Bearer,
        ))),
        "deepmyst" | "deep-myst" => Ok(compat(OpenAiCompatibleProvider::new(
            "DeepMyst",
            "https://api.deepmyst.com/v1",
            key,
            AuthStyle::Bearer,
        ))),

        "ovhcloud" | "ovh" => {
            let mut p = openai::OpenAiProvider::with_base_url(
                Some("https://oai.endpoints.kepler.ai.cloud.ovh.net/v1"),
                key,
            );
            if !options.extra_headers.is_empty() {
                p = p.with_extra_headers(options.extra_headers.clone());
            }
            Ok(Box::new(p))
        }

        name if name.starts_with("custom:") => {
            let base_url = parse_custom_provider_url(
                name.strip_prefix("custom:").unwrap_or(""),
                "Custom provider",
                "custom:https://your-api.com",
            )?;
            Ok(compat(OpenAiCompatibleProvider::new_with_vision(
                "Custom",
                &base_url,
                key,
                AuthStyle::Bearer,
                true,
            )))
        }

        name if name.starts_with("anthropic-custom:") => {
            let base_url = parse_custom_provider_url(
                name.strip_prefix("anthropic-custom:").unwrap_or(""),
                "Anthropic-custom provider",
                "anthropic-custom:https://your-api.com",
            )?;
            let mut p =
                anthropic::AnthropicProvider::with_base_url(key, Some(&base_url));
            if let Some(mt) = options.provider_max_tokens {
                p = p.with_max_tokens(mt);
            }
            if !options.extra_headers.is_empty() {
                p = p.with_extra_headers(options.extra_headers.clone());
            }
            Ok(Box::new(p))
        }

        _ => anyhow::bail!(
            "Unknown provider: {name}. Check README for supported providers or run `sen onboard` to reconfigure.\n\
             Tip: Use \"custom:https://your-api.com\" for OpenAI-compatible endpoints.\n\
             Tip: Use \"anthropic-custom:https://your-api.com\" for Anthropic-compatible endpoints."
        ),
    }
}

fn parse_provider_profile(s: &str) -> (&str, Option<&str>) {
    if s.starts_with("custom:") || s.starts_with("anthropic-custom:") {
        return (s, None);
    }
    match s.split_once(':') {
        Some((provider, profile)) if !profile.is_empty() => (provider, Some(profile)),
        _ => (s, None),
    }
}

pub fn resolve_runtime_provider_name(
    saved_id: &str,
    cfg: &crate::config::Config,
) -> String {
    let trimmed = saved_id.trim();
    if trimmed.is_empty() {
        return saved_id.to_string();
    }

    if trimmed.starts_with("custom:") || trimmed.starts_with("anthropic-custom:") {
        return trimmed.to_string();
    }

    let Some(profile) = cfg.model_providers.get(trimmed) else {
        return trimmed.to_string();
    };

    let preset = profile
        .preset_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let base_url = profile
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let wire_api_lower = profile
        .wire_api
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase);
    let wire_lower = wire_api_lower.as_deref();

    let is_responses_wire = matches!(
        wire_lower,
        Some("responses") | Some("openai-responses") | Some("open-ai-responses")
    );
    let is_anthropic_wire = matches!(
        wire_lower,
        Some("anthropic") | Some("anthropic-messages") | Some("anthropic-chat")
    );
    let is_chat_wire = matches!(
        wire_lower,
        Some("chat_completions")
            | Some("chat-completions")
            | Some("openai-chat")
            | Some("openai_chat")
    );
    let wire_is_explicit = wire_lower.is_some();
    let preset_is_codex = matches!(
        preset,
        Some("openai-codex") | Some("openai_codex") | Some("codex")
    );
    let preset_is_anthropic = matches!(preset, Some("anthropic"));

    if is_responses_wire || (!wire_is_explicit && preset_is_codex) {
        return "openai-codex".to_string();
    }

    if matches!(preset, Some("anthropic-custom")) {
        if let Some(url) = base_url {
            return format!("anthropic-custom:{url}");
        }
    }

    if is_anthropic_wire {
        if let Some(url) = base_url {
            if !matches!(preset, Some("anthropic")) || !is_official_anthropic_base_url(url) {
                return format!("anthropic-custom:{url}");
            }
        }
    }

    if matches!(preset, Some("custom")) {
        if let Some(url) = base_url {
            return format!("custom:{url}");
        }
    }

    if is_chat_wire && (preset_is_codex || preset_is_anthropic) {
        if let Some(url) = base_url {
            return format!("custom:{url}");
        }
    }

    if let Some(preset) = preset {
        if matches!(preset, "anthropic") {
            if wire_is_explicit && !is_anthropic_wire {
                if let Some(url) = base_url {
                    return format!("custom:{url}");
                }
            }
            return "anthropic".to_string();
        }
        return preset.to_string();
    }

    if let Some(url) = base_url {
        return format!("custom:{url}");
    }

    trimmed.to_string()
}

fn is_official_anthropic_base_url(url: &str) -> bool {
    let trimmed = url.trim().trim_end_matches('/').to_ascii_lowercase();
    matches!(
        trimmed.as_str(),
        "https://api.anthropic.com"
            | "http://api.anthropic.com"
            | "https://api.anthropic.com/v1"
            | "http://api.anthropic.com/v1"
    )
}

pub fn profile_model_names(profile: &crate::config::ModelProviderConfig) -> Vec<String> {
    if !profile.model_names.is_empty() {
        return profile
            .model_names
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    for slot in ["main", "haiku", "sonnet", "opus"] {
        if let Some(value) = profile.models.get(slot) {
            let trimmed = value.trim();
            if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
                out.push(trimmed.to_string());
            }
        }
    }
    let mut keys: Vec<&String> = profile.models.keys().collect();
    keys.sort();
    for k in keys {
        if let Some(value) = profile.models.get(k) {
            let trimmed = value.trim();
            if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
                out.push(trimmed.to_string());
            }
        }
    }
    out
}

pub fn first_configured_model(config: &crate::config::Config) -> Option<String> {
    let mut order: Vec<String> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    if let Some(pid) = config
        .default_provider
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Some(profile) = config.model_providers.get(pid) {
            for name in profile_model_names(profile) {
                if seen.insert(name.clone()) {
                    order.push(name);
                }
            }
        }
    }
    let mut keys: Vec<&String> = config.model_providers.keys().collect();
    keys.sort();
    for k in keys {
        if let Some(profile) = config.model_providers.get(k) {
            for name in profile_model_names(profile) {
                if seen.insert(name.clone()) {
                    order.push(name);
                }
            }
        }
    }
    order.into_iter().next()
}

pub fn resolve_default_model(config: &crate::config::Config) -> anyhow::Result<String> {
    if let Some(m) = config
        .default_model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Ok(m.to_string());
    }
    if let Some(name) = first_configured_model(config) {
        return Ok(name);
    }
    Err(anyhow::anyhow!(
        "no_model_configured: 未添加模型，请先在提供商设置页添加至少一个模型 (no model configured; please add at least one model in Provider settings)"
    ))
}

pub fn create_resilient_provider(
    primary_name: &str,
    api_key: Option<&str>,
    api_url: Option<&str>,
    reliability: &crate::config::ReliabilityConfig,
) -> anyhow::Result<Box<dyn Provider>> {
    create_resilient_provider_with_options(
        primary_name,
        api_key,
        api_url,
        reliability,
        &ProviderRuntimeOptions::default(),
    )
}

pub fn create_resilient_provider_with_options(
    primary_name: &str,
    api_key: Option<&str>,
    api_url: Option<&str>,
    reliability: &crate::config::ReliabilityConfig,
    options: &ProviderRuntimeOptions,
) -> anyhow::Result<Box<dyn Provider>> {
    let mut providers: Vec<(String, Box<dyn Provider>)> = Vec::new();

    let primary_provider = match primary_name {
        "openai-codex" | "openai_codex" | "codex" => {
            create_provider_with_options(primary_name, api_key, options)?
        }
        _ => create_provider_with_url_and_options(primary_name, api_key, api_url, options)?,
    };
    providers.push((primary_name.to_string(), primary_provider));

    for fallback in &reliability.fallback_providers {
        if fallback == primary_name || providers.iter().any(|(name, _)| name == fallback) {
            continue;
        }

        let (provider_name, profile_override) = parse_provider_profile(fallback);

        let fallback_options = match profile_override {
            Some(profile) => {
                let mut opts = options.clone();
                opts.auth_profile_override = Some(profile.to_string());
                opts
            }
            None => options.clone(),
        };

        match create_provider_with_options(provider_name, None, &fallback_options) {
            Ok(provider) => providers.push((fallback.clone(), provider)),
            Err(_error) => {
                tracing::warn!(
                    fallback_provider = fallback,
                    "Ignoring invalid fallback provider during initialization"
                );
            }
        }
    }

    let reliable = ReliableProvider::new(
        providers,
        reliability.provider_retries,
        reliability.provider_backoff_ms,
    )
    .with_api_keys(reliability.api_keys.clone())
    .with_model_fallbacks(reliability.model_fallbacks.clone())
    .with_retry_caps(
        reliability.engine_overload_max_retries,
        reliability.account_rate_limit_max_retries,
    )
    .with_transient_max_retries(reliability.transient_max_retries)
    .with_client_rate_limit_enabled(reliability.client_llm_rate_limit_enabled);

    Ok(Box::new(reliable))
}

pub fn create_routed_provider(
    primary_name: &str,
    api_key: Option<&str>,
    api_url: Option<&str>,
    reliability: &crate::config::ReliabilityConfig,
    model_routes: &[crate::config::ModelRouteConfig],
    default_model: &str,
) -> anyhow::Result<Box<dyn Provider>> {
    create_routed_provider_with_options(
        primary_name,
        api_key,
        api_url,
        reliability,
        model_routes,
        default_model,
        &ProviderRuntimeOptions::default(),
    )
}

pub fn create_routed_provider_with_options(
    primary_name: &str,
    api_key: Option<&str>,
    api_url: Option<&str>,
    reliability: &crate::config::ReliabilityConfig,
    model_routes: &[crate::config::ModelRouteConfig],
    default_model: &str,
    options: &ProviderRuntimeOptions,
) -> anyhow::Result<Box<dyn Provider>> {
    if model_routes.is_empty() {
        return create_resilient_provider_with_options(
            primary_name,
            api_key,
            api_url,
            reliability,
            options,
        );
    }

    let mut needed: Vec<String> = vec![primary_name.to_string()];
    for route in model_routes {
        if !needed.iter().any(|n| n == &route.provider) {
            needed.push(route.provider.clone());
        }
    }

    let mut providers: Vec<(String, Box<dyn Provider>)> = Vec::new();
    for name in &needed {
        let routed_credential = model_routes
            .iter()
            .find(|r| &r.provider == name)
            .and_then(|r| {
                r.api_key.as_ref().and_then(|raw_key| {
                    let trimmed_key = raw_key.trim();
                    (!trimmed_key.is_empty()).then_some(trimmed_key)
                })
            });
        let key = routed_credential.or(api_key);

        let url = if name == primary_name { api_url } else { None };
        match create_resilient_provider_with_options(name, key, url, reliability, options) {
            Ok(provider) => providers.push((name.clone(), provider)),
            Err(e) => {
                if name == primary_name {
                    return Err(e);
                }
                tracing::warn!(
                    provider = name.as_str(),
                    "Ignoring routed provider that failed to initialize"
                );
            }
        }
    }

    let routes: Vec<(String, router::Route)> = model_routes
        .iter()
        .map(|r| {
            (
                r.hint.clone(),
                router::Route {
                    provider_name: r.provider.clone(),
                    model: r.model.clone(),
                },
            )
        })
        .collect();

    Ok(Box::new(router::RouterProvider::new(
        providers,
        routes,
        default_model.to_string(),
    )))
}

#[allow(clippy::too_many_arguments)]
pub async fn create_routed_provider_with_options_async(
    primary_name: String,
    api_key: Option<String>,
    api_url: Option<String>,
    reliability: crate::config::ReliabilityConfig,
    model_routes: Vec<crate::config::ModelRouteConfig>,
    default_model: String,
    options: ProviderRuntimeOptions,
) -> anyhow::Result<Box<dyn Provider>> {
    tokio::task::spawn_blocking(move || {
        create_routed_provider_with_options(
            &primary_name,
            api_key.as_deref(),
            api_url.as_deref(),
            &reliability,
            &model_routes,
            &default_model,
            &options,
        )
    })
    .await
    .map_err(|e| anyhow::anyhow!("provider initialization task failed: {e}"))?
}

pub async fn create_provider_async(
    name: String,
    api_key: Option<String>,
) -> anyhow::Result<Box<dyn Provider>> {
    tokio::task::spawn_blocking(move || create_provider(&name, api_key.as_deref()))
        .await
        .map_err(|e| anyhow::anyhow!("provider initialization task failed: {e}"))?
}

pub async fn create_provider_with_options_async(
    name: String,
    api_key: Option<String>,
    options: ProviderRuntimeOptions,
) -> anyhow::Result<Box<dyn Provider>> {
    tokio::task::spawn_blocking(move || {
        create_provider_with_options(&name, api_key.as_deref(), &options)
    })
    .await
    .map_err(|e| anyhow::anyhow!("provider initialization task failed: {e}"))?
}

pub async fn create_provider_with_url_async(
    name: String,
    api_key: Option<String>,
    api_url: Option<String>,
) -> anyhow::Result<Box<dyn Provider>> {
    tokio::task::spawn_blocking(move || {
        create_provider_with_url(&name, api_key.as_deref(), api_url.as_deref())
    })
    .await
    .map_err(|e| anyhow::anyhow!("provider initialization task failed: {e}"))?
}

pub async fn create_provider_with_url_and_options_async(
    name: String,
    api_key: Option<String>,
    api_url: Option<String>,
    options: ProviderRuntimeOptions,
) -> anyhow::Result<Box<dyn Provider>> {
    tokio::task::spawn_blocking(move || {
        create_provider_with_url_and_options(&name, api_key.as_deref(), api_url.as_deref(), &options)
    })
    .await
    .map_err(|e| anyhow::anyhow!("provider initialization task failed: {e}"))?
}

pub async fn create_resilient_provider_with_options_async(
    primary_name: String,
    api_key: Option<String>,
    api_url: Option<String>,
    reliability: crate::config::ReliabilityConfig,
    options: ProviderRuntimeOptions,
) -> anyhow::Result<Box<dyn Provider>> {
    tokio::task::spawn_blocking(move || {
        create_resilient_provider_with_options(
            &primary_name,
            api_key.as_deref(),
            api_url.as_deref(),
            &reliability,
            &options,
        )
    })
    .await
    .map_err(|e| anyhow::anyhow!("provider initialization task failed: {e}"))?
}

pub struct ProviderInfo {

    pub name: &'static str,

    pub display_name: &'static str,

    pub aliases: &'static [&'static str],

    pub local: bool,
}

pub fn provider_display_name(provider_name: &str) -> String {
    list_providers()
        .into_iter()
        .find(|p| p.name == provider_name)
        .map(|p| p.display_name.to_string())
        .unwrap_or_else(|| provider_name.to_string())
}

pub fn list_providers() -> Vec<ProviderInfo> {
    vec![

        ProviderInfo {
            name: "openrouter",
            display_name: "OpenRouter",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "anthropic",
            display_name: "Anthropic",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "openai",
            display_name: "OpenAI",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "openai-codex",
            display_name: "OpenAI Codex (OAuth)",
            aliases: &["openai_codex", "codex"],
            local: false,
        },
        ProviderInfo {
            name: "telnyx",
            display_name: "Telnyx",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "azure_openai",
            display_name: "Azure OpenAI",
            aliases: &["azure-openai", "azure"],
            local: false,
        },
        ProviderInfo {
            name: "ollama",
            display_name: "Ollama",
            aliases: &[],
            local: true,
        },
        ProviderInfo {
            name: "gemini",
            display_name: "Google Gemini",
            aliases: &["google", "google-gemini"],
            local: false,
        },

        ProviderInfo {
            name: "senweaver",
            display_name: "SenWeaver",
            aliases: &["sw"],
            local: false,
        },
        ProviderInfo {
            name: "venice",
            display_name: "Venice",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "vercel",
            display_name: "Vercel AI Gateway",
            aliases: &["vercel-ai"],
            local: false,
        },
        ProviderInfo {
            name: "cloudflare",
            display_name: "Cloudflare AI",
            aliases: &["cloudflare-ai"],
            local: false,
        },
        ProviderInfo {
            name: "moonshot",
            display_name: "Moonshot",
            aliases: &["kimi"],
            local: false,
        },
        ProviderInfo {
            name: "kimi-code",
            display_name: "Kimi Code",
            aliases: &["kimi_coding", "kimi_for_coding"],
            local: false,
        },
        ProviderInfo {
            name: "synthetic",
            display_name: "Synthetic",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "opencode",
            display_name: "OpenCode Zen",
            aliases: &["opencode-zen"],
            local: false,
        },
        ProviderInfo {
            name: "opencode-go",
            display_name: "OpenCode Go",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "zai",
            display_name: "Z.AI",
            aliases: &["z.ai"],
            local: false,
        },
        ProviderInfo {
            name: "glm",
            display_name: "GLM (Zhipu)",
            aliases: &["zhipu"],
            local: false,
        },
        ProviderInfo {
            name: "minimax",
            display_name: "MiniMax",
            aliases: &[
                "minimax-intl",
                "minimax-io",
                "minimax-global",
                "minimax-cn",
                "minimaxi",
                "minimax-oauth",
                "minimax-oauth-cn",
                "minimax-portal",
                "minimax-portal-cn",
            ],
            local: false,
        },
        ProviderInfo {
            name: "bedrock",
            display_name: "Amazon Bedrock",
            aliases: &["aws-bedrock"],
            local: false,
        },
        ProviderInfo {
            name: "qianfan",
            display_name: "Qianfan (Baidu)",
            aliases: &["baidu"],
            local: false,
        },
        ProviderInfo {
            name: "doubao",
            display_name: "Doubao (Volcengine)",
            aliases: &["volcengine", "ark", "doubao-cn"],
            local: false,
        },
        ProviderInfo {
            name: "qwen",
            display_name: "Qwen (DashScope / Qwen Code OAuth)",
            aliases: &[
                "dashscope",
                "qwen-intl",
                "dashscope-intl",
                "qwen-us",
                "dashscope-us",
                "qwen-code",
                "qwen-oauth",
                "qwen_oauth",
            ],
            local: false,
        },
        ProviderInfo {
            name: "bailian",
            display_name: "Bailian (Aliyun)",
            aliases: &["aliyun-bailian", "aliyun"],
            local: false,
        },
        ProviderInfo {
            name: "groq",
            display_name: "Groq",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "mistral",
            display_name: "Mistral",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "xai",
            display_name: "xAI (Grok)",
            aliases: &["grok"],
            local: false,
        },
        ProviderInfo {
            name: "deepseek",
            display_name: "DeepSeek",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "together",
            display_name: "Together AI",
            aliases: &["together-ai"],
            local: false,
        },
        ProviderInfo {
            name: "fireworks",
            display_name: "Fireworks AI",
            aliases: &["fireworks-ai"],
            local: false,
        },
        ProviderInfo {
            name: "novita",
            display_name: "Novita AI",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "perplexity",
            display_name: "Perplexity",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "cohere",
            display_name: "Cohere",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "copilot",
            display_name: "GitHub Copilot",
            aliases: &["github-copilot"],
            local: false,
        },
        ProviderInfo {
            name: "claude-code",
            display_name: "Claude Code (CLI)",
            aliases: &[],
            local: true,
        },
        ProviderInfo {
            name: "gemini-cli",
            display_name: "Gemini CLI",
            aliases: &[],
            local: true,
        },
        ProviderInfo {
            name: "kilocli",
            display_name: "KiloCLI",
            aliases: &["kilo"],
            local: true,
        },
        ProviderInfo {
            name: "lmstudio",
            display_name: "LM Studio",
            aliases: &["lm-studio"],
            local: true,
        },
        ProviderInfo {
            name: "llamacpp",
            display_name: "llama.cpp server",
            aliases: &["llama.cpp"],
            local: true,
        },
        ProviderInfo {
            name: "sglang",
            display_name: "SGLang",
            aliases: &[],
            local: true,
        },
        ProviderInfo {
            name: "vllm",
            display_name: "vLLM",
            aliases: &[],
            local: true,
        },
        ProviderInfo {
            name: "osaurus",
            display_name: "Osaurus",
            aliases: &[],
            local: true,
        },
        ProviderInfo {
            name: "nvidia",
            display_name: "NVIDIA NIM",
            aliases: &["nvidia-nim", "build.nvidia.com"],
            local: false,
        },
        ProviderInfo {
            name: "siliconflow",
            display_name: "SiliconFlow",
            aliases: &["silicon-flow"],
            local: false,
        },
        ProviderInfo {
            name: "aihubmix",
            display_name: "AiHubMix",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "litellm",
            display_name: "LiteLLM",
            aliases: &["lite-llm"],
            local: false,
        },

        ProviderInfo {
            name: "cerebras",
            display_name: "Cerebras",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "sambanova",
            display_name: "SambaNova",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "hyperbolic",
            display_name: "Hyperbolic",
            aliases: &[],
            local: false,
        },

        ProviderInfo {
            name: "deepinfra",
            display_name: "DeepInfra",
            aliases: &["deep-infra"],
            local: false,
        },
        ProviderInfo {
            name: "huggingface",
            display_name: "Hugging Face",
            aliases: &["hf"],
            local: false,
        },
        ProviderInfo {
            name: "ai21",
            display_name: "AI21 Labs",
            aliases: &["ai21-labs"],
            local: false,
        },
        ProviderInfo {
            name: "reka",
            display_name: "Reka",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "baseten",
            display_name: "Baseten",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "nscale",
            display_name: "Nscale",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "anyscale",
            display_name: "Anyscale",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "nebius",
            display_name: "Nebius AI Studio",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "friendli",
            display_name: "Friendli AI",
            aliases: &["friendliai"],
            local: false,
        },
        ProviderInfo {
            name: "lepton",
            display_name: "Lepton AI",
            aliases: &["lepton-ai"],
            local: false,
        },

        ProviderInfo {
            name: "stepfun",
            display_name: "Stepfun",
            aliases: &["step"],
            local: false,
        },
        ProviderInfo {
            name: "baichuan",
            display_name: "Baichuan",
            aliases: &[],
            local: false,
        },
        ProviderInfo {
            name: "yi",
            display_name: "01.AI (Yi)",
            aliases: &["01ai", "lingyiwanwu"],
            local: false,
        },
        ProviderInfo {
            name: "hunyuan",
            display_name: "Tencent Hunyuan",
            aliases: &["tencent"],
            local: false,
        },

        ProviderInfo {
            name: "ovhcloud",
            display_name: "OVHcloud AI Endpoints",
            aliases: &["ovh"],
            local: false,
        },
        ProviderInfo {
            name: "avian",
            display_name: "Avian",
            aliases: &[],
            local: false,
        },
    ]
}
