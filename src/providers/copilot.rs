// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::providers::traits::{
    ChatMessage, ChatRequest as ProviderChatRequest, ChatResponse as ProviderChatResponse,
    Provider, TokenUsage, ToolCall as ProviderToolCall,
};
use crate::tools::ToolSpec;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::warn;

const GITHUB_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const GITHUB_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const GITHUB_ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_API_KEY_URL: &str = "https://api.github.com/copilot_internal/v2/token";
const DEFAULT_API: &str = "https://api.githubcopilot.com";

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default = "default_interval")]
    interval: u64,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
}

fn default_interval() -> u64 {
    5
}

fn default_expires_in() -> u64 {
    900
}

#[derive(Debug, Deserialize)]
struct AccessTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiKeyInfo {
    token: String,
    expires_at: i64,
    #[serde(default)]
    endpoints: Option<ApiEndpoints>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiEndpoints {
    api: Option<String>,
}

struct CachedApiKey {
    token: String,
    api_endpoint: String,
    expires_at: i64,
}

#[derive(Debug, Serialize)]
struct ApiChatRequest<'a> {
    model: String,
    messages: Vec<ApiMessage>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<NativeToolSpec<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ApiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<ApiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "crate::providers::sanitize::skip_serializing_tool_calls")]
    tool_calls: Option<Vec<NativeToolCall>>,
}

#[derive(Debug, Serialize)]
struct NativeToolSpec<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    function: NativeToolFunctionSpec<'a>,
}

#[derive(Debug, Serialize)]
struct NativeToolFunctionSpec<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct NativeToolCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    function: NativeFunctionCall,
}

#[derive(Debug, Serialize, Deserialize)]
struct NativeFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum ApiContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrlDetail },
}

#[derive(Debug, Clone, Serialize)]
struct ImageUrlDetail {
    url: String,
}

#[derive(Debug, Deserialize)]
struct ApiChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
struct UsageInfo {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens_details: Option<UsageCompletionTokensDetails>,
}

#[derive(Debug, Deserialize)]
struct UsageCompletionTokensDetails {
    #[serde(default)]
    reasoning_tokens: Option<u64>,
}

impl UsageInfo {
    fn reasoning_output_tokens(&self) -> Option<u64> {
        self.completion_tokens_details
            .as_ref()
            .and_then(|d| d.reasoning_tokens)
    }
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<NativeToolCall>>,
}

#[derive(Clone)]
pub struct CopilotProvider {
    github_token: Option<String>,

    refresh_lock: Arc<Mutex<Option<CachedApiKey>>>,
    token_dir: PathBuf,
}

impl CopilotProvider {
    pub fn new(github_token: Option<&str>) -> Self {
        let token_dir = directories::ProjectDirs::from("", "", "sen")
            .map(|dir| dir.config_dir().join("copilot"))
            .unwrap_or_else(|| {

                let user = std::env::var("USER")
                    .or_else(|_| std::env::var("USERNAME"))
                    .unwrap_or_else(|_| "unknown".to_string());
                std::env::temp_dir().join(format!("sen-copilot-{user}"))
            });

        if let Err(err) = std::fs::create_dir_all(&token_dir) {
            warn!(
                "Failed to create Copilot token directory {:?}: {err}. Token caching is disabled.",
                token_dir
            );
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                if let Err(err) =
                    std::fs::set_permissions(&token_dir, std::fs::Permissions::from_mode(0o700))
                {
                    warn!(
                        "Failed to set Copilot token directory permissions on {:?}: {err}",
                        token_dir
                    );
                }
            }
            #[cfg(windows)]
            restrict_dir_to_current_user(&token_dir);
        }

