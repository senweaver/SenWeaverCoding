// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::config::schema::Config;

#[derive(Debug, Clone)]
pub struct ResolvedProvider {
    pub provider_id: String,
    pub base_url: String,
    pub api_key: Option<String>,
}

fn default_base_url(provider_id: &str) -> &'static str {
    match provider_id.to_ascii_lowercase().as_str() {
        "openai" | "openai-codex" | "openai-responses" => "https://api.openai.com/v1",
        "openrouter" => "https://openrouter.ai/api/v1",
        "deepseek" => "https://api.deepseek.com/v1",
        "gemini" | "google" => "https://generativelanguage.googleapis.com/v1beta",
        "groq" => "https://api.groq.com/openai/v1",
        "mistral" => "https://api.mistral.ai/v1",
        "xai" | "grok" => "https://api.x.ai/v1",
        "elevenlabs" => "https://api.elevenlabs.io",
        "fal" => "https://fal.run",
        "volcengine" | "doubao" => "https://ark.cn-beijing.volces.com/api/v3",
        "minimax" => "https://api.minimaxi.chat/v1",
        _ => "https://api.openai.com/v1",
    }
}

fn env_key_for(provider_id: &str) -> Option<&'static str> {
    Some(match provider_id.to_ascii_lowercase().as_str() {
        "openai" | "openai-codex" | "openai-responses" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "deepseek" => "DEEPSEEK_API_KEY",
        "gemini" | "google" => "GEMINI_API_KEY",
        "groq" => "GROQ_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        "xai" | "grok" => "XAI_API_KEY",
        "elevenlabs" => "ELEVENLABS_API_KEY",
        "fal" => "FAL_API_KEY",
        "volcengine" => "ARK_API_KEY",
        "minimax" => "MINIMAX_API_KEY",
        _ => return None,
    })
}

fn key_from_config(config: &Config, provider_id: &str) -> Option<String> {
    if let Some(profile) = config.model_providers.get(provider_id) {
        if let Some(key) = profile
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(key.to_string());
        }
    }
    if let Some(key) = config
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(key.to_string());
    }
    env_key_for(provider_id)
        .and_then(|var| std::env::var(var).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn base_url_from_config(config: &Config, provider_id: &str) -> String {
    if let Some(profile) = config.model_providers.get(provider_id) {
        if let Some(url) = profile
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return url.trim_end_matches('/').to_string();
        }
    }
    default_base_url(provider_id).to_string()
}

pub fn provider_has_key(config: &Config, provider_id: &str) -> bool {
    if let Some(profile) = config.model_providers.get(provider_id) {
        if profile
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some()
        {
            return true;
        }
    }
    env_key_for(provider_id)
        .and_then(|var| std::env::var(var).ok())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

pub fn provider_for_model(config: &Config, model: &str) -> Option<String> {
    let needle = model.trim();
    if needle.is_empty() {
        return None;
    }
    for (id, profile) in &config.model_providers {
        if profile.model_names.iter().any(|m| m == needle) {
            return Some(id.clone());
        }
        if profile.models.values().any(|m| m == needle) {
            return Some(id.clone());
        }
    }
    None
}

pub fn resolve(config: &Config, provider_hint: Option<&str>, model: &str) -> ResolvedProvider {
    let provider_id = provider_hint
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
        .or_else(|| provider_for_model(config, model))
        .or_else(|| {
            config
                .default_provider
                .as_deref()
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or_else(|| "openai".to_string());

    let base_url = base_url_from_config(config, &provider_id);
    let api_key = key_from_config(config, &provider_id);

    ResolvedProvider {
        provider_id,
        base_url,
        api_key,
    }
}
