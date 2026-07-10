// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::auth::AuthService;
use crate::providers::traits::{
    ChatMessage, ChatRequest as ProviderChatRequest, ChatResponse as ProviderChatResponse,
    Provider, TokenUsage, ToolCall as ProviderToolCall, ToolsPayload,
};
use crate::tools::ToolSpec;
use async_trait::async_trait;
use base64::Engine;
use directories::UserDirs;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

pub struct GeminiProvider {
    auth: Option<GeminiAuth>,
    oauth_project: Arc<tokio::sync::Mutex<Option<String>>>,
    oauth_cred_paths: Vec<PathBuf>,
    oauth_index: Arc<tokio::sync::Mutex<usize>>,

    auth_service: Option<AuthService>,

    auth_profile_override: Option<String>,

    extra_headers: std::collections::HashMap<String, String>,

    timeout_secs: u64,

    max_output_tokens: u32,
}

const GEMINI_DEFAULT_TIMEOUT_SECS: u64 = 120;

const GEMINI_DEFAULT_MAX_OUTPUT_TOKENS: u32 = 8192;

struct OAuthTokenState {
    access_token: String,
    refresh_token: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,

    expiry_millis: Option<i64>,

    refreshing: bool,
}

enum GeminiAuth {

    ExplicitKey(String),

    EnvGeminiKey(String),

    EnvGoogleKey(String),

    OAuthToken(Arc<tokio::sync::Mutex<OAuthTokenState>>),

    ManagedOAuth,
}

impl GeminiAuth {

    fn is_api_key(&self) -> bool {
        matches!(
            self,
            GeminiAuth::ExplicitKey(_) | GeminiAuth::EnvGeminiKey(_) | GeminiAuth::EnvGoogleKey(_)
        )
    }

    fn is_oauth(&self) -> bool {
        matches!(self, GeminiAuth::OAuthToken(_) | GeminiAuth::ManagedOAuth)
    }

    fn api_key_credential(&self) -> &str {
        match self {
            GeminiAuth::ExplicitKey(s)
            | GeminiAuth::EnvGeminiKey(s)
            | GeminiAuth::EnvGoogleKey(s) => s,
            GeminiAuth::OAuthToken(_) | GeminiAuth::ManagedOAuth => "",
        }
    }
}

#[derive(Debug, Serialize, Clone)]
struct GenerateContentRequest {
    contents: Vec<Content>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<Content>,
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,

    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiToolSpec>>,

    #[serde(rename = "toolConfig", skip_serializing_if = "Option::is_none")]
    tool_config: Option<GeminiToolConfig>,
}

#[derive(Debug, Serialize)]
struct InternalGenerateContentEnvelope {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_prompt_id: Option<String>,
    request: InternalGenerateContentRequest,
}

#[derive(Debug, Serialize)]
struct InternalGenerateContentRequest {
    contents: Vec<Content>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<Content>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiToolSpec>>,
    #[serde(rename = "toolConfig", skip_serializing_if = "Option::is_none")]
    tool_config: Option<GeminiToolConfig>,
}

#[derive(Debug, Serialize, Clone)]
struct Content {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    parts: Vec<Part>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(untagged)]
enum Part {
    Text {
        text: String,
    },
    Inline {
        inline_data: InlineData,
    },

    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: FunctionCallPart,
    },

    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: FunctionResponsePart,
    },
}

impl Part {
    fn text(s: impl Into<String>) -> Self {
        Part::Text { text: s.into() }
    }
}

#[derive(Debug, Serialize, Clone)]
struct InlineData {
    mime_type: String,
    data: String,
}

#[derive(Debug, Serialize, Clone)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Clone)]
struct GeminiToolSpec {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Serialize, Clone)]
struct GeminiToolConfig {
    #[serde(rename = "functionCallingConfig")]
    function_calling_config: FunctionCallingConfig,
}

#[derive(Debug, Serialize, Clone)]
struct FunctionCallingConfig {

    mode: String,
}

#[derive(Debug, Serialize, Clone)]
struct FunctionCallPart {
    name: String,
    args: serde_json::Value,
}

#[derive(Debug, Serialize, Clone)]
struct FunctionResponsePart {
    name: String,

    response: serde_json::Value,
}

fn build_parts(content: &str) -> Vec<Part> {
    let (text, image_refs) = crate::multimodal::parse_image_markers(content);
    let mut parts = Vec::new();
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        parts.push(Part::text(trimmed));
    }
    for uri in &image_refs {
        if let Some(rest) = uri.strip_prefix("data:") {
            if let Some(semi_pos) = rest.find(';') {
                let mime = &rest[..semi_pos];
                if let Some(b64) = rest[semi_pos + 1..].strip_prefix("base64,") {
                    parts.push(Part::Inline {
                        inline_data: InlineData {
                            mime_type: mime.to_string(),
                            data: b64.to_string(),
                        },
                    });
                }
            }
        }
    }
    if parts.is_empty() {
        parts.push(Part::text(content));
    }
    parts
}

#[derive(Debug, Serialize, Clone)]
struct GenerationConfig {
    temperature: f64,
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,

    #[serde(rename = "responseMimeType", skip_serializing_if = "Option::is_none")]
    response_mime_type: Option<String>,

