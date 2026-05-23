// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use reqwest::multipart::{Form, Part};

use crate::config::TranscriptionConfig;

const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;

const TRANSCRIPTION_TIMEOUT_SECS: u64 = 120;

fn mime_for_audio(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "flac" => Some("audio/flac"),
        "mp3" | "mpeg" | "mpga" => Some("audio/mpeg"),
        "mp4" | "m4a" => Some("audio/mp4"),
        "ogg" | "oga" => Some("audio/ogg"),
        "opus" => Some("audio/opus"),
        "wav" => Some("audio/wav"),
        "webm" => Some("audio/webm"),
        _ => None,
    }
}

fn normalize_audio_filename(file_name: &str) -> String {
    match file_name.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("oga") => format!("{stem}.ogg"),
        _ => file_name.to_string(),
    }
}

fn resolve_transcription_api_key(config: &TranscriptionConfig) -> Result<String> {

    if let Some(ref key) = config.api_key {
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    if config.api_url.contains("openai.com") {
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            return Ok(key);
        }
    } else if config.api_url.contains("groq.com") {
        if let Ok(key) = std::env::var("GROQ_API_KEY") {
            return Ok(key);
        }
    }

    for var in ["TRANSCRIPTION_API_KEY", "GROQ_API_KEY", "OPENAI_API_KEY"] {
        if let Ok(key) = std::env::var(var) {
            return Ok(key);
        }
    }

    bail!(
        "No API key found for voice transcription — set one of: \
         transcription.api_key in config, TRANSCRIPTION_API_KEY, GROQ_API_KEY, or OPENAI_API_KEY"
    );
}

fn resolve_audio_format(file_name: &str) -> Result<(String, &'static str)> {
    let normalized_name = normalize_audio_filename(file_name);
    let extension = normalized_name
        .rsplit_once('.')
        .map(|(_, e)| e)
        .unwrap_or("");
    let mime = mime_for_audio(extension).ok_or_else(|| {
        anyhow::anyhow!(
            "Unsupported audio format '.{extension}' — \
             accepted: flac, mp3, mp4, mpeg, mpga, m4a, ogg, opus, wav, webm"
        )
    })?;
    Ok((normalized_name, mime))
}

fn validate_audio(audio_data: &[u8], file_name: &str) -> Result<(String, &'static str)> {
    if audio_data.len() > MAX_AUDIO_BYTES {
        bail!(
            "Audio file too large ({} bytes, max {MAX_AUDIO_BYTES})",
            audio_data.len()
        );
    }
    resolve_audio_format(file_name)
}

#[async_trait]
pub trait TranscriptionProvider: Send + Sync {

    fn name(&self) -> &str;

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String>;

    fn supported_formats(&self) -> Vec<String> {
        vec![
            "flac", "mp3", "mpeg", "mpga", "mp4", "m4a", "ogg", "oga", "opus", "wav", "webm",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }
}

pub struct GroqProvider {
    api_url: String,
    model: String,
    api_key: String,
    language: Option<String>,
}

impl GroqProvider {

    pub fn from_config(config: &TranscriptionConfig) -> Result<Self> {
        let api_key = config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                std::env::var("GROQ_API_KEY")
                    .ok()
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
            })
            .context(
                "Missing transcription API key: set [transcription].api_key or GROQ_API_KEY environment variable",
            )?;

        Ok(Self {
            api_url: config.api_url.clone(),
            model: config.model.clone(),
            api_key,
            language: config.language.clone(),
        })
    }
}

