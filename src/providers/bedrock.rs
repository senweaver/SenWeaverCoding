// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::providers::traits::{
    ChatMessage, ChatRequest as ProviderChatRequest, ChatResponse as ProviderChatResponse,
    Provider, ProviderCapabilities, TokenUsage, ToolCall as ProviderToolCall, ToolsPayload,
};
use crate::tools::ToolSpec;
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const ENDPOINT_PREFIX: &str = "bedrock-runtime";

const SIGNING_SERVICE: &str = "bedrock";
const DEFAULT_REGION: &str = "us-east-1";
const DEFAULT_MAX_TOKENS: u32 = 4096;

enum BedrockAuth {
    SigV4(AwsCredentials),
    BearerToken(String),
}

struct AwsCredentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
    region: String,
}

impl AwsCredentials {

    fn from_env() -> anyhow::Result<Self> {
        let access_key_id = env_required("AWS_ACCESS_KEY_ID")?;
        let secret_access_key = env_required("AWS_SECRET_ACCESS_KEY")?;

        let session_token = env_optional("AWS_SESSION_TOKEN");

        let region = env_optional("AWS_REGION")
            .or_else(|| env_optional("AWS_DEFAULT_REGION"))
            .unwrap_or_else(|| DEFAULT_REGION.to_string());

        Ok(Self {
            access_key_id,
            secret_access_key,
            session_token,
            region,
        })
    }

    async fn from_imds() -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()?;

        let token = client
            .put("http://169.254.169.254/latest/api/token")
            .header("X-aws-ec2-metadata-token-ttl-seconds", "21600")
            .send()
            .await?
            .text()
            .await?;

        let role = client
            .get("http://169.254.169.254/latest/meta-data/iam/security-credentials/")
            .header("X-aws-ec2-metadata-token", &token)
            .send()
            .await?
            .text()
            .await?;
        let role = role.trim().to_string();
        anyhow::ensure!(!role.is_empty(), "No IAM role attached to this instance");

        let creds_url = format!(
            "http://169.254.169.254/latest/meta-data/iam/security-credentials/{}",
            role
        );
        let creds_json: serde_json::Value = client
            .get(&creds_url)
            .header("X-aws-ec2-metadata-token", &token)
            .send()
            .await?
            .json()
            .await?;

        let access_key_id = creds_json["AccessKeyId"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing AccessKeyId in IMDS response"))?
            .to_string();
        let secret_access_key = creds_json["SecretAccessKey"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing SecretAccessKey in IMDS response"))?
            .to_string();
        let session_token = creds_json["Token"].as_str().map(|s| s.to_string());

        let region = match client
            .get("http://169.254.169.254/latest/meta-data/placement/region")
            .header("X-aws-ec2-metadata-token", &token)
            .send()
            .await
        {
            Ok(resp) => resp.text().await.unwrap_or_default(),
            Err(_) => String::new(),
        };
        let region = if region.trim().is_empty() {
            env_optional("AWS_REGION")
                .or_else(|| env_optional("AWS_DEFAULT_REGION"))
                .unwrap_or_else(|| DEFAULT_REGION.to_string())
        } else {
            region.trim().to_string()
        };

        tracing::info!(
            "Loaded AWS credentials from EC2 instance metadata (role: {})",
            role
        );

        Ok(Self {
            access_key_id,
            secret_access_key,
            session_token,
            region,
        })
    }

    async fn resolve() -> anyhow::Result<Self> {
        if let Ok(creds) = Self::from_env() {
            return Ok(creds);
        }
        Self::from_imds().await
    }

    fn host(&self) -> String {
        format!("{ENDPOINT_PREFIX}.{}.amazonaws.com", self.region)
    }
}

fn env_required(name: &str) -> anyhow::Result<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Environment variable {name} is required for Bedrock"))
}

