// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};

use crate::config::TtsConfig;

const DEFAULT_MAX_TEXT_LENGTH: usize = 4096;

const TTS_HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

#[async_trait::async_trait]
pub trait TtsProvider: Send + Sync {

    fn name(&self) -> &str;

    async fn synthesize(&self, text: &str, voice: &str) -> Result<Vec<u8>>;

    fn supported_voices(&self) -> Vec<String>;

    fn supported_formats(&self) -> Vec<String>;
}

pub struct OpenAiTtsProvider {
    api_key: String,
    model: String,
    speed: f64,
    client: reqwest::Client,
}

impl OpenAiTtsProvider {

    pub fn new(config: &crate::config::OpenAiTtsConfig) -> Result<Self> {
        let api_key = config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                std::env::var("OPENAI_API_KEY")
                    .ok()
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
            })
            .context("Missing OpenAI TTS API key: set [tts.openai].api_key or OPENAI_API_KEY")?;

        Ok(Self {
            api_key,
            model: config.model.clone(),
            speed: config.speed,
            client: reqwest::Client::builder()
                .timeout(TTS_HTTP_TIMEOUT)
                .build()
                .context("Failed to build HTTP client for OpenAI TTS")?,
        })
    }
}

#[async_trait::async_trait]
impl TtsProvider for OpenAiTtsProvider {
    fn name(&self) -> &str {
        "openai"
    }

    async fn synthesize(&self, text: &str, voice: &str) -> Result<Vec<u8>> {
        let body = serde_json::json!({
            "model": self.model,
            "input": text,
            "voice": voice,
            "speed": self.speed,
            "response_format": "opus",
        });

        let resp = self
            .client
            .post("https://api.openai.com/v1/audio/speech")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("Failed to send OpenAI TTS request")?;

        let status = resp.status();
        if !status.is_success() {
            let error_body: serde_json::Value = resp
                .json()
                .await
                .unwrap_or_else(|_| serde_json::json!({"error": "unknown"}));
            let msg = error_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            bail!("OpenAI TTS API error ({}): {}", status, msg);
        }

        let bytes = resp
            .bytes()
            .await
            .context("Failed to read OpenAI TTS response body")?;
        Ok(bytes.to_vec())
    }

    fn supported_voices(&self) -> Vec<String> {
        ["alloy", "echo", "fable", "onyx", "nova", "shimmer"]
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }

    fn supported_formats(&self) -> Vec<String> {
        ["mp3", "opus", "aac", "flac", "wav", "pcm"]
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }
}

pub struct ElevenLabsTtsProvider {
    api_key: String,
    model_id: String,
    stability: f64,
    similarity_boost: f64,
    client: reqwest::Client,
}

impl ElevenLabsTtsProvider {

    pub fn new(config: &crate::config::ElevenLabsTtsConfig) -> Result<Self> {
        let api_key = config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                std::env::var("ELEVENLABS_API_KEY")
                    .ok()
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
            })
            .context(
                "Missing ElevenLabs API key: set [tts.elevenlabs].api_key or ELEVENLABS_API_KEY",
            )?;

        Ok(Self {
            api_key,
            model_id: config.model_id.clone(),
            stability: config.stability,
            similarity_boost: config.similarity_boost,
            client: reqwest::Client::builder()
                .timeout(TTS_HTTP_TIMEOUT)
                .build()
                .context("Failed to build HTTP client for ElevenLabs TTS")?,
        })
    }
}

#[async_trait::async_trait]
impl TtsProvider for ElevenLabsTtsProvider {
    fn name(&self) -> &str {
        "elevenlabs"
    }

    async fn synthesize(&self, text: &str, voice: &str) -> Result<Vec<u8>> {
        if !voice
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            bail!("ElevenLabs voice ID contains invalid characters: {voice}");
        }
        let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{voice}");
        let body = serde_json::json!({
            "text": text,
            "model_id": self.model_id,
            "voice_settings": {
                "stability": self.stability,
                "similarity_boost": self.similarity_boost,
            },
        });

        let resp = self
            .client
            .post(&url)
            .header("xi-api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .context("Failed to send ElevenLabs TTS request")?;

        let status = resp.status();
        if !status.is_success() {
            let error_body: serde_json::Value = resp
                .json()
                .await
                .unwrap_or_else(|_| serde_json::json!({"error": "unknown"}));
            let msg = error_body["detail"]["message"]
                .as_str()
                .or_else(|| error_body["detail"].as_str())
                .unwrap_or("unknown error");
            bail!("ElevenLabs TTS API error ({}): {}", status, msg);
        }

        let bytes = resp
            .bytes()
            .await
            .context("Failed to read ElevenLabs TTS response body")?;
        Ok(bytes.to_vec())
    }