#[async_trait]
impl TranscriptionProvider for GroqProvider {
    fn name(&self) -> &str {
        "groq"
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String> {
        let (normalized_name, mime) = validate_audio(audio_data, file_name)?;

        let client = crate::services::get_services()
            .proxy_runtime()
            .build_client("transcription.groq");

        let file_part = Part::bytes(audio_data.to_vec())
            .file_name(normalized_name)
            .mime_str(mime)?;

        let mut form = Form::new()
            .part("file", file_part)
            .text("model", self.model.clone())
            .text("response_format", "json");

        if let Some(ref lang) = self.language {
            form = form.text("language", lang.clone());
        }

        let resp = client
            .post(&self.api_url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .context("Failed to send transcription request to Groq")?;

        parse_whisper_response(resp).await
    }
}

pub struct OpenAiWhisperProvider {
    api_key: String,
    model: String,
}

impl OpenAiWhisperProvider {
    pub fn from_config(config: &crate::config::OpenAiSttConfig) -> Result<Self> {
        let api_key = config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToOwned::to_owned)
            .context("Missing OpenAI STT API key: set [transcription.openai].api_key")?;

        Ok(Self {
            api_key,
            model: config.model.clone(),
        })
    }
}

#[async_trait]
impl TranscriptionProvider for OpenAiWhisperProvider {
    fn name(&self) -> &str {
        "openai"
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String> {
        let (normalized_name, mime) = validate_audio(audio_data, file_name)?;

        let client = crate::services::get_services()
            .proxy_runtime()
            .build_client("transcription.openai");

        let file_part = Part::bytes(audio_data.to_vec())
            .file_name(normalized_name)
            .mime_str(mime)?;

        let form = Form::new()
            .part("file", file_part)
            .text("model", self.model.clone())
            .text("response_format", "json");

        let resp = client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .bearer_auth(&self.api_key)
            .multipart(form)
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .context("Failed to send transcription request to OpenAI")?;

        parse_whisper_response(resp).await
    }
}

pub struct DeepgramProvider {
    api_key: String,
    model: String,
}

impl DeepgramProvider {
    pub fn from_config(config: &crate::config::DeepgramSttConfig) -> Result<Self> {
        let api_key = config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToOwned::to_owned)
            .context("Missing Deepgram API key: set [transcription.deepgram].api_key")?;

        Ok(Self {
            api_key,
            model: config.model.clone(),
        })
    }
}

#[async_trait]
impl TranscriptionProvider for DeepgramProvider {
    fn name(&self) -> &str {
        "deepgram"
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String> {
        let (_, mime) = validate_audio(audio_data, file_name)?;

        let client = crate::services::get_services()
            .proxy_runtime()
            .build_client("transcription.deepgram");

        let url = format!(
            "https://api.deepgram.com/v1/listen?model={}&punctuate=true",
            self.model
        );

        let resp = client
            .post(&url)
            .header("Authorization", format!("Token {}", self.api_key))
            .header("Content-Type", mime)
            .body(audio_data.to_vec())
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .context("Failed to send transcription request to Deepgram")?;

        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse Deepgram response")?;

        if !status.is_success() {
            let error_msg = body["err_msg"]
                .as_str()
                .or_else(|| body["error"].as_str())
                .unwrap_or("unknown error");
            bail!("Deepgram API error ({}): {}", status, error_msg);
        }

        let text = body["results"]["channels"][0]["alternatives"][0]["transcript"]
            .as_str()
            .context("Deepgram response missing transcript field")?
            .to_string();

        Ok(text)
    }
}

pub struct AssemblyAiProvider {
    api_key: String,
}

impl AssemblyAiProvider {
    pub fn from_config(config: &crate::config::AssemblyAiSttConfig) -> Result<Self> {
        let api_key = config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToOwned::to_owned)
            .context("Missing AssemblyAI API key: set [transcription.assemblyai].api_key")?;