    #[serde(rename = "responseSchema", skip_serializing_if = "Option::is_none")]
    response_schema: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct GenerateContentResponse {
    candidates: Option<Vec<Candidate>>,
    error: Option<ApiError>,
    #[serde(default)]
    response: Option<Box<GenerateContentResponse>>,
    #[serde(default, rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Debug, Deserialize)]
struct GeminiUsageMetadata {
    #[serde(default, rename = "promptTokenCount")]
    prompt_token_count: Option<u64>,
    #[serde(default, rename = "candidatesTokenCount")]
    candidates_token_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    #[serde(default)]
    content: Option<CandidateContent>,
}

#[derive(Debug, Deserialize)]
struct CandidateContent {
    parts: Vec<ResponsePart>,
}

#[derive(Debug, Deserialize)]
struct ResponsePart {
    #[serde(default)]
    text: Option<String>,

    #[serde(default)]
    thought: bool,

    #[serde(default, rename = "functionCall")]
    function_call: Option<ResponseFunctionCall>,
}

#[derive(Debug, Deserialize, Clone)]
struct ResponseFunctionCall {
    name: String,
    #[serde(default)]
    args: Option<serde_json::Value>,
}

#[derive(Debug, Default)]
pub(super) struct GeminiCandidate {
    pub text: Option<String>,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<ProviderToolCall>,
}

impl CandidateContent {

    fn effective_text(self) -> Option<String> {
        self.into_candidate().text
    }

    fn into_candidate(self) -> GeminiCandidate {
        let mut answer_parts: Vec<String> = Vec::new();
        let mut first_thinking: Option<String> = None;
        let mut tool_calls: Vec<ProviderToolCall> = Vec::new();

        for part in self.parts {
            if let Some(function_call) = part.function_call {
                let arguments = function_call
                    .args
                    .map(|v| serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string()))
                    .unwrap_or_else(|| "{}".to_string());
                tool_calls.push(ProviderToolCall {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: function_call.name,
                    arguments,
                });
                continue;
            }
            if let Some(text) = part.text {
                if text.is_empty() {
                    continue;
                }
                if !part.thought {
                    answer_parts.push(text);
                } else if first_thinking.is_none() {
                    first_thinking = Some(text);
                }
            }
        }

        let text = if answer_parts.is_empty() {
            None
        } else {
            Some(answer_parts.join(""))
        };

        GeminiCandidate {
            text,
            reasoning: first_thinking,
            tool_calls,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
}

impl GenerateContentResponse {

    fn into_effective_response(self) -> Self {
        match self {
            Self {
                response: Some(inner),
                ..
            } => *inner,
            other => other,
        }
    }
}

#[derive(Debug, Deserialize)]
struct GeminiCliOAuthCreds {
    access_token: Option<String>,
    #[serde(alias = "idToken")]
    id_token: Option<String>,
    refresh_token: Option<String>,
    #[serde(alias = "clientId")]
    client_id: Option<String>,
    #[serde(alias = "clientSecret")]
    client_secret: Option<String>,

    #[serde(alias = "expiryDate")]
    expiry_date: Option<i64>,

    expiry: Option<String>,
}

const GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

const CLOUDCODE_PA_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com/v1internal";

const LOAD_CODE_ASSIST_ENDPOINT: &str =
    "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";

const PUBLIC_API_ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta";

struct RefreshedToken {
    access_token: String,

    expiry_millis: Option<i64>,
}

fn refresh_gemini_cli_token(
    refresh_token: &str,
    client_id: Option<&str>,
    client_secret: Option<&str>,
) -> anyhow::Result<RefreshedToken> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new());

    let form = build_oauth_refresh_form(refresh_token, client_id, client_secret);

    let response = client
        .post(GOOGLE_TOKEN_ENDPOINT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .map_err(|error| anyhow::anyhow!("Gemini CLI OAuth refresh request failed: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .unwrap_or_else(|_| "<failed to read response body>".to_string());

    if !status.is_success() {
        anyhow::bail!(
            "Gemini CLI OAuth refresh failed (HTTP {status}): {}",
            crate::providers::sanitize_api_error(&body)
        );
    }

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: Option<String>,
        expires_in: Option<i64>,
    }

    let parsed: TokenResponse = serde_json::from_str(&body)
        .map_err(|_| anyhow::anyhow!("Gemini CLI OAuth refresh response is not valid JSON"))?;

    let access_token = parsed
        .access_token
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Gemini CLI OAuth refresh response missing access_token"))?;

    let expiry_millis = parsed.expires_in.and_then(|secs| {
        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_millis()).ok())?;
        now_millis.checked_add(secs.checked_mul(1000)?)
    });

    Ok(RefreshedToken {
        access_token,
        expiry_millis,
    })
}

fn build_oauth_refresh_form(
    refresh_token: &str,
    client_id: Option<&str>,
    client_secret: Option<&str>,
) -> Vec<(&'static str, String)> {
    let mut form = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
    ];
    if let Some(id) = client_id.and_then(GeminiProvider::normalize_non_empty) {
        form.push(("client_id", id));
    }
    if let Some(secret) = client_secret.and_then(GeminiProvider::normalize_non_empty) {
        form.push(("client_secret", secret));
    }
    form
}

fn extract_client_id_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .ok()?;

    #[derive(Deserialize)]
    struct IdTokenClaims {
        aud: Option<String>,
        azp: Option<String>,
    }

    let claims: IdTokenClaims = serde_json::from_slice(&decoded).ok()?;
    claims
        .aud
        .as_deref()
        .and_then(GeminiProvider::normalize_non_empty)
        .or_else(|| {
            claims
                .azp
                .as_deref()
                .and_then(GeminiProvider::normalize_non_empty)
        })
}

async fn refresh_gemini_cli_token_async(
    refresh_token: &str,
    client_id: Option<&str>,
    client_secret: Option<&str>,
) -> anyhow::Result<RefreshedToken> {
    let refresh_token = refresh_token.to_string();
    let client_id = client_id.map(str::to_string);
    let client_secret = client_secret.map(str::to_string);
    tokio::task::spawn_blocking(move || {
        refresh_gemini_cli_token(
            &refresh_token,
            client_id.as_deref(),
            client_secret.as_deref(),
        )
    })
    .await
    .map_err(|e| anyhow::anyhow!("Token refresh task panicked: {e}"))?
}