        Self {
            github_token: github_token
                .filter(|token| !token.is_empty())
                .map(String::from),
            refresh_lock: Arc::new(Mutex::new(None)),
            token_dir,
        }
    }

    fn http_client(&self) -> Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts("provider.copilot", 120, 10)
    }

    fn stream_http_client(&self) -> Client {
        let mut headers = reqwest::header::HeaderMap::new();
        for (header, value) in &Self::COPILOT_HEADERS {
            if let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(header.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                headers.insert(name, val);
            }
        }
        crate::services::require_services()
            .proxy_runtime()
            .build_stream_client("provider.copilot.stream", 300, 10, &headers)
    }

    const COPILOT_HEADERS: [(&str, &str); 4] = [
        ("Editor-Version", "vscode/1.85.1"),
        ("Editor-Plugin-Version", "copilot/1.155.0"),
        ("User-Agent", "GithubCopilot/1.155.0"),
        ("Accept", "application/json"),
    ];

    fn convert_tools(tools: Option<&[ToolSpec]>) -> Option<Vec<NativeToolSpec<'_>>> {
        tools.map(|items| {
            let mut seen: std::collections::HashSet<&str> =
                std::collections::HashSet::with_capacity(items.len());
            items
                .iter()
                .filter(|tool| seen.insert(tool.name.as_str()))
                .map(|tool| NativeToolSpec {
                    kind: "function",
                    function: NativeToolFunctionSpec {
                        name: &tool.name,
                        description: &tool.description,
                        parameters: &tool.parameters,
                    },
                })
                .collect()
        })
    }

    fn to_api_content(role: &str, content: &str) -> Option<ApiContent> {
        if role != "user" {
            return Some(ApiContent::Text(content.to_string()));
        }

        let (cleaned_text, image_refs) = crate::multimodal::parse_image_markers(content);
        if image_refs.is_empty() {
            return Some(ApiContent::Text(content.to_string()));
        }

        let mut parts = Vec::with_capacity(image_refs.len() + 1);
        let trimmed = cleaned_text.trim();
        if !trimmed.is_empty() {
            parts.push(ContentPart::Text {
                text: trimmed.to_string(),
            });
        }
        for image_ref in image_refs {
            parts.push(ContentPart::ImageUrl {
                image_url: ImageUrlDetail { url: image_ref },
            });
        }

        Some(ApiContent::Parts(parts))
    }

    fn convert_messages(messages: &[ChatMessage]) -> Vec<ApiMessage> {
        messages
            .iter()
            .map(|message| {
                if message.role == "assistant" {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&message.content) {
                        if let Some(tool_calls_value) = value.get("tool_calls") {
                            if let Ok(parsed_calls) =
                                serde_json::from_value::<Vec<ProviderToolCall>>(tool_calls_value.clone())
                            {
                                let tool_calls = parsed_calls
                                    .into_iter()
                                    .map(|tool_call| NativeToolCall {
                                        id: Some(tool_call.id),
                                        kind: Some("function".to_string()),
                                        function: NativeFunctionCall {
                                            name: tool_call.name,
                                            arguments: tool_call.arguments,
                                        },
                                    })
                                    .collect::<Vec<_>>();

                                let content = value
                                    .get("content")
                                    .and_then(serde_json::Value::as_str)
                                    .map(|s| ApiContent::Text(s.to_string()));

                                return ApiMessage {
                                    role: "assistant".to_string(),
                                    content,
                                    tool_call_id: None,
                                    tool_calls: Some(tool_calls),
                                };
                            }
                        }
                    }
                }

                if message.role == "tool" {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&message.content) {
                        let tool_call_id = value
                            .get("tool_call_id")
                            .and_then(serde_json::Value::as_str)
                            .map(ToString::to_string);
                        let content = value
                            .get("content")
                            .and_then(serde_json::Value::as_str)
                            .map(|s| ApiContent::Text(s.to_string()));

                        return ApiMessage {
                            role: "tool".to_string(),
                            content,
                            tool_call_id,
                            tool_calls: None,
                        };
                    }
                }

                ApiMessage {
                    role: message.role.clone(),
                    content: Self::to_api_content(&message.role, &message.content),
                    tool_call_id: None,
                    tool_calls: None,
                }
            })
            .collect()
    }

    async fn send_chat_request(
        &self,
        messages: Vec<ApiMessage>,
        tools: Option<&[ToolSpec]>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let (token, endpoint) = self.get_api_key().await?;
        let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));

        let native_tools = Self::convert_tools(tools);
        let request = ApiChatRequest {
            model: model.to_string(),
            messages,
            temperature,
            tool_choice: native_tools.as_ref().map(|_| "auto".to_string()),
            tools: native_tools,
            stream: None,
        };

        let mut req = crate::providers::core::idempotency::apply_idempotency_header(
            self.http_client().post(&url),
        )
        .header("Authorization", format!("Bearer {token}"))
        .json(&request);

        for (header, value) in &Self::COPILOT_HEADERS {
            req = req.header(*header, *value);
        }

        let response = req.send().await?;

        if !response.status().is_success() {
            return Err(super::api_error("GitHub Copilot", response).await);
        }

        let api_response: ApiChatResponse = response.json().await?;
        let usage = api_response.usage.map(|u| TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            reasoning_tokens: u.reasoning_output_tokens(),
        });
        let choice = api_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No response from GitHub Copilot"))?;

        let tool_calls = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tool_call| {
                let arguments = crate::providers::sanitize::normalize_tool_call_arguments(
                    &tool_call.function.name,
                    tool_call.function.arguments,
                );
                ProviderToolCall {
                    id: tool_call
                        .id
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    name: tool_call.function.name,
                    arguments,
                }
            })
            .collect();

        Ok(ProviderChatResponse {
            text: choice.message.content,
            tool_calls,
            usage,
            reasoning_content: None,
            thinking_signature: None,
            stop_reason: choice
                .finish_reason
                .as_deref()
                .and_then(crate::providers::traits::StopReason::from_wire),
        })
    }

    async fn get_api_key(&self) -> anyhow::Result<(String, String)> {
        {
            let cached = self.refresh_lock.lock().await;
            if let Some(cached_key) = cached.as_ref() {
                if chrono::Utc::now().timestamp() + 120 < cached_key.expires_at {
                    return Ok((cached_key.token.clone(), cached_key.api_endpoint.clone()));
                }
            }
        }

        if let Some(info) = self.load_api_key_from_disk().await {
            if chrono::Utc::now().timestamp() + 120 < info.expires_at {
                let endpoint = info
                    .endpoints
                    .as_ref()
                    .and_then(|e| e.api.clone())
                    .unwrap_or_else(|| DEFAULT_API.to_string());
                let token = info.token;

                let mut cached = self.refresh_lock.lock().await;
                *cached = Some(CachedApiKey {
                    token: token.clone(),
                    api_endpoint: endpoint.clone(),
                    expires_at: info.expires_at,
                });
                return Ok((token, endpoint));
            }
        }

        let access_token = self.get_github_access_token().await?;
        let api_key_info = self.exchange_for_api_key(&access_token).await?;
        self.save_api_key_to_disk(&api_key_info).await;

        let endpoint = api_key_info
            .endpoints
            .as_ref()
            .and_then(|e| e.api.clone())
            .unwrap_or_else(|| DEFAULT_API.to_string());

        let mut cached = self.refresh_lock.lock().await;
        if let Some(existing) = cached.as_ref() {
            if chrono::Utc::now().timestamp() + 120 < existing.expires_at {
                return Ok((existing.token.clone(), existing.api_endpoint.clone()));
            }
        }
        *cached = Some(CachedApiKey {
            token: api_key_info.token.clone(),
            api_endpoint: endpoint.clone(),
            expires_at: api_key_info.expires_at,
        });

        Ok((api_key_info.token, endpoint))
    }

    async fn get_github_access_token(&self) -> anyhow::Result<String> {
        if let Some(token) = &self.github_token {
            return Ok(token.clone());
        }

        let access_token_path = self.token_dir.join("access-token");
        let secrets = crate::security::secrets::SecretStore::new(&self.token_dir, true);
        if let Ok(cached) = tokio::fs::read_to_string(&access_token_path).await {
            let decrypted = secrets.decrypt(cached.trim()).unwrap_or_else(|_| cached.clone());
            let token = decrypted.trim();
            if !token.is_empty() {
                return Ok(token.to_string());
            }
        }

        if !std::io::IsTerminal::is_terminal(&std::io::stderr()) {
            anyhow::bail!(
                "GitHub Copilot is not authenticated and no interactive terminal is available. \
                 Set GITHUB_TOKEN (or provide the provider API key) or run an interactive \
                 `sen agent -p copilot` session once to complete the device-code login."
            );
        }

        let token = self.device_code_login().await?;
        let to_store = secrets.encrypt(&token).unwrap_or_else(|_| token.clone());
        write_file_secure(&access_token_path, &to_store).await;
        Ok(token)
    }

    async fn device_code_login(&self) -> anyhow::Result<String> {
        let response: DeviceCodeResponse = self
            .http_client()
            .post(GITHUB_DEVICE_CODE_URL)
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "client_id": GITHUB_CLIENT_ID,
                "scope": "read:user"
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let mut poll_interval = Duration::from_secs(response.interval.max(5));
        let expires_in = response.expires_in.max(1);
        let expires_at = tokio::time::Instant::now() + Duration::from_secs(expires_in);

        tracing::info!(
            "GitHub Copilot authentication is required. Visit: {} | Code: {} | Waiting for authorization...",
            response.verification_uri,
            response.user_code
        );

        while tokio::time::Instant::now() < expires_at {
            tokio::time::sleep(poll_interval).await;

            let token_response: AccessTokenResponse = self
                .http_client()
                .post(GITHUB_ACCESS_TOKEN_URL)
                .header("Accept", "application/json")
                .json(&serde_json::json!({
                    "client_id": GITHUB_CLIENT_ID,
                    "device_code": response.device_code,
                    "grant_type": "urn:ietf:params:oauth:grant-type:device_code"
                }))
                .send()
                .await?
                .json()
                .await?;

            if let Some(token) = token_response.access_token {
                tracing::info!("GitHub Copilot authentication succeeded.");
                return Ok(token);
            }

            match token_response.error.as_deref() {
                Some("slow_down") => {
                    poll_interval += Duration::from_secs(5);
                }
                Some("authorization_pending") | None => {}
                Some("expired_token") => {
                    anyhow::bail!("GitHub device authorization expired")
                }
                Some(error) => anyhow::bail!("GitHub auth failed: {error}"),
            }
        }

        anyhow::bail!("Timed out waiting for GitHub authorization")
    }

    async fn exchange_for_api_key(&self, access_token: &str) -> anyhow::Result<ApiKeyInfo> {
        let mut request = self.http_client().get(GITHUB_API_KEY_URL);
        for (header, value) in &Self::COPILOT_HEADERS {
            request = request.header(*header, *value);
        }
        request = request.header("Authorization", format!("token {access_token}"));

        let response = request.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let sanitized = super::sanitize_api_error(&body);

            if status.as_u16() == 401 || status.as_u16() == 403 {
                let access_token_path = self.token_dir.join("access-token");
                tokio::fs::remove_file(&access_token_path).await.ok();
            }

            anyhow::bail!(
                "Failed to get Copilot API key ({status}): {sanitized}. \
                 Ensure your GitHub account has an active Copilot subscription."
            );
        }

        let info: ApiKeyInfo = response.json().await?;
        Ok(info)
    }

    async fn load_api_key_from_disk(&self) -> Option<ApiKeyInfo> {
        let path = self.token_dir.join("api-key.json");
        let data = tokio::fs::read_to_string(&path).await.ok()?;
        serde_json::from_str(&data).ok()
    }

    async fn save_api_key_to_disk(&self, info: &ApiKeyInfo) {
        let path = self.token_dir.join("api-key.json");
        if let Ok(json) = serde_json::to_string_pretty(info) {
            write_file_secure(&path, &json).await;
        }
    }
}