        Ok(Self { api_key })
    }
}

#[async_trait]
impl TranscriptionProvider for AssemblyAiProvider {
    fn name(&self) -> &str {
        "assemblyai"
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String> {
        let (_, _) = validate_audio(audio_data, file_name)?;

        let client = crate::services::get_services()
            .proxy_runtime()
            .build_client("transcription.assemblyai");

        let upload_resp = client
            .post("https://api.assemblyai.com/v2/upload")
            .header("Authorization", &self.api_key)
            .header("Content-Type", "application/octet-stream")
            .body(audio_data.to_vec())
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .context("Failed to upload audio to AssemblyAI")?;

        let upload_status = upload_resp.status();
        let upload_body: serde_json::Value = upload_resp
            .json()
            .await
            .context("Failed to parse AssemblyAI upload response")?;

        if !upload_status.is_success() {
            let error_msg = upload_body["error"].as_str().unwrap_or("unknown error");
            bail!("AssemblyAI upload error ({}): {}", upload_status, error_msg);
        }

        let upload_url = upload_body["upload_url"]
            .as_str()
            .context("AssemblyAI upload response missing 'upload_url'")?;

        let transcript_req = serde_json::json!({
            "audio_url": upload_url,
        });

        let create_resp = client
            .post("https://api.assemblyai.com/v2/transcript")
            .header("Authorization", &self.api_key)
            .json(&transcript_req)
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .context("Failed to create AssemblyAI transcription")?;

        let create_status = create_resp.status();
        let create_body: serde_json::Value = create_resp
            .json()
            .await
            .context("Failed to parse AssemblyAI create response")?;

        if !create_status.is_success() {
            let error_msg = create_body["error"].as_str().unwrap_or("unknown error");
            bail!(
                "AssemblyAI transcription error ({}): {}",
                create_status,
                error_msg
            );
        }

        let transcript_id = create_body["id"]
            .as_str()
            .context("AssemblyAI response missing 'id'")?;

        let poll_url = format!("https://api.assemblyai.com/v2/transcript/{transcript_id}");
        let poll_interval = std::time::Duration::from_secs(3);
        let poll_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(180);

        while tokio::time::Instant::now() < poll_deadline {
            tokio::time::sleep(poll_interval).await;

            let poll_resp = client
                .get(&poll_url)
                .header("Authorization", &self.api_key)
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .await
                .context("Failed to poll AssemblyAI transcription")?;

            let poll_status = poll_resp.status();
            let poll_body: serde_json::Value = poll_resp
                .json()
                .await
                .context("Failed to parse AssemblyAI poll response")?;

            if !poll_status.is_success() {
                let error_msg = poll_body["error"].as_str().unwrap_or("unknown poll error");
                bail!("AssemblyAI poll error ({}): {}", poll_status, error_msg);
            }

            let status_str = poll_body["status"].as_str().unwrap_or("unknown");

            match status_str {
                "completed" => {
                    let text = poll_body["text"]
                        .as_str()
                        .context("AssemblyAI response missing 'text'")?
                        .to_string();
                    return Ok(text);
                }
                "error" => {
                    let error_msg = poll_body["error"]
                        .as_str()
                        .unwrap_or("unknown transcription error");
                    bail!("AssemblyAI transcription failed: {}", error_msg);
                }
                _ => {}
            }
        }

        bail!("AssemblyAI transcription timed out after 180s")
    }
}

pub struct GoogleSttProvider {
    api_key: String,
    language_code: String,
}

impl GoogleSttProvider {
    pub fn from_config(config: &crate::config::GoogleSttConfig) -> Result<Self> {
        let api_key = config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToOwned::to_owned)
            .context("Missing Google STT API key: set [transcription.google].api_key")?;

        Ok(Self {
            api_key,
            language_code: config.language_code.clone(),
        })
    }
}

#[async_trait]
impl TranscriptionProvider for GoogleSttProvider {
    fn name(&self) -> &str {
        "google"
    }

    fn supported_formats(&self) -> Vec<String> {

        vec!["flac", "wav", "ogg", "opus", "mp3", "webm"]
            .into_iter()
            .map(String::from)
            .collect()
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String> {
        let (normalized_name, _) = validate_audio(audio_data, file_name)?;

        let client = crate::services::get_services()
            .proxy_runtime()
            .build_client("transcription.google");

        let encoding = match normalized_name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("flac") => "FLAC",
            Some("wav") => "LINEAR16",
            Some("ogg" | "opus") => "OGG_OPUS",
            Some("mp3") => "MP3",
            Some("webm") => "WEBM_OPUS",
            Some(ext) => bail!("Google STT does not support '.{ext}' input"),
            None => bail!("Google STT requires a file extension"),
        };

        let audio_content =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, audio_data);