fn env_optional(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn derive_signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

fn build_authorization_header(
    credentials: &AwsCredentials,
    method: &str,
    canonical_uri: &str,
    query_string: &str,
    headers: &[(String, String)],
    payload: &[u8],
    timestamp: &chrono::DateTime<chrono::Utc>,
) -> String {
    let date_stamp = timestamp.format("%Y%m%d").to_string();
    let amz_date = timestamp.format("%Y%m%dT%H%M%SZ").to_string();

    let mut canonical_headers = String::new();
    for (k, v) in headers {
        canonical_headers.push_str(k);
        canonical_headers.push(':');
        canonical_headers.push_str(v);
        canonical_headers.push('\n');
    }

    let signed_headers: String = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let payload_hash = sha256_hex(payload);

    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{query_string}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );

    let credential_scope = format!(
        "{date_stamp}/{}/{SIGNING_SERVICE}/aws4_request",
        credentials.region
    );

    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );

    let signing_key = derive_signing_key(
        &credentials.secret_access_key,
        &date_stamp,
        &credentials.region,
        SIGNING_SERVICE,
    );

    let signature = hex::encode(hmac_sha256(&signing_key, string_to_sign.as_bytes()));

    format!(
        "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
        credentials.access_key_id
    )
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConverseRequest {
    messages: Vec<ConverseMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<SystemBlock>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inference_config: Option<InferenceConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_config: Option<ToolConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConverseMessage {
    role: String,
    content: Vec<ContentBlock>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum ContentBlock {
    Text(TextBlock),
    ToolUse(ToolUseWrapper),
    ToolResult(ToolResultWrapper),
    CachePointBlock(CachePointWrapper),
    Image(ImageWrapper),
}

#[derive(Debug, Serialize, Deserialize)]
struct ImageWrapper {
    image: ImageBlock,
}

#[derive(Debug, Serialize, Deserialize)]
struct ImageBlock {
    format: String,
    source: ImageSource,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImageSource {
    bytes: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TextBlock {
    text: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolUseWrapper {
    tool_use: ToolUseBlock,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolUseBlock {
    tool_use_id: String,
    name: String,
    input: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolResultWrapper {
    tool_result: ToolResultBlock,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolResultBlock {
    tool_use_id: String,
    content: Vec<ToolResultContent>,
    status: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachePointWrapper {
    cache_point: CachePoint,
}

#[derive(Debug, Serialize, Deserialize)]
struct ToolResultContent {
    text: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachePoint {
    #[serde(rename = "type")]
    cache_type: String,
}

impl CachePoint {
    fn default_cache() -> Self {
        Self {
            cache_type: "default".to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum SystemBlock {
    Text(TextBlock),
    CachePoint(CachePointWrapper),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InferenceConfig {
    max_tokens: u32,
    temperature: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolConfig {
    tools: Vec<ToolDefinition>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolDefinition {
    tool_spec: ToolSpecDef,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolSpecDef {
    name: String,
    description: String,
    input_schema: InputSchema,
}

#[derive(Debug, Serialize)]
struct InputSchema {
    json: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct ConverseResponse {
    #[serde(default)]
    output: Option<ConverseOutput>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<BedrockUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BedrockUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    cache_write_input_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ConverseOutput {
    #[serde(default)]
    message: Option<ConverseOutputMessage>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ConverseOutputMessage {
    role: String,
    content: Vec<ResponseContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
enum ResponseContentBlock {
    ToolUse(ResponseToolUseWrapper),
    Text(TextBlock),
    Other(serde_json::Value),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResponseToolUseWrapper {
    tool_use: ToolUseBlock,
}

pub struct BedrockProvider {
    auth: Option<BedrockAuth>,
    max_tokens: u32,
}

impl BedrockProvider {
    pub fn new() -> Self {

        if let Some(token) = env_optional("BEDROCK_API_KEY") {
            return Self {
                auth: Some(BedrockAuth::BearerToken(token)),
                max_tokens: DEFAULT_MAX_TOKENS,
            };
        }
        Self {
            auth: AwsCredentials::from_env().ok().map(BedrockAuth::SigV4),
            max_tokens: DEFAULT_MAX_TOKENS,
        }
    }

    pub async fn new_async() -> Self {

        if let Some(token) = env_optional("BEDROCK_API_KEY") {
            return Self {
                auth: Some(BedrockAuth::BearerToken(token)),
                max_tokens: DEFAULT_MAX_TOKENS,
            };
        }
        let auth = AwsCredentials::resolve().await.ok().map(BedrockAuth::SigV4);
        Self {
            auth,
            max_tokens: DEFAULT_MAX_TOKENS,
        }
    }

    pub fn with_bearer_token(token: &str) -> Self {
        Self {
            auth: Some(BedrockAuth::BearerToken(token.to_string())),
            max_tokens: DEFAULT_MAX_TOKENS,
        }
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    fn http_client(&self) -> Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts("provider.bedrock", 120, 10)
    }

    fn encode_model_path(model_id: &str) -> String {
        model_id.replace(':', "%3A")
    }

    fn resolve_region() -> String {
        env_optional("AWS_REGION")
            .or_else(|| env_optional("AWS_DEFAULT_REGION"))
            .unwrap_or_else(|| DEFAULT_REGION.to_string())
    }

    fn endpoint_url(region: &str, model_id: &str) -> String {
        format!("https://{ENDPOINT_PREFIX}.{region}.amazonaws.com/model/{model_id}/converse")
    }

    fn canonical_uri(model_id: &str) -> String {
        let encoded = Self::encode_model_path(model_id);
        format!("/model/{encoded}/converse")
    }

    async fn resolve_auth(&self) -> anyhow::Result<BedrockAuth> {

        if let Some(ref auth) = self.auth {
            match auth {
                BedrockAuth::BearerToken(token) => {
                    return Ok(BedrockAuth::BearerToken(token.clone()));
                }
                BedrockAuth::SigV4(_) => {

                }
            }
        }

        if let Some(token) = env_optional("BEDROCK_API_KEY") {
            return Ok(BedrockAuth::BearerToken(token));
        }

        if let Ok(creds) = AwsCredentials::from_env() {
            return Ok(BedrockAuth::SigV4(creds));
        }
        Ok(BedrockAuth::SigV4(AwsCredentials::from_imds().await?))
    }

    fn should_cache_system(text: &str) -> bool {
        text.len() > 3072
    }

    fn should_cache_conversation(messages: &[ChatMessage]) -> bool {
        messages.iter().filter(|m| m.role != "system").count() > 4
    }

    fn convert_messages(
        messages: &[ChatMessage],
    ) -> (Option<Vec<SystemBlock>>, Vec<ConverseMessage>) {
        let mut system_blocks = Vec::new();
        let mut converse_messages = Vec::new();

        for msg in messages {
            match msg.role.as_str() {
                "system" => {
                    system_blocks.push(SystemBlock::Text(TextBlock {
                        text: msg.content.clone(),
                    }));
                }
                "assistant" => {
                    if let Some(blocks) = Self::parse_assistant_tool_call_message(&msg.content) {
                        converse_messages.push(ConverseMessage {
                            role: "assistant".to_string(),
                            content: blocks,
                        });
                    } else {
                        converse_messages.push(ConverseMessage {
                            role: "assistant".to_string(),
                            content: vec![ContentBlock::Text(TextBlock {
                                text: msg.content.clone(),
                            })],
                        });
                    }
                }
                "tool" => {
                    let tool_result_msg = Self::parse_tool_result_message(&msg.content)
                        .unwrap_or_else(|| {

                            let tool_use_id = Self::extract_tool_call_id(&msg.content)
                                .or_else(|| Self::last_pending_tool_use_id(&converse_messages))
                                .unwrap_or_else(|| "unknown".to_string());

                            tracing::warn!(
                                "Failed to parse tool result message, creating error \
                                 toolResult for tool_use_id={}",
                                tool_use_id
                            );

                            ConverseMessage {
                                role: "user".to_string(),
                                content: vec![ContentBlock::ToolResult(ToolResultWrapper {
                                    tool_result: ToolResultBlock {
                                        tool_use_id,
                                        content: vec![ToolResultContent {
                                            text: msg.content.clone(),
                                        }],
                                        status: "error".to_string(),
                                    },
                                })],
                            }
                        });

                    if let Some(last) = converse_messages.last_mut() {
                        if last.role == "user"
                            && last
                                .content
                                .iter()
                                .all(|b| matches!(b, ContentBlock::ToolResult(_)))
                        {
                            last.content.extend(tool_result_msg.content);
                            continue;
                        }
                    }
                    converse_messages.push(tool_result_msg);
                }
                _ => {
                    let content_blocks = Self::parse_user_content_blocks(&msg.content);
                    converse_messages.push(ConverseMessage {
                        role: "user".to_string(),
                        content: content_blocks,
                    });
                }
            }
        }

        let system = if system_blocks.is_empty() {
            None
        } else {
            Some(system_blocks)
        };
        (system, converse_messages)
    }

    fn extract_tool_call_id(content: &str) -> Option<String> {
        let value = serde_json::from_str::<serde_json::Value>(content).ok()?;
        value
            .get("tool_call_id")
            .or_else(|| value.get("tool_use_id"))
            .or_else(|| value.get("toolUseId"))
            .and_then(serde_json::Value::as_str)
            .map(String::from)
    }

    fn last_pending_tool_use_id(converse_messages: &[ConverseMessage]) -> Option<String> {
        let last_assistant = converse_messages
            .iter()
            .rev()
            .find(|m| m.role == "assistant")?;

        let tool_use_ids: Vec<&str> = last_assistant
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse(wrapper) => Some(wrapper.tool_use.tool_use_id.as_str()),
                _ => None,
            })
            .collect();

        let answered_ids: Vec<&str> = converse_messages
            .iter()
            .rev()
            .take_while(|m| m.role == "user")
            .flat_map(|m| m.content.iter())
            .filter_map(|b| match b {
                ContentBlock::ToolResult(wrapper) => Some(wrapper.tool_result.tool_use_id.as_str()),
                _ => None,
            })
            .collect();

        tool_use_ids
            .into_iter()
            .find(|id| !answered_ids.contains(id))
            .map(String::from)
    }

    fn parse_user_content_blocks(content: &str) -> Vec<ContentBlock> {
        let mut blocks: Vec<ContentBlock> = Vec::new();
        let mut remaining = content;
        let has_image = content.contains("[IMAGE:");
        tracing::info!(
            "parse_user_content_blocks called, len={}, has_image={}",
            content.len(),
            has_image
        );

        while let Some(start) = remaining.find("[IMAGE:") {

            let text_before = &remaining[..start];
            if !text_before.trim().is_empty() {
                blocks.push(ContentBlock::Text(TextBlock {
                    text: text_before.to_string(),
                }));
            }

            let after = &remaining[start + 7..];
            if let Some(end) = after.find(']') {
                let src = &after[..end];
                remaining = &after[end + 1..];

                if let Some(rest) = src.strip_prefix("data:") {
                    if let Some(semi) = rest.find(';') {
                        let mime = &rest[..semi];
                        let after_semi = &rest[semi + 1..];
                        if let Some(b64) = after_semi.strip_prefix("base64,") {
                            let format = match mime {
                                "image/png" => "png",
                                "image/gif" => "gif",
                                "image/webp" => "webp",
                                _ => "jpeg",
                            };
                            blocks.push(ContentBlock::Image(ImageWrapper {
                                image: ImageBlock {
                                    format: format.to_string(),
                                    source: ImageSource {
                                        bytes: b64.to_string(),
                                    },
                                },
                            }));
                            continue;
                        }
                    }
                }

                blocks.push(ContentBlock::Text(TextBlock {
                    text: format!("[image: {}]", src),
                }));
            } else {

                blocks.push(ContentBlock::Text(TextBlock {
                    text: remaining.to_string(),
                }));
                break;
            }
        }

        if !remaining.trim().is_empty() {
            blocks.push(ContentBlock::Text(TextBlock {
                text: remaining.to_string(),
            }));
        }

        if blocks.is_empty() {
            blocks.push(ContentBlock::Text(TextBlock {
                text: content.to_string(),
            }));
        }

        blocks
    }

    fn parse_assistant_tool_call_message(content: &str) -> Option<Vec<ContentBlock>> {
        let value = serde_json::from_str::<serde_json::Value>(content).ok()?;
        let tool_calls = value
            .get("tool_calls")
            .and_then(|v| serde_json::from_value::<Vec<ProviderToolCall>>(v.clone()).ok())?;

        let mut blocks = Vec::new();
        if let Some(text) = value
            .get("content")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            blocks.push(ContentBlock::Text(TextBlock {
                text: text.to_string(),
            }));
        }
        for call in tool_calls {
            let input = serde_json::from_str::<serde_json::Value>(&call.arguments)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
            blocks.push(ContentBlock::ToolUse(ToolUseWrapper {
                tool_use: ToolUseBlock {
                    tool_use_id: call.id,
                    name: call.name,
                    input,
                },
            }));
        }
        Some(blocks)
    }

    fn parse_tool_result_message(content: &str) -> Option<ConverseMessage> {
        let value = serde_json::from_str::<serde_json::Value>(content).ok()?;
        let tool_use_id = value
            .get("tool_call_id")
            .or_else(|| value.get("tool_use_id"))
            .or_else(|| value.get("toolUseId"))
            .and_then(serde_json::Value::as_str)?
            .to_string();
        let result = value
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        Some(ConverseMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult(ToolResultWrapper {
                tool_result: ToolResultBlock {
                    tool_use_id,
                    content: vec![ToolResultContent { text: result }],
                    status: "success".to_string(),
                },
            })],
        })
    }

    fn convert_tools_to_converse(tools: Option<&[ToolSpec]>) -> Option<ToolConfig> {
        let items = tools?;
        if items.is_empty() {
            return None;
        }
        let deduped = crate::tools::dedupe_tool_specs(items);
        let tool_defs: Vec<ToolDefinition> = deduped
            .iter()
            .map(|tool| ToolDefinition {
                tool_spec: ToolSpecDef {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    input_schema: InputSchema {
                        json: tool.parameters.clone(),
                    },
                },
            })
            .collect();
        if tool_defs.is_empty() {
            None
        } else {
            Some(ToolConfig { tools: tool_defs })
        }
    }

    fn parse_converse_response(response: ConverseResponse) -> ProviderChatResponse {
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let stop_reason = response
            .stop_reason
            .as_deref()
            .and_then(crate::providers::traits::StopReason::from_wire);

        let usage = response.usage.map(|u| TokenUsage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            cached_input_tokens: u.cache_read_input_tokens,
            cache_creation_input_tokens: u.cache_write_input_tokens,
        });

        if let Some(output) = response.output {
            if let Some(message) = output.message {
                for block in message.content {
                    match block {
                        ResponseContentBlock::Text(tb) => {
                            let trimmed = tb.text.trim().to_string();
                            if !trimmed.is_empty() {
                                text_parts.push(trimmed);
                            }
                        }
                        ResponseContentBlock::ToolUse(wrapper) => {
                            if !wrapper.tool_use.name.is_empty() {
                                tool_calls.push(ProviderToolCall {
                                    id: wrapper.tool_use.tool_use_id,
                                    name: wrapper.tool_use.name,
                                    arguments: wrapper.tool_use.input.to_string(),
                                });
                            }
                        }
                        ResponseContentBlock::Other(_) => {}
                    }
                }
            }
        }

        ProviderChatResponse {
            text: if text_parts.is_empty() {
                None
            } else {
                Some(text_parts.join("\n"))
            },
            tool_calls,
            usage,
            reasoning_content: None,
            thinking_signature: None,
            stop_reason,
        }
    }

    async fn send_converse_request(
        &self,
        auth: &BedrockAuth,
        model: &str,
        request_body: &ConverseRequest,
    ) -> anyhow::Result<ConverseResponse> {
        let payload = serde_json::to_vec(request_body)?;

        if let Ok(debug_val) = serde_json::from_slice::<serde_json::Value>(&payload) {
            if let Some(msgs) = debug_val.get("messages").and_then(|m| m.as_array()) {
                for msg in msgs {
                    if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
                        for block in content {
                            if block.get("image").is_some() {
                                let mut b = block.clone();
                                if let Some(img) = b.get_mut("image") {
                                    if let Some(src) = img.get_mut("source") {
                                        if let Some(bytes) = src.get_mut("bytes") {
                                            if let Some(s) = bytes.as_str() {
                                                *bytes = serde_json::json!(format!(
                                                    "<base64 {} chars>",
                                                    s.len()
                                                ));
                                            }
                                        }
                                    }
                                }
                                tracing::info!(
                                    "Bedrock image block: {}",
                                    serde_json::to_string(&b).unwrap_or_default()
                                );
                            }
                        }
                    }
                }
            }
        }

        let response: reqwest::Response = match auth {
            BedrockAuth::BearerToken(token) => {
                let region = Self::resolve_region();
                let url = Self::endpoint_url(&region, model);

                self.http_client()
                    .post(&url)
                    .header("content-type", "application/json")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(payload)
                    .send()
                    .await?
            }
            BedrockAuth::SigV4(credentials) => {
                let url = Self::endpoint_url(&credentials.region, model);
                let canonical_uri = Self::canonical_uri(model);
                let now = chrono::Utc::now();
                let host = credentials.host();
                let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

                let mut headers_to_sign = vec![
                    ("content-type".to_string(), "application/json".to_string()),
                    ("host".to_string(), host),
                    ("x-amz-date".to_string(), amz_date.clone()),
                ];
                if let Some(ref session_token) = credentials.session_token {
                    headers_to_sign
                        .push(("x-amz-security-token".to_string(), session_token.clone()));
                }
                headers_to_sign.sort_by(|a, b| a.0.cmp(&b.0));

                let authorization = build_authorization_header(
                    credentials,
                    "POST",
                    &canonical_uri,
                    "",
                    &headers_to_sign,
                    &payload,
                    &now,
                );

                let mut request = self
                    .http_client()
                    .post(&url)
                    .header("content-type", "application/json")
                    .header("x-amz-date", &amz_date)
                    .header("authorization", &authorization);

                if let Some(ref session_token) = credentials.session_token {
                    request = request.header("x-amz-security-token", session_token);
                }

                request.body(payload).send().await?
            }
        };

        if !response.status().is_success() {
            return Err(super::api_error("Bedrock", response).await);
        }

        let converse_response: ConverseResponse = response.json().await?;
        Ok(converse_response)
    }
}

#[async_trait]
impl Provider for BedrockProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: true,
            vision: true,
            prompt_caching: true,
            responses_api: false,
        }
    }

    fn supports_native_tools(&self) -> bool {
        true
    }

    fn convert_tools(&self, tools: &[ToolSpec]) -> ToolsPayload {
        let tool_values: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "toolSpec": {
                        "name": t.name,
                        "description": t.description,
                        "inputSchema": { "json": t.parameters }
                    }
                })
            })
            .collect();
        ToolsPayload::Anthropic { tools: tool_values }
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let auth = self.resolve_auth().await?;

        let system = system_prompt.map(|text| {
            let mut blocks = vec![SystemBlock::Text(TextBlock {
                text: text.to_string(),
            })];
            if Self::should_cache_system(text) {
                blocks.push(SystemBlock::CachePoint(CachePointWrapper {
                    cache_point: CachePoint::default_cache(),
                }));
            }
            blocks
        });

        let request = ConverseRequest {
            system,
            messages: vec![ConverseMessage {
                role: "user".to_string(),
                content: Self::parse_user_content_blocks(message),
            }],
            inference_config: Some(InferenceConfig {
                max_tokens: self.max_tokens,
                temperature,
            }),
            tool_config: None,
        };

        let response = self.send_converse_request(&auth, model, &request).await?;

        Self::parse_converse_response(response)
            .text
            .ok_or_else(|| anyhow::anyhow!("No response from Bedrock"))
    }

    async fn chat(
        &self,
        request: ProviderChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let auth = self.resolve_auth().await?;

        let sanitized_messages = crate::providers::sanitize::sanitize_messages_before_send_for_trait(
            self,
            request.messages.to_vec(),
            model,
            self.max_tokens as usize,
            None,
        );
        let (system_blocks, mut converse_messages) = Self::convert_messages(&sanitized_messages);

        let system = system_blocks.map(|mut blocks| {
            let has_large_system = blocks
                .iter()
                .any(|b| matches!(b, SystemBlock::Text(tb) if Self::should_cache_system(&tb.text)));
            if has_large_system {
                blocks.push(SystemBlock::CachePoint(CachePointWrapper {
                    cache_point: CachePoint::default_cache(),
                }));
            }
            blocks
        });

        if Self::should_cache_conversation(&sanitized_messages) {
            if let Some(last_msg) = converse_messages.last_mut() {
                last_msg
                    .content
                    .push(ContentBlock::CachePointBlock(CachePointWrapper {
                        cache_point: CachePoint::default_cache(),
                    }));
            }
        }

        let tool_config = Self::convert_tools_to_converse(request.tools);

        let converse_request = ConverseRequest {
            system,
            messages: converse_messages,
            inference_config: Some(InferenceConfig {
                max_tokens: self.max_tokens,
                temperature,
            }),
            tool_config,
        };

        let response = self
            .send_converse_request(&auth, model, &converse_request)
            .await?;

        Ok(Self::parse_converse_response(response))
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        let region = match self.auth {
            Some(BedrockAuth::SigV4(ref creds)) => creds.region.clone(),
            Some(BedrockAuth::BearerToken(_)) => Self::resolve_region(),
            None => return Ok(()),
        };
        let url = format!("https://{ENDPOINT_PREFIX}.{region}.amazonaws.com/");
        let _ = self.http_client().get(&url).send().await;
        Ok(())
    }
}