impl GeminiProvider {

    pub fn new(api_key: Option<&str>) -> Self {
        let oauth_cred_paths = Self::discover_oauth_cred_paths();
        let resolved_auth = api_key
            .and_then(Self::normalize_non_empty)
            .map(GeminiAuth::ExplicitKey)
            .or_else(|| Self::load_non_empty_env("GEMINI_API_KEY").map(GeminiAuth::EnvGeminiKey))
            .or_else(|| Self::load_non_empty_env("GOOGLE_API_KEY").map(GeminiAuth::EnvGoogleKey))
            .or_else(|| {
                Self::try_load_gemini_cli_token(oauth_cred_paths.first())
                    .map(|state| GeminiAuth::OAuthToken(Arc::new(tokio::sync::Mutex::new(state))))
            });

        Self {
            auth: resolved_auth,
            oauth_project: Arc::new(tokio::sync::Mutex::new(None)),
            oauth_cred_paths,
            oauth_index: Arc::new(tokio::sync::Mutex::new(0)),
            auth_service: None,
            auth_profile_override: None,
            extra_headers: std::collections::HashMap::new(),
            timeout_secs: GEMINI_DEFAULT_TIMEOUT_SECS,
            max_output_tokens: GEMINI_DEFAULT_MAX_OUTPUT_TOKENS,
        }
    }

    pub fn with_extra_headers(
        mut self,
        headers: std::collections::HashMap<String, String>,
    ) -> Self {
        self.extra_headers = headers;
        self
    }