    fn supported_voices(&self) -> Vec<String> {

        Vec::new()
    }

    fn supported_formats(&self) -> Vec<String> {
        ["mp3", "pcm", "ulaw"]
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }
}

pub struct GoogleTtsProvider {
    api_key: String,
    language_code: String,
    client: reqwest::Client,
}

impl GoogleTtsProvider {

    pub fn new(config: &crate::config::GoogleTtsConfig) -> Result<Self> {
        let api_key = config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                std::env::var("GOOGLE_TTS_API_KEY")
                    .ok()
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
            })
            .context(
                "Missing Google TTS API key: set [tts.google].api_key or GOOGLE_TTS_API_KEY",
            )?;

        Ok(Self {
            api_key,
            language_code: config.language_code.clone(),
            client: reqwest::Client::builder()
                .timeout(TTS_HTTP_TIMEOUT)
                .build()
                .context("Failed to build HTTP client for Google TTS")?,
        })
    }
}

#[async_trait::async_trait]
impl TtsProvider for GoogleTtsProvider {
    fn name(&self) -> &str {
        "google"
    }

    async fn synthesize(&self, text: &str, voice: &str) -> Result<Vec<u8>> {
        let url = "https://texttospeech.googleapis.com/v1/text:synthesize";
        let body = serde_json::json!({
            "input": { "text": text },
            "voice": {
                "languageCode": self.language_code,
                "name": voice,
            },
            "audioConfig": {
                "audioEncoding": "MP3",
            },
        });

        let resp = self
            .client
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .context("Failed to send Google TTS request")?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse Google TTS response")?;

        if !status.is_success() {
            let msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            bail!("Google TTS API error ({}): {}", status, msg);
        }

        let audio_b64 = resp_body["audioContent"]
            .as_str()
            .context("Google TTS response missing 'audioContent' field")?;

        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(audio_b64)
            .context("Failed to decode Google TTS base64 audio")?;
        Ok(bytes)
    }

    fn supported_voices(&self) -> Vec<String> {

        [
            "en-US-Standard-A",
            "en-US-Standard-B",
            "en-US-Standard-C",
            "en-US-Standard-D",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
    }

    fn supported_formats(&self) -> Vec<String> {
        ["mp3", "wav", "ogg"]
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }
}

pub struct EdgeTtsProvider {
    binary_path: String,
}

impl EdgeTtsProvider {

    const ALLOWED_BINARIES: &[&str] = &["edge-tts", "edge-playback"];

    pub fn new(config: &crate::config::EdgeTtsConfig) -> Result<Self> {
        let path = &config.binary_path;
        if path.contains('/') || path.contains('\\') {
            bail!(
                "Edge TTS binary_path must be a bare command name without path separators, got: {path}"
            );
        }
        if !Self::ALLOWED_BINARIES.contains(&path.as_str()) {
            bail!(
                "Edge TTS binary_path must be one of {:?}, got: {path}",
                Self::ALLOWED_BINARIES,
            );
        }
        Ok(Self {
            binary_path: config.binary_path.clone(),
        })
    }
}

#[async_trait::async_trait]
impl TtsProvider for EdgeTtsProvider {
    fn name(&self) -> &str {
        "edge"
    }

    async fn synthesize(&self, text: &str, voice: &str) -> Result<Vec<u8>> {
        let temp_dir = std::env::temp_dir();
        let output_file = temp_dir.join(format!("sen_tts_{}.mp3", uuid::Uuid::new_v4()));
        let output_path = output_file
            .to_str()
            .context("Failed to build temp file path for Edge TTS")?;

        let output = tokio::time::timeout(
            TTS_HTTP_TIMEOUT,
            crate::util::hidden_async_command(&self.binary_path)
                .arg("--text")
                .arg(text)
                .arg("--voice")
                .arg(voice)
                .arg("--write-media")
                .arg(output_path)
                .output(),
        )
        .await
        .context("Edge TTS subprocess timed out")?
        .context("Failed to spawn edge-tts subprocess")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            let _ = tokio::fs::remove_file(&output_file).await;
            bail!("edge-tts failed (exit {}): {}", output.status, stderr);
        }

        let bytes = tokio::fs::read(&output_file)
            .await
            .context("Failed to read edge-tts output file")?;

        let _ = tokio::fs::remove_file(&output_file).await;

        Ok(bytes)
    }

    fn supported_voices(&self) -> Vec<String> {

        [
            "en-US-AriaNeural",
            "en-US-GuyNeural",
            "en-US-JennyNeural",
            "en-GB-SoniaNeural",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
    }

    fn supported_formats(&self) -> Vec<String> {
        vec!["mp3".to_string()]
    }
}

pub struct PiperTtsProvider {
    client: reqwest::Client,
    api_url: String,
}

impl PiperTtsProvider {