#[cfg(windows)]
fn restrict_dir_to_current_user(dir: &Path) {
    let Ok(user) = std::env::var("USERNAME") else {
        return;
    };
    if user.trim().is_empty() {
        return;
    }
    let mut cmd = crate::util::hidden_sync_command("icacls");
    cmd.arg(dir)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{user}:(OI)(CI)F"))
        .arg("/grant:r")
        .arg("*S-1-5-18:(OI)(CI)F");
    match cmd.output() {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            warn!(
                "icacls could not restrict Copilot token directory {:?}: {}",
                dir,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Err(err) => {
            warn!(
                "failed to run icacls to restrict Copilot token directory {:?}: {err}",
                dir
            );
        }
    }
}

async fn write_file_secure(path: &Path, content: &str) {
    let path = path.to_path_buf();
    let content = content.to_string();

    let result = tokio::task::spawn_blocking(move || {
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)?;
            file.write_all(content.as_bytes())?;

            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            Ok::<(), std::io::Error>(())
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&path, &content)?;
            Ok::<(), std::io::Error>(())
        }
    })
    .await;

    match result {
        Ok(Ok(())) => {}
        Ok(Err(err)) => warn!("Failed to write secure file: {err}"),
        Err(err) => warn!("Failed to spawn blocking write: {err}"),
    }
}