    #[must_use]
    pub fn with_timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs.max(1);
        self
    }

    #[must_use]
    pub fn with_max_output_tokens(mut self, max_output_tokens: u32) -> Self {
        self.max_output_tokens = max_output_tokens.max(1);
        self
    }

    pub fn new_with_auth(
        api_key: Option<&str>,
        auth_service: AuthService,
        profile_override: Option<String>,
    ) -> Self {
        let oauth_cred_paths = Self::discover_oauth_cred_paths();

        let explicit_auth = api_key
            .and_then(Self::normalize_non_empty)
            .map(GeminiAuth::ExplicitKey)
            .or_else(|| Self::load_non_empty_env("GEMINI_API_KEY").map(GeminiAuth::EnvGeminiKey))
            .or_else(|| Self::load_non_empty_env("GOOGLE_API_KEY").map(GeminiAuth::EnvGoogleKey));

        let (auth, attach_service) = match explicit_auth {
            Some(a) => (Some(a), false),
            None => {
                let cli_auth = Self::try_load_gemini_cli_token(oauth_cred_paths.first())
                    .map(|state| GeminiAuth::OAuthToken(Arc::new(tokio::sync::Mutex::new(state))));
                match cli_auth {
                    Some(a) => (Some(a), false),
                    None => (Some(GeminiAuth::ManagedOAuth), true),
                }
            }
        };

        Self {
            auth,
            oauth_project: Arc::new(tokio::sync::Mutex::new(None)),
            oauth_cred_paths,
            oauth_index: Arc::new(tokio::sync::Mutex::new(0)),
            auth_service: if attach_service {
                Some(auth_service)
            } else {
                None
            },
            auth_profile_override: profile_override,
            extra_headers: std::collections::HashMap::new(),
            timeout_secs: GEMINI_DEFAULT_TIMEOUT_SECS,
            max_output_tokens: GEMINI_DEFAULT_MAX_OUTPUT_TOKENS,
        }
    }

    fn normalize_non_empty(value: &str) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn load_non_empty_env(name: &str) -> Option<String> {
        std::env::var(name)
            .ok()
            .and_then(|value| Self::normalize_non_empty(&value))
    }

    fn load_gemini_cli_creds(creds_path: &PathBuf) -> Option<GeminiCliOAuthCreds> {
        if !creds_path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(creds_path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn discover_oauth_cred_paths() -> Vec<PathBuf> {
        let home = match UserDirs::new() {
            Some(u) => u.home_dir().to_path_buf(),
            None => return Vec::new(),
        };

        let mut paths = Vec::new();

        let primary = home.join(".gemini").join("oauth_creds.json");
        if primary.exists() {
            paths.push(primary);
        }

        if let Ok(entries) = std::fs::read_dir(&home) {
            let mut extras: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with(".gemini-") && name.ends_with("-home") {
                        let path = e.path().join(".gemini").join("oauth_creds.json");
                        if path.exists() {
                            return Some(path);
                        }
                    }
                    None
                })
                .collect();
            extras.sort();
            paths.extend(extras);
        }

        paths
    }

    fn try_load_gemini_cli_token(path: Option<&PathBuf>) -> Option<OAuthTokenState> {
        let creds = Self::load_gemini_cli_creds(path?)?;

        let expiry_millis = creds.expiry_date.or_else(|| {
            creds.expiry.as_deref().and_then(|expiry| {
                chrono::DateTime::parse_from_rfc3339(expiry)
                    .ok()
                    .map(|dt| dt.timestamp_millis())
            })
        });

        let access_token = creds
            .access_token
            .and_then(|token| Self::normalize_non_empty(&token))?;

        let id_token_client_id = creds
            .id_token
            .as_deref()
            .and_then(extract_client_id_from_id_token);

        let client_id = Self::load_non_empty_env("GEMINI_OAUTH_CLIENT_ID")
            .or_else(|| {
                creds
                    .client_id
                    .as_deref()
                    .and_then(Self::normalize_non_empty)
            })
            .or(id_token_client_id);
        let client_secret = Self::load_non_empty_env("GEMINI_OAUTH_CLIENT_SECRET").or_else(|| {
            creds
                .client_secret
                .as_deref()
                .and_then(Self::normalize_non_empty)
        });

        Some(OAuthTokenState {
            access_token,
            refresh_token: creds.refresh_token,
            client_id,
            client_secret,
            expiry_millis,
            refreshing: false,
        })
    }

    pub fn has_cli_credentials() -> bool {
        Self::discover_oauth_cred_paths().iter().any(|path| {
            Self::load_gemini_cli_creds(path)
                .and_then(|creds| {
                    creds
                        .access_token
                        .as_deref()
                        .and_then(Self::normalize_non_empty)
                })
                .is_some()
        })
    }

    pub fn has_any_auth() -> bool {
        Self::load_non_empty_env("GEMINI_API_KEY").is_some()
            || Self::load_non_empty_env("GOOGLE_API_KEY").is_some()
            || Self::has_cli_credentials()
    }

    pub fn auth_source(&self) -> &'static str {
        match self.auth.as_ref() {
            Some(GeminiAuth::ExplicitKey(_)) => "config",
            Some(GeminiAuth::EnvGeminiKey(_)) => "GEMINI_API_KEY env var",
            Some(GeminiAuth::EnvGoogleKey(_)) => "GOOGLE_API_KEY env var",
            Some(GeminiAuth::OAuthToken(_)) => "Gemini CLI OAuth",
            Some(GeminiAuth::ManagedOAuth) => "auth-profiles",
            None => "none",
        }
    }

    fn oauth_now_millis() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(i64::MAX)
    }

    async fn get_valid_oauth_token(
        state: &Arc<tokio::sync::Mutex<OAuthTokenState>>,
    ) -> anyhow::Result<String> {
        const WAIT_SLICE_MS: u64 = 50;
        const TAKEOVER_AFTER_MS: u64 = 30_000;
        const MAX_TAKEOVERS: u32 = 2;

        let mut waited_ms: u64 = 0;
        let mut takeovers: u32 = 0;

        loop {
            let (refresh_token, client_id, client_secret) = {
                let mut guard = state.lock().await;

                let needs_refresh = guard
                    .expiry_millis
                    .map_or(true, |exp| exp <= Self::oauth_now_millis().saturating_add(60_000));
                if !needs_refresh {
                    return Ok(guard.access_token.clone());
                }

                if guard.refreshing {
                    if waited_ms >= TAKEOVER_AFTER_MS {
                        if takeovers >= MAX_TAKEOVERS {
                            drop(guard);
                            anyhow::bail!(
                                "Gemini CLI OAuth token refresh did not complete within timeout (refresher may have panicked or been cancelled); aborting to avoid spinning"
                            );
                        }
                        guard.refreshing = false;
                        waited_ms = 0;
                        takeovers = takeovers.saturating_add(1);
                        drop(guard);
                        tracing::warn!(
                            "Gemini OAuth: refresh wait exceeded {TAKEOVER_AFTER_MS}ms; reclaiming refresh slot (possible refresher panic/cancel)"
                        );
                        continue;
                    }
                    drop(guard);
                    tokio::time::sleep(std::time::Duration::from_millis(WAIT_SLICE_MS)).await;
                    waited_ms = waited_ms.saturating_add(WAIT_SLICE_MS);
                    continue;
                }

                let Some(refresh_token) = guard.refresh_token.clone() else {
                    anyhow::bail!(
                        "Gemini CLI OAuth token expired and no refresh_token available  - re-run `gemini` to authenticate"
                    );
                };

                guard.refreshing = true;
                (
                    refresh_token,
                    guard.client_id.clone(),
                    guard.client_secret.clone(),
                )
            };

            let result = refresh_gemini_cli_token_async(
                &refresh_token,
                client_id.as_deref(),
                client_secret.as_deref(),
            )
            .await;

            let mut guard = state.lock().await;
            guard.refreshing = false;
            let refreshed = result?;
            tracing::info!("Gemini CLI OAuth token refreshed successfully (runtime)");
            guard.access_token = refreshed.access_token.clone();
            guard.expiry_millis = refreshed.expiry_millis;
            return Ok(refreshed.access_token);
        }
    }

    async fn rotate_oauth_credential(
        &self,
        state: &Arc<tokio::sync::Mutex<OAuthTokenState>>,
    ) -> bool {
        if self.oauth_cred_paths.len() <= 1 {
            return false;
        }

        let mut idx = self.oauth_index.lock().await;
        let start = *idx;

        loop {
            let next = (*idx + 1) % self.oauth_cred_paths.len();
            *idx = next;

            if next == start {
                return false;
            }

            let next_path = self.oauth_cred_paths.get(next).cloned();
            let loaded = tokio::task::spawn_blocking(move || {
                Self::try_load_gemini_cli_token(next_path.as_ref())
            })
            .await
            .ok()
            .flatten();
            if let Some(next_state) = loaded {
                {
                    let mut guard = state.lock().await;
                    *guard = next_state;
                }
                {
                    let mut cached_project = self.oauth_project.lock().await;
                    *cached_project = None;
                }
                tracing::warn!(
                    "Gemini OAuth: rotated credential to {}",
                    self.oauth_cred_paths[next].display()
                );
                return true;
            }
        }
    }

    fn format_model_name(model: &str) -> String {
        if model.starts_with("models/") {
            model.to_string()
        } else {
            format!("models/{model}")
        }
    }

    fn format_internal_model_name(model: &str) -> String {
        model.strip_prefix("models/").unwrap_or(model).to_string()
    }

    fn build_generate_content_url(model: &str, auth: &GeminiAuth) -> String {
        match auth {
            GeminiAuth::OAuthToken(_) | GeminiAuth::ManagedOAuth => {

                format!("{CLOUDCODE_PA_ENDPOINT}:generateContent")
            }
            _ => {
                let model_name = Self::format_model_name(model);
                format!("{PUBLIC_API_ENDPOINT}/{model_name}:generateContent")
            }
        }
    }

    fn http_client(&self) -> Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts_and_headers(
                "provider.gemini",
                self.timeout_secs,
                10,
                &self.extra_headers,
            )
    }

    async fn resolve_oauth_project(&self, token: &str) -> anyhow::Result<String> {
        let project_seed = Self::load_non_empty_env("GOOGLE_CLOUD_PROJECT")
            .or_else(|| Self::load_non_empty_env("GOOGLE_CLOUD_PROJECT_ID"));
        let project_seed_for_request = project_seed.clone();
        let duet_project_for_request = project_seed.clone();

        {
            let cached = self.oauth_project.lock().await;
            if let Some(ref project) = *cached {
                return Ok(project.clone());
            }
        }

        let client = self.http_client();
        let response = client
            .post(LOAD_CODE_ASSIST_ENDPOINT)
            .bearer_auth(token)
            .json(&serde_json::json!({
                "cloudaicompanionProject": project_seed_for_request,
                "metadata": {
                    "ideType": "GEMINI_CLI",
                    "platform": "PLATFORM_UNSPECIFIED",
                    "pluginType": "GEMINI",
                    "duetProject": duet_project_for_request,
                }
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if let Some(seed) = project_seed {
                tracing::warn!(
                    "loadCodeAssist failed (HTTP {status}); using GOOGLE_CLOUD_PROJECT fallback"
                );
                return Ok(seed);
            }
            anyhow::bail!(
                "loadCodeAssist failed (HTTP {status}): {}",
                crate::providers::sanitize_api_error(&body)
            );
        }

        #[derive(Deserialize)]
        struct LoadCodeAssistResponse {
            #[serde(rename = "cloudaicompanionProject")]
            cloudaicompanion_project: Option<String>,
        }

        let result: LoadCodeAssistResponse = response.json().await?;
        let project = result
            .cloudaicompanion_project
            .filter(|p| !p.trim().is_empty())
            .or(project_seed)
            .ok_or_else(|| anyhow::anyhow!("loadCodeAssist response missing project context"))?;

        {
            let mut cached = self.oauth_project.lock().await;
            *cached = Some(project.clone());
        }

        Ok(project)
    }

    fn build_generate_content_request(
        &self,
        auth: &GeminiAuth,
        url: &str,
        request: &GenerateContentRequest,
        model: &str,
        include_generation_config: bool,
        project: Option<&str>,
        oauth_token: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let req = self.http_client().post(url).json(request);
        match auth {
            GeminiAuth::OAuthToken(_) | GeminiAuth::ManagedOAuth => {
                let token = oauth_token.unwrap_or_default();

                let internal_request = InternalGenerateContentEnvelope {
                    model: Self::format_internal_model_name(model),
                    project: project.map(|value| value.to_string()),
                    user_prompt_id: Some(uuid::Uuid::new_v4().to_string()),
                    request: InternalGenerateContentRequest {
                        contents: request.contents.clone(),
                        system_instruction: request.system_instruction.clone(),
                        generation_config: if include_generation_config {
                            Some(request.generation_config.clone())
                        } else {
                            None
                        },
                        tools: request.tools.clone(),
                        tool_config: request.tool_config.clone(),
                    },
                };
                self.http_client()
                    .post(url)
                    .json(&internal_request)
                    .bearer_auth(token)
            }
            _ if auth.is_api_key() => {
                req.header("x-goog-api-key", auth.api_key_credential())
            }
            _ => req,
        }
    }

    fn should_retry_oauth_without_generation_config(
        status: reqwest::StatusCode,
        error_text: &str,
    ) -> bool {
        if status != reqwest::StatusCode::BAD_REQUEST {
            return false;
        }

        error_text.contains("Unknown name \"generationConfig\"")
            || error_text.contains("Unknown name 'generationConfig'")
            || error_text.contains(r#"Unknown name \"generationConfig\""#)
    }

    fn should_rotate_oauth_on_error(status: reqwest::StatusCode, error_text: &str) -> bool {
        status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
            || status.is_server_error()
            || error_text.contains("RESOURCE_EXHAUSTED")
    }
}

impl GeminiProvider {

    async fn send_generate_content(
        &self,
        contents: Vec<Content>,
        system_instruction: Option<Content>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<(String, Option<TokenUsage>)> {
        self.send_generate_content_with_config(
            contents,
            system_instruction,
            model,
            GenerationConfig {
                temperature,
                max_output_tokens: self.max_output_tokens,
                response_mime_type: None,
                response_schema: None,
            },
        )
        .await
    }

    async fn send_generate_content_with_config(
        &self,
        contents: Vec<Content>,
        system_instruction: Option<Content>,
        model: &str,
        generation_config: GenerationConfig,
    ) -> anyhow::Result<(String, Option<TokenUsage>)> {
        let request = GenerateContentRequest {
            contents,
            system_instruction,
            generation_config,
            tools: None,
            tool_config: None,
        };
        let result = self.send_raw_request(request, model).await?;
        let usage = result.usage_metadata.as_ref().map(|u| TokenUsage {
            input_tokens: u.prompt_token_count,
            output_tokens: u.candidates_token_count,
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
        });
        let text = result
            .candidates
            .and_then(|c| c.into_iter().next())
            .and_then(|c| c.content)
            .and_then(|c| c.effective_text())
            .ok_or_else(|| anyhow::anyhow!("No response from Gemini"))?;
        Ok((text, usage))
    }

    async fn send_raw_request(
        &self,
        request: GenerateContentRequest,
        model: &str,
    ) -> anyhow::Result<GenerateContentResponse> {
        let auth = self.auth.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Gemini API key not found. Options:\n\
                 1. Set GEMINI_API_KEY env var\n\
                 2. Run `gemini` CLI to authenticate (tokens will be reused)\n\
                 3. Run `sen auth login --provider gemini`\n\
                 4. Get an API key from https://aistudio.google.com/app/apikey\n\
                 5. Run `sen onboard` to configure"
            )
        })?;

        let oauth_state = match auth {
            GeminiAuth::OAuthToken(state) => Some(state.clone()),
            _ => None,
        };

        let (mut oauth_token, mut project) = match auth {
            GeminiAuth::OAuthToken(state) => {
                let token = Self::get_valid_oauth_token(state).await?;
                let proj = self.resolve_oauth_project(&token).await?;
                (Some(token), Some(proj))
            }
            GeminiAuth::ManagedOAuth => {
                let auth_service = self
                    .auth_service
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("ManagedOAuth requires auth_service"))?;
                let token = auth_service
                    .get_valid_gemini_access_token(self.auth_profile_override.as_deref())
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Gemini auth profile not found. Run `sen auth login --provider gemini`."
                        )
                    })?;
                let proj = self.resolve_oauth_project(&token).await?;
                (Some(token), Some(proj))
            }
            _ => (None, None),
        };

        let url = Self::build_generate_content_url(model, auth);

        let mut response = self
            .build_generate_content_request(
                auth,
                &url,
                &request,
                model,
                true,
                project.as_deref(),
                oauth_token.as_deref(),
            )
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            if auth.is_oauth() && Self::should_rotate_oauth_on_error(status, &error_text) {

                let can_retry = match auth {
                    GeminiAuth::OAuthToken(_) => {
                        if let Some(state) = oauth_state.as_ref() {
                            self.rotate_oauth_credential(state).await
                        } else {
                            false
                        }
                    }
                    GeminiAuth::ManagedOAuth => true,
                    _ => false,
                };

                if can_retry {

                    let (new_token, new_project) = match auth {
                        GeminiAuth::OAuthToken(state) => {
                            let token = Self::get_valid_oauth_token(state).await?;
                            let proj = self.resolve_oauth_project(&token).await?;
                            (token, proj)
                        }
                        GeminiAuth::ManagedOAuth => {
                            let auth_service = self.auth_service.as_ref().ok_or_else(|| {
                                anyhow::anyhow!("Gemini ManagedOAuth requires auth_service but none was configured")
                            })?;
                            let token = auth_service
                                .get_valid_gemini_access_token(
                                    self.auth_profile_override.as_deref(),
                                )
                                .await?
                                .ok_or_else(|| anyhow::anyhow!("Gemini auth profile not found"))?;
                            let proj = self.resolve_oauth_project(&token).await?;
                            (token, proj)
                        }
                        _ => {
                            return Err(anyhow::anyhow!(
                                "Gemini OAuth refresh path reached unexpected auth variant (expected OAuthToken or ManagedOAuth)"
                            ));
                        }
                    };
                    oauth_token = Some(new_token);
                    project = Some(new_project);
                    response = self
                        .build_generate_content_request(
                            auth,
                            &url,
                            &request,
                            model,
                            true,
                            project.as_deref(),
                            oauth_token.as_deref(),
                        )
                        .send()
                        .await?;
                } else {
                    anyhow::bail!(
                        "Gemini API error ({status}): {}",
                        crate::providers::sanitize_api_error(&error_text)
                    );
                }
            } else if auth.is_oauth()
                && Self::should_retry_oauth_without_generation_config(status, &error_text)
            {
                tracing::warn!(
                    "Gemini OAuth internal endpoint rejected generationConfig; retrying without generationConfig"
                );
                response = self
                    .build_generate_content_request(
                        auth,
                        &url,
                        &request,
                        model,
                        false,
                        project.as_deref(),
                        oauth_token.as_deref(),
                    )
                    .send()
                    .await?;
            } else {
                anyhow::bail!(
                    "Gemini API error ({status}): {}",
                    crate::providers::sanitize_api_error(&error_text)
                );
            }
        }

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            if auth.is_oauth()
                && Self::should_retry_oauth_without_generation_config(status, &error_text)
            {
                tracing::warn!(
                    "Gemini OAuth internal endpoint rejected generationConfig; retrying without generationConfig"
                );
                response = self
                    .build_generate_content_request(
                        auth,
                        &url,
                        &request,
                        model,
                        false,
                        project.as_deref(),
                        oauth_token.as_deref(),
                    )
                    .send()
                    .await?;
            } else {
                anyhow::bail!(
                    "Gemini API error ({status}): {}",
                    crate::providers::sanitize_api_error(&error_text)
                );
            }
        }

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "Gemini API error ({status}): {}",
                crate::providers::sanitize_api_error(&error_text)
            );
        }

        let result: GenerateContentResponse = response.json().await?;
        if let Some(err) = &result.error {
            anyhow::bail!(
                "Gemini API error: {}",
                crate::providers::sanitize_api_error(&err.message)
            );
        }
        let result = result.into_effective_response();
        if let Some(err) = result.error {
            anyhow::bail!(
                "Gemini API error: {}",
                crate::providers::sanitize_api_error(&err.message)
            );
        }

        Ok(result)
    }

    async fn send_with_tools_internal(
        &self,
        request: GenerateContentRequest,
        model: &str,
    ) -> anyhow::Result<ProviderChatResponse> {
        let result = self.send_raw_request(request, model).await?;

        let usage = result.usage_metadata.as_ref().map(|u| TokenUsage {
            input_tokens: u.prompt_token_count,
            output_tokens: u.candidates_token_count,
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
        });

        let candidate = result
            .candidates
            .and_then(|c| c.into_iter().next())
            .and_then(|c| c.content)
            .map(|c| c.into_candidate())
            .unwrap_or_default();

        Ok(ProviderChatResponse {
            text: candidate.text,
            tool_calls: candidate.tool_calls,
            usage,
            reasoning_content: candidate.reasoning,
        })
    }

    fn convert_messages_native(
        messages: &[ChatMessage],
    ) -> (Vec<&str>, Vec<Content>) {
        let mut system_parts: Vec<&str> = Vec::new();
        let mut contents: Vec<Content> = Vec::new();

        let mut tool_id_to_name: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        for msg in messages {
            match msg.role.as_str() {
                "system" => {
                    system_parts.push(&msg.content);
                }
                "user" => {
                    contents.push(Content {
                        role: Some("user".to_string()),
                        parts: build_parts(&msg.content),
                    });
                }
                "assistant" => {
                    let mut parts = Vec::new();
                    let mut consumed_tool_calls = false;

                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&msg.content) {
                        if let Some(tool_calls_value) = value.get("tool_calls") {
                            if let Ok(tool_calls) =
                                serde_json::from_value::<Vec<ProviderToolCall>>(
                                    tool_calls_value.clone(),
                                )
                            {
                                if !tool_calls.is_empty() {
                                    if let Some(text_field) =
                                        value.get("content").and_then(|c| c.as_str())
                                    {
                                        if !text_field.is_empty() {
                                            parts.push(Part::text(text_field));
                                        }
                                    }
                                    for call in tool_calls {
                                        let args: serde_json::Value =
                                            serde_json::from_str(&call.arguments)
                                                .unwrap_or(serde_json::Value::Object(
                                                    Default::default(),
                                                ));
                                        tool_id_to_name
                                            .insert(call.id.clone(), call.name.clone());
                                        parts.push(Part::FunctionCall {
                                            function_call: FunctionCallPart {
                                                name: call.name,
                                                args,
                                            },
                                        });
                                    }
                                    consumed_tool_calls = true;
                                }
                            }
                        }
                    }

                    if !consumed_tool_calls {
                        parts.push(Part::text(&msg.content));
                    }

                    contents.push(Content {
                        role: Some("model".to_string()),
                        parts,
                    });
                }
                "tool" => {

                    let (fn_name, response_body) =
                        parse_tool_message(&msg.content, &tool_id_to_name);
                    contents.push(Content {
                        role: Some("user".to_string()),
                        parts: vec![Part::FunctionResponse {
                            function_response: FunctionResponsePart {
                                name: fn_name,
                                response: response_body,
                            },
                        }],
                    });
                }
                _ => {}
            }
        }

        (system_parts, contents)
    }

    fn build_tools(tools: &[ToolSpec]) -> Option<Vec<GeminiToolSpec>> {
        if tools.is_empty() {
            return None;
        }
        let deduped = crate::tools::dedupe_tool_specs(tools);
        let declarations: Vec<GeminiFunctionDeclaration> = deduped
            .iter()
            .map(|t| GeminiFunctionDeclaration {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: if t.parameters.is_null()
                    || t.parameters
                        .as_object()
                        .is_some_and(|obj| obj.is_empty())
                {
                    None
                } else {
                    Some(t.parameters.clone())
                },
            })
            .collect();
        if declarations.is_empty() {
            None
        } else {
            Some(vec![GeminiToolSpec {
                function_declarations: declarations,
            }])
        }
    }

    fn build_tools_from_json(tools: &[serde_json::Value]) -> Option<Vec<GeminiToolSpec>> {
        if tools.is_empty() {
            return None;
        }
        let mut seen: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(tools.len());
        let declarations: Vec<GeminiFunctionDeclaration> = tools
            .iter()
            .filter_map(|entry| {
                let func = entry.get("function")?;
                let name = func.get("name")?.as_str()?.to_string();
                if !seen.insert(name.clone()) {
                    return None;
                }
                let description = func
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                let parameters = func.get("parameters").and_then(|p| {
                    if p.is_null() || p.as_object().is_some_and(|o| o.is_empty()) {
                        None
                    } else {
                        Some(p.clone())
                    }
                });
                Some(GeminiFunctionDeclaration {
                    name,
                    description,
                    parameters,
                })
            })
            .collect();
        if declarations.is_empty() {
            None
        } else {
            Some(vec![GeminiToolSpec {
                function_declarations: declarations,
            }])
        }
    }
}