    pub fn new(api_url: &str) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(TTS_HTTP_TIMEOUT)
                .build()
                .context("Failed to build HTTP client for Piper TTS")?,
            api_url: api_url.to_string(),
        })
    }
}

#[async_trait::async_trait]
impl TtsProvider for PiperTtsProvider {
    fn name(&self) -> &str {
        "piper"
    }

    async fn synthesize(&self, text: &str, voice: &str) -> Result<Vec<u8>> {
        let body = serde_json::json!({
            "model": "tts-1",
            "input": text,
            "voice": voice,
        });

        let resp = self
            .client
            .post(&self.api_url)
            .json(&body)
            .send()
            .await
            .context("Failed to send Piper TTS request")?;

        let status = resp.status();
        if !status.is_success() {
            let error_body: serde_json::Value = resp
                .json()
                .await
                .unwrap_or_else(|_| serde_json::json!({"error": "unknown"}));
            let msg = error_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            bail!("Piper TTS API error ({}): {}", status, msg);
        }

        let bytes = resp
            .bytes()
            .await
            .context("Failed to read Piper TTS response body")?;
        Ok(bytes.to_vec())
    }

    fn supported_voices(&self) -> Vec<String> {

        Vec::new()
    }

    fn supported_formats(&self) -> Vec<String> {
        ["mp3", "wav", "opus"]
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }
}

pub struct TtsManager {
    providers: HashMap<String, Box<dyn TtsProvider>>,
    default_provider: String,
    default_voice: String,
    max_text_length: usize,
}

impl TtsManager {

    pub fn new(config: &TtsConfig) -> Result<Self> {
        let mut providers: HashMap<String, Box<dyn TtsProvider>> = HashMap::new();

        if let Some(ref openai_cfg) = config.openai {
            match OpenAiTtsProvider::new(openai_cfg) {
                Ok(p) => {
                    providers.insert("openai".to_string(), Box::new(p));
                }
                Err(e) => {
                    tracing::warn!("Skipping OpenAI TTS provider: {e}");
                }
            }
        }

        if let Some(ref elevenlabs_cfg) = config.elevenlabs {
            match ElevenLabsTtsProvider::new(elevenlabs_cfg) {
                Ok(p) => {
                    providers.insert("elevenlabs".to_string(), Box::new(p));
                }
                Err(e) => {
                    tracing::warn!("Skipping ElevenLabs TTS provider: {e}");
                }
            }
        }

        if let Some(ref google_cfg) = config.google {
            match GoogleTtsProvider::new(google_cfg) {
                Ok(p) => {
                    providers.insert("google".to_string(), Box::new(p));
                }
                Err(e) => {
                    tracing::warn!("Skipping Google TTS provider: {e}");
                }
            }
        }

        if let Some(ref edge_cfg) = config.edge {
            match EdgeTtsProvider::new(edge_cfg) {
                Ok(p) => {
                    providers.insert("edge".to_string(), Box::new(p));
                }
                Err(e) => {
                    tracing::warn!("Skipping Edge TTS provider: {e}");
                }
            }
        }

        if let Some(ref piper_cfg) = config.piper {
            match PiperTtsProvider::new(&piper_cfg.api_url) {
                Ok(provider) => {
                    providers.insert("piper".to_string(), Box::new(provider));
                }
                Err(e) => tracing::warn!("Failed to create Piper TTS provider: {e}"),
            }
        }

        let max_text_length = if config.max_text_length == 0 {
            DEFAULT_MAX_TEXT_LENGTH
        } else {
            config.max_text_length
        };

        Ok(Self {
            providers,
            default_provider: config.default_provider.clone(),
            default_voice: config.default_voice.clone(),
            max_text_length,
        })
    }

    pub async fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
        self.synthesize_with_provider(text, &self.default_provider, &self.default_voice)
            .await
    }

    pub async fn synthesize_with_provider(
        &self,
        text: &str,
        provider: &str,
        voice: &str,
    ) -> Result<Vec<u8>> {
        if text.is_empty() {
            bail!("TTS text must not be empty");
        }
        let char_count = text.chars().count();
        if char_count > self.max_text_length {
            bail!(
                "TTS text too long ({} chars, max {})",
                char_count,
                self.max_text_length
            );
        }

        let tts = self.providers.get(provider).ok_or_else(|| {
            anyhow::anyhow!(
                "TTS provider '{}' not configured (available: {})",
                provider,
                self.available_providers().join(", ")
            )
        })?;

        tts.synthesize(text, voice).await
    }

    pub fn available_providers(&self) -> Vec<String> {
        let mut names: Vec<_> = self.providers.keys().cloned().collect();
        names.sort();
        names
    }
}