#[async_trait]
impl Provider for CopilotProvider {
    fn capabilities(&self) -> crate::providers::traits::ProviderCapabilities {
        crate::providers::traits::ProviderCapabilities {
            native_tool_calling: true,
            vision: true,
            prompt_caching: false,
            responses_api: false,
        }
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let mut messages = Vec::new();
        if let Some(system) = system_prompt {
            messages.push(ApiMessage {
                role: "system".to_string(),
                content: Some(ApiContent::Text(system.to_string())),
                tool_call_id: None,
                tool_calls: None,
            });
        }
        messages.push(ApiMessage {
            role: "user".to_string(),
            content: Self::to_api_content("user", message),
            tool_call_id: None,
            tool_calls: None,
        });

        let response = self
            .send_chat_request(messages, None, model, temperature)
            .await?;
        Ok(response.text.unwrap_or_default())
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let sanitized = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            messages.to_vec(),
            model,
            0,
            None,
        );
        let response = self
            .send_chat_request(Self::convert_messages(&sanitized), None, model, temperature)
            .await?;
        Ok(response.text.unwrap_or_default())
    }

    async fn chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let sanitized_messages = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            request.messages.to_vec(),
            model,
            0,
            None,
        );
        self.send_chat_request(
            Self::convert_messages(&sanitized_messages),
            request.tools,
            model,
            temperature,
        )
        .await
    }

    fn supports_native_tools(&self) -> bool {
        true
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_streaming_tool_events(&self) -> bool {
        true
    }

    fn stream_chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
        options: crate::providers::traits::StreamOptions,
    ) -> futures_util::stream::BoxStream<
        'static,
        crate::providers::traits::StreamResult<crate::providers::traits::StreamEvent>,
    > {
        use crate::providers::traits::{StreamError, StreamEvent};
        use futures_util::StreamExt;
        use futures_util::stream;

        if !options.enabled {
            return stream::once(async { Ok(StreamEvent::Final) }).boxed();
        }

        let sanitized_messages = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            request.messages.to_vec(),
            model,
            0,
            None,
        );
        let api_messages = Self::convert_messages(&sanitized_messages);
        let tools_owned: Option<Vec<ToolSpec>> = request.tools.map(<[ToolSpec]>::to_vec);
        let model_owned = model.to_string();
        let provider = self.clone();
        let count_tokens = options.count_tokens;

        let (tx, rx) = tokio::sync::mpsc::channel::<
            crate::providers::traits::StreamResult<StreamEvent>,
        >(100);

        let idempotency_key = crate::providers::core::idempotency::current_idempotency_key();
        let cancel_token = crate::providers::current_session_cancel_token();
        crate::runtime::spawn_supervised("providers.copilot.stream", async move {
            let (token, endpoint) = match provider.get_api_key().await {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx
                        .send(Err(StreamError::Provider(format!(
                            "GitHub Copilot auth failed: {e}"
                        ))))
                        .await;
                    return;
                }
            };
            let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));
            let native_tools = Self::convert_tools(tools_owned.as_deref());
            let body = ApiChatRequest {
                model: model_owned,
                messages: api_messages,
                temperature,
                tool_choice: native_tools.as_ref().map(|_| "auto".to_string()),
                tools: native_tools,
                stream: Some(true),
            };
            let mut req = crate::providers::core::idempotency::apply_idempotency_header_value(
                provider.stream_http_client().post(&url),
                idempotency_key,
            )
            .header("Authorization", format!("Bearer {token}"))
            .json(&body);
            for (header, value) in &Self::COPILOT_HEADERS {
                req = req.header(*header, *value);
            }
            let response = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx
                        .send(Err(StreamError::Provider(format!(
                            "GitHub Copilot stream request failed: {e}"
                        ))))
                        .await;
                    return;
                }
            };
            if !response.status().is_success() {
                let (status, text) =
                    crate::providers::stream_error_body_with_retry_after(response).await;
                let sanitized = crate::providers::sanitize_api_error(&text);
                let _ = tx
                    .send(Err(crate::providers::stream_upstream_error(
                        "GitHub Copilot",
                        status,
                        &text,
                        &sanitized,
                    )))
                    .await;
                return;
            }

            let mut event_stream =
                crate::providers::core::openai_sse::sse_bytes_to_events(
                    response,
                    count_tokens,
                    crate::providers::sanitize::ProviderKind::OpenAi,
                );
            loop {
                let event = tokio::select! {
                    _ = crate::providers::stream_cancelled(&cancel_token) => break,
                    next = event_stream.next() => match next {
                        Some(event) => event,
                        None => break,
                    },
                };
                if tx.send(event).await.is_err() {
                    break;
                }
            }
        });

        stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        })
        .boxed()
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        let _ = self.get_api_key().await?;
        Ok(())
    }
}