        let request_body = serde_json::json!({
            "config": {
                "encoding": encoding,
                "languageCode": &self.language_code,
                "enableAutomaticPunctuation": true,
            },
            "audio": {
                "content": audio_content,
            }
        });

        let url = format!(
            "https://speech.googleapis.com/v1/speech:recognize?key={}",
            self.api_key
        );

        let resp = client
            .post(&url)
            .json(&request_body)
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .context("Failed to send transcription request to Google STT")?;

        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse Google STT response")?;

        if !status.is_success() {
            let error_msg = body["error"]["message"].as_str().unwrap_or("unknown error");
            bail!("Google STT API error ({}): {}", status, error_msg);
        }

        let text = body["results"][0]["alternatives"][0]["transcript"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(text)
    }
}

pub struct LocalWhisperProvider {
    url: String,
    bearer_token: String,
    max_audio_bytes: usize,
    timeout_secs: u64,
}

impl LocalWhisperProvider {

    pub fn from_config(config: &crate::config::LocalWhisperConfig) -> Result<Self> {
        let url = config.url.trim().to_string();
        anyhow::ensure!(!url.is_empty(), "local_whisper: `url` must not be empty");
        let parsed = url
            .parse::<reqwest::Url>()
            .with_context(|| format!("local_whisper: invalid `url`: {url:?}"))?;
        anyhow::ensure!(
            matches!(parsed.scheme(), "http" | "https"),
            "local_whisper: `url` must use http or https scheme, got {:?}",
            parsed.scheme()
        );

        let bearer_token = match config.bearer_token.as_deref().map(str::trim) {
            None => anyhow::bail!("local_whisper: `bearer_token` must be set"),
            Some("") => anyhow::bail!("local_whisper: `bearer_token` must not be empty"),
            Some(t) => t.to_string(),
        };

        anyhow::ensure!(
            config.max_audio_bytes > 0,
            "local_whisper: `max_audio_bytes` must be greater than zero"
        );

        anyhow::ensure!(
            config.timeout_secs > 0,
            "local_whisper: `timeout_secs` must be greater than zero"
        );

        Ok(Self {
            url,
            bearer_token,
            max_audio_bytes: config.max_audio_bytes,
            timeout_secs: config.timeout_secs,
        })
    }
}

#[async_trait]
impl TranscriptionProvider for LocalWhisperProvider {
    fn name(&self) -> &str {
        "local_whisper"
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String> {
        if audio_data.len() > self.max_audio_bytes {
            bail!(
                "Audio file too large ({} bytes, local_whisper max {})",
                audio_data.len(),
                self.max_audio_bytes
            );
        }

        let (normalized_name, mime) = resolve_audio_format(file_name)?;

        let client = crate::services::get_services()
            .proxy_runtime()
            .build_client("transcription.local_whisper");

        let file_part = Part::bytes(audio_data.to_vec())
            .file_name(normalized_name)
            .mime_str(mime)?;

        let resp = client
            .post(&self.url)
            .bearer_auth(&self.bearer_token)
            .multipart(Form::new().part("file", file_part))
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .send()
            .await
            .context("Failed to send audio to local Whisper endpoint")?;

        parse_whisper_response(resp).await
    }
}

async fn parse_whisper_response(resp: reqwest::Response) -> Result<String> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Transcription API error ({}): {}", status, body.trim());
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse transcription response")?;

    let text = body["text"]
        .as_str()
        .context("Transcription response missing 'text' field")?
        .to_string();

    Ok(text)
}

pub struct TranscriptionManager {
    providers: HashMap<String, Box<dyn TranscriptionProvider>>,
    default_provider: String,
}

impl TranscriptionManager {