fn parse_tool_message(
    raw: &str,
    tool_id_to_name: &std::collections::HashMap<String, String>,
) -> (String, serde_json::Value) {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) {
        let tool_call_id = value
            .get("tool_call_id")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        let explicit_name = value
            .get("name")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        let name = explicit_name
            .or_else(|| {
                tool_call_id
                    .as_deref()
                    .and_then(|id| tool_id_to_name.get(id).cloned())
            })
            .or(tool_call_id)
            .unwrap_or_else(|| "tool".to_string());
        let response = value
            .get("content")
            .cloned()
            .map(|content| {
                if let Some(s) = content.as_str() {
                    serde_json::json!({ "content": s })
                } else {
                    serde_json::json!({ "content": content })
                }
            })
            .unwrap_or_else(|| serde_json::json!({ "content": raw }));
        (name, response)
    } else {
        (
            "tool".to_string(),
            serde_json::json!({ "content": raw }),
        )
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn capabilities(&self) -> crate::providers::traits::ProviderCapabilities {
        crate::providers::traits::ProviderCapabilities {
            vision: true,

            native_tool_calling: true,
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
        let system_instruction = system_prompt.map(|sys| Content {
            role: None,
            parts: vec![Part::text(sys)],
        });

        let contents = vec![Content {
            role: Some("user".to_string()),
            parts: build_parts(message),
        }];

        let (text, _usage) = self
            .send_generate_content(contents, system_instruction, model, temperature)
            .await?;
        Ok(text)
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
        let (system_parts, contents) = Self::convert_messages_native(&sanitized);

        let system_instruction = if system_parts.is_empty() {
            None
        } else {
            Some(Content {
                role: None,
                parts: vec![Part::text(system_parts.join("\n\n"))],
            })
        };

        let (text, _usage) = self
            .send_generate_content(contents, system_instruction, model, temperature)
            .await?;
        Ok(text)
    }

    async fn chat_structured(
        &self,
        messages: &[ChatMessage],
        schema: &serde_json::Value,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<crate::providers::traits::StructuredResponse> {

        let (system_parts, contents) = Self::convert_messages_native(messages);

        let system_instruction = if system_parts.is_empty() {
            None
        } else {
            Some(Content {
                role: None,
                parts: vec![Part::text(system_parts.join("\n\n"))],
            })
        };

        let generation_config = GenerationConfig {
            temperature,
            max_output_tokens: self.max_output_tokens,
            response_mime_type: Some("application/json".to_string()),
            response_schema: Some(schema.clone()),
        };

        let (text, usage) = self
            .send_generate_content_with_config(
                contents,
                system_instruction,
                model,
                generation_config,
            )
            .await?;

        let parsed = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .or_else(|| crate::providers::traits::parse_first_json_object(&text))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Gemini structured-output reply was not valid JSON: {text}"
                )
            })?;

        Ok(crate::providers::traits::StructuredResponse {
            data: parsed,
            raw_text: text,
            usage,
        })
    }

    fn convert_tools(
        &self,
        tools: &[ToolSpec],
    ) -> ToolsPayload {
        let function_declarations: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                let mut obj = serde_json::Map::new();
                obj.insert(
                    "name".to_string(),
                    serde_json::Value::String(t.name.clone()),
                );
                obj.insert(
                    "description".to_string(),
                    serde_json::Value::String(t.description.clone()),
                );
                let params_empty = t.parameters.is_null()
                    || t.parameters
                        .as_object()
                        .is_some_and(|o| o.is_empty());
                if !params_empty {
                    obj.insert("parameters".to_string(), t.parameters.clone());
                }
                serde_json::Value::Object(obj)
            })
            .collect();
        ToolsPayload::Gemini {
            function_declarations,
        }
    }

    async fn chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let sanitized = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            request.messages.to_vec(),
            model,
            0,
            None,
        );
        let (system_parts, contents) = Self::convert_messages_native(&sanitized);
        let system_instruction = if system_parts.is_empty() {
            None
        } else {
            Some(Content {
                role: None,
                parts: vec![Part::text(system_parts.join("\n\n"))],
            })
        };

        let tools = request
            .tools
            .and_then(|t| Self::build_tools(t));
        let tool_config = tools.as_ref().map(|_| GeminiToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: "AUTO".to_string(),
            },
        });

        let gen_req = GenerateContentRequest {
            contents,
            system_instruction,
            generation_config: GenerationConfig {
                temperature,
                max_output_tokens: self.max_output_tokens,
                response_mime_type: None,
                response_schema: None,
            },
            tools,
            tool_config,
        };

        let response = self.send_with_tools_internal(gen_req, model).await?;
        Ok(ProviderChatResponse {
            text: if response.text.as_deref().map(str::is_empty).unwrap_or(true) {
                None
            } else {
                response.text
            },
            tool_calls: response.tool_calls,
            usage: response.usage,
            reasoning_content: response.reasoning_content,
        })
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let sanitized = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            messages.to_vec(),
            model,
            0,
            None,
        );
        let (system_parts, contents) = Self::convert_messages_native(&sanitized);
        let system_instruction = if system_parts.is_empty() {
            None
        } else {
            Some(Content {
                role: None,
                parts: vec![Part::text(system_parts.join("\n\n"))],
            })
        };

        let tools_decl = Self::build_tools_from_json(tools);
        let tool_config = tools_decl.as_ref().map(|_| GeminiToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: "AUTO".to_string(),
            },
        });

        let gen_req = GenerateContentRequest {
            contents,
            system_instruction,
            generation_config: GenerationConfig {
                temperature,
                max_output_tokens: self.max_output_tokens,
                response_mime_type: None,
                response_schema: None,
            },
            tools: tools_decl,
            tool_config,
        };

        let response = self.send_with_tools_internal(gen_req, model).await?;
        Ok(ProviderChatResponse {
            text: if response.text.as_deref().map(str::is_empty).unwrap_or(true) {
                None
            } else {
                response.text
            },
            tool_calls: response.tool_calls,
            usage: response.usage,
            reasoning_content: response.reasoning_content,
        })
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        if let Some(auth) = self.auth.as_ref() {
            match auth {
                GeminiAuth::ManagedOAuth => {

                    let auth_service = self
                        .auth_service
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("ManagedOAuth requires auth_service"))?;

                    let _token = auth_service
                        .get_valid_gemini_access_token(self.auth_profile_override.as_deref())
                        .await?
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "Gemini auth profile not found or expired. Run: sen auth login --provider gemini"
                            )
                        })?;

                }
                GeminiAuth::OAuthToken(_) => {

                }
                _ => {

                    let url = "https://generativelanguage.googleapis.com/v1beta/models";
                    let mut req = self.http_client().get(url);
                    if auth.is_api_key() {
                        req = req.header("x-goog-api-key", auth.api_key_credential());
                    }
                    req.send().await?.error_for_status()?;
                }
            }
        }
        Ok(())
    }
}

