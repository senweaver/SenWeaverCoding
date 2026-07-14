// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use serde::Serialize;

use crate::config::{Config, ModelProviderConfig, MultimodalConfig};
use crate::providers::traits::{ChatMessage, ChatRequest, Provider};

pub struct VisionClient {
    provider: Box<dyn Provider>,
    model: String,
    multimodal: MultimodalConfig,
    temperature: f64,
}

impl VisionClient {
    pub fn from_config(config: &Config, provider_id: &str, model: &str) -> Result<Self> {
        let canonical_key = config
            .model_providers
            .get_key_value(provider_id)
            .map(|(k, _)| k.clone())
            .or_else(|| {
                config
                    .model_providers
                    .keys()
                    .find(|k| k.eq_ignore_ascii_case(provider_id))
                    .cloned()
            });
        let profile = canonical_key
            .as_deref()
            .and_then(|key| config.model_providers.get(key));

        let mut options = crate::providers::provider_runtime_options_from_config(config);
        if let Some(profile) = profile {
            if let Some(base) = profile.base_url.clone() {
                options.provider_api_url = Some(base);
            }
            if let Some(path) = profile.api_path.clone() {
                options.api_path = Some(path);
            }
            if let Some(max_tokens) = profile.max_tokens {
                options.provider_max_tokens = Some(max_tokens);
            }
            for (name, value) in crate::config::build_custom_headers_map(&profile.custom_headers)
            {
                options.extra_headers.insert(name, value);
            }
            if !profile.model_context_windows.is_empty() {
                options.model_context_windows = profile.model_context_windows.clone();
            }
        }

        let runtime_name = crate::providers::resolve_runtime_provider_name(
            canonical_key.as_deref().unwrap_or(provider_id),
            config,
        );

        let provider = crate::providers::create_provider_for_model(
            &runtime_name,
            model,
            profile.and_then(|p| p.api_key.as_deref()),
            profile.and_then(|p| p.base_url.as_deref()),
            &options,
        )?;

        Ok(Self {
            provider,
            model: model.to_string(),
            multimodal: config.multimodal.clone(),
            temperature: 0.2,
        })
    }

    pub fn max_reference_images(&self) -> usize {
        let (max_images, _) = self.multimodal.effective_limits();
        max_images.saturating_sub(1)
    }

    pub async fn complete_text(&self, system_prompt: &str, user_text: &str) -> Result<String> {
        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_text),
        ];
        let request = ChatRequest {
            messages: &messages,
            tools: None,
        };
        let response = self
            .provider
            .chat(request, &self.model, self.temperature)
            .await?;
        Ok(response.text.unwrap_or_default())
    }

    pub async fn complete_with_images(
        &self,
        system_prompt: &str,
        user_text: &str,
        image_data_uris: &[&str],
    ) -> Result<String> {
        let mut user_content = user_text.to_string();
        for uri in image_data_uris {
            user_content.push_str("\n\n[IMAGE:");
            user_content.push_str(uri);
            user_content.push(']');
        }
        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_content),
        ];
        let prepared =
            crate::multimodal::prepare_messages_for_provider(&messages, &self.multimodal).await?;
        let request = ChatRequest {
            messages: &prepared.messages,
            tools: None,
        };
        let response = self
            .provider
            .chat(request, &self.model, self.temperature)
            .await?;
        Ok(response.text.unwrap_or_default())
    }

    pub async fn complete_with_image(
        &self,
        system_prompt: &str,
        user_text: &str,
        image_data_uri: &str,
    ) -> Result<String> {
        let user_content = format!("{user_text}\n\n[IMAGE:{image_data_uri}]");
        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_content),
        ];
        let prepared =
            crate::multimodal::prepare_messages_for_provider(&messages, &self.multimodal).await?;
        let request = ChatRequest {
            messages: &prepared.messages,
            tools: None,
        };
        let response = self
            .provider
            .chat(request, &self.model, self.temperature)
            .await?;
        Ok(response.text.unwrap_or_default())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct VisionModel {
    pub provider: String,
    pub provider_name: String,
    pub model: String,
    pub explicit_vision: bool,
    pub recommended: bool,
}

fn provider_display_name(provider_id: &str, profile: &crate::config::schema::ModelProviderConfig) -> String {
    profile
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| provider_id.to_string())
}

pub fn list_vision_models(config: &Config) -> Vec<VisionModel> {
    let mut out: Vec<VisionModel> = Vec::new();
    let mut seen: std::collections::BTreeSet<(String, String)> = std::collections::BTreeSet::new();

    let recommended = recommended_route(config);

    for (provider_id, profile) in &config.model_providers {
        for model in profile_model_names(profile) {
            let explicit = profile
                .explicit_vision_for_model(&model)
                .unwrap_or(false);
            let heuristic = name_suggests_vision(&model);
            if !(explicit || heuristic) {
                continue;
            }
            let key = (provider_id.clone(), model.clone());
            if !seen.insert(key) {
                continue;
            }
            let is_recommended = recommended
                .as_ref()
                .is_some_and(|(p, m)| p.eq_ignore_ascii_case(provider_id) && m == &model);
            out.push(VisionModel {
                provider: provider_id.clone(),
                provider_name: provider_display_name(provider_id, profile),
                model,
                explicit_vision: explicit,
                recommended: is_recommended,
            });
        }
    }

    out.sort_by(|a, b| {
        b.recommended
            .cmp(&a.recommended)
            .then_with(|| a.provider_name.cmp(&b.provider_name))
            .then_with(|| a.model.cmp(&b.model))
    });
    out
}

fn recommended_route(config: &Config) -> Option<(String, String)> {
    if let (Some(provider), Some(model)) = (
        config.multimodal.vision_provider.as_ref(),
        config.multimodal.vision_model.as_ref(),
    ) {
        if !provider.is_empty() && !model.is_empty() {
            return Some((provider.clone(), model.clone()));
        }
    }
    if let (Some(provider), Some(model)) =
        (config.default_provider.as_ref(), config.default_model.as_ref())
    {
        if config.model_vision_capability(provider, model) == Some(true) {
            return Some((provider.clone(), model.clone()));
        }
    }
    None
}

fn profile_model_names(profile: &ModelProviderConfig) -> Vec<String> {
    if !profile.model_names.is_empty() {
        return profile
            .model_names
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for value in profile.models.values() {
        let trimmed = value.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn name_suggests_vision(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.contains("vision")
        || lower.contains("-vl")
        || lower.contains("vl-")
        || lower.contains("vl2")
        || lower.contains("internvl")
        || lower.contains("multimodal")
        || lower.contains("omni")
        || lower.contains("4o")
        || lower.contains("gemini")
        || lower.contains("claude-3")
        || lower.contains("claude-4")
        || lower.contains("sonnet")
        || lower.contains("opus")
        || lower.contains("pixtral")
        || lower.contains("llava")
        || lower.contains("minicpm-v")
        || lower.contains("minicpm-o")
        || lower.contains("-4v")
        || lower.contains("glm-4v")
        || lower.contains("step-1v")
        || lower.contains("step-1o")
        || lower.contains("qvq")
        || lower.contains("molmo")
        || lower.contains("ui-tars")
        || lower.contains("uitars")
}