    pub fn new(config: &TranscriptionConfig) -> Result<Self> {
        let mut providers: HashMap<String, Box<dyn TranscriptionProvider>> = HashMap::new();

        if let Ok(groq) = GroqProvider::from_config(config) {
            providers.insert("groq".to_string(), Box::new(groq));
        }

        if let Some(ref openai_cfg) = config.openai {
            if let Ok(p) = OpenAiWhisperProvider::from_config(openai_cfg) {
                providers.insert("openai".to_string(), Box::new(p));
            }
        }

        if let Some(ref deepgram_cfg) = config.deepgram {
            if let Ok(p) = DeepgramProvider::from_config(deepgram_cfg) {
                providers.insert("deepgram".to_string(), Box::new(p));
            }
        }

        if let Some(ref assemblyai_cfg) = config.assemblyai {
            if let Ok(p) = AssemblyAiProvider::from_config(assemblyai_cfg) {
                providers.insert("assemblyai".to_string(), Box::new(p));
            }
        }

        if let Some(ref google_cfg) = config.google {
            if let Ok(p) = GoogleSttProvider::from_config(google_cfg) {
                providers.insert("google".to_string(), Box::new(p));
            }
        }

        if let Some(ref local_cfg) = config.local_whisper {
            match LocalWhisperProvider::from_config(local_cfg) {
                Ok(p) => {
                    providers.insert("local_whisper".to_string(), Box::new(p));
                }
                Err(e) => {
                    tracing::warn!("local_whisper config invalid, provider skipped: {e}");
                }
            }
        }

        let default_provider = config.default_provider.clone();

        if config.enabled && !providers.contains_key(&default_provider) {
            let available: Vec<&str> = providers.keys().map(|k| k.as_str()).collect();
            bail!(
                "Default transcription provider '{}' is not configured. Available: {available:?}",
                default_provider
            );
        }

        Ok(Self {
            providers,
            default_provider,
        })
    }

    pub async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String> {
        self.transcribe_with_provider(audio_data, file_name, &self.default_provider)
            .await
    }

    pub async fn transcribe_with_provider(
        &self,
        audio_data: &[u8],
        file_name: &str,
        provider: &str,
    ) -> Result<String> {
        let p = self.providers.get(provider).ok_or_else(|| {
            let available: Vec<&str> = self.providers.keys().map(|k| k.as_str()).collect();
            anyhow::anyhow!(
                "Transcription provider '{provider}' not configured. Available: {available:?}"
            )
        })?;

        p.transcribe(audio_data, file_name).await
    }

    pub fn available_providers(&self) -> Vec<&str> {
        self.providers.keys().map(|k| k.as_str()).collect()
    }
}

pub async fn transcribe_audio(
    audio_data: Vec<u8>,
    file_name: &str,
    config: &TranscriptionConfig,
) -> Result<String> {

    validate_audio(&audio_data, file_name)?;

    match config.default_provider.as_str() {
        "groq" => {
            let groq = GroqProvider::from_config(config)?;
            groq.transcribe(&audio_data, file_name).await
        }
        "openai" => {
            let openai_cfg = config.openai.as_ref().context(
                "Default transcription provider 'openai' is not configured. Add [transcription.openai]",
            )?;
            let openai = OpenAiWhisperProvider::from_config(openai_cfg)?;
            openai.transcribe(&audio_data, file_name).await
        }
        "deepgram" => {
            let deepgram_cfg = config.deepgram.as_ref().context(
                "Default transcription provider 'deepgram' is not configured. Add [transcription.deepgram]",
            )?;
            let deepgram = DeepgramProvider::from_config(deepgram_cfg)?;
            deepgram.transcribe(&audio_data, file_name).await
        }
        "assemblyai" => {
            let assemblyai_cfg = config.assemblyai.as_ref().context(
                "Default transcription provider 'assemblyai' is not configured. Add [transcription.assemblyai]",
            )?;
            let assemblyai = AssemblyAiProvider::from_config(assemblyai_cfg)?;
            assemblyai.transcribe(&audio_data, file_name).await
        }
        "google" => {
            let google_cfg = config.google.as_ref().context(
                "Default transcription provider 'google' is not configured. Add [transcription.google]",
            )?;
            let google = GoogleSttProvider::from_config(google_cfg)?;
            google.transcribe(&audio_data, file_name).await
        }
        other => bail!("Unsupported transcription provider '{other}'"),
    }
}
