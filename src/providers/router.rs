// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::Provider;
use super::traits::{
    ChatMessage, ChatRequest, ChatResponse, StreamChunk, StreamError, StreamEvent, StreamOptions,
    StreamResult,
};
use crate::config::schema::ModelPricing;
use async_trait::async_trait;
use futures_util::stream::{self, BoxStream, StreamExt};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Route {
    pub provider_name: String,
    pub model: String,
}

pub struct RouterProvider {
    routes: HashMap<String, (usize, String)>,
    providers: Vec<(String, Box<dyn Provider>)>,
    default_index: usize,
    default_model: String,
    prices: HashMap<String, ModelPricing>,
}

impl RouterProvider {

    pub fn new(
        providers: Vec<(String, Box<dyn Provider>)>,
        routes: Vec<(String, Route)>,
        default_model: String,
    ) -> Self {

        let name_to_index: HashMap<&str, usize> = providers
            .iter()
            .enumerate()
            .map(|(i, (name, _))| (name.as_str(), i))
            .collect();

        let resolved_routes: HashMap<String, (usize, String)> = routes
            .into_iter()
            .filter_map(|(hint, route)| {
                let index = name_to_index.get(route.provider_name.as_str()).copied();
                match index {
                    Some(i) => Some((hint, (i, route.model))),
                    None => {
                        tracing::warn!(
                            hint = hint,
                            provider = route.provider_name,
                            "Route references unknown provider, skipping"
                        );
                        None
                    }
                }
            })
            .collect();

        Self {
            routes: resolved_routes,
            providers,
            default_index: 0,
            default_model,
            prices: HashMap::new(),
        }
    }

    pub fn with_prices(mut self, prices: HashMap<String, ModelPricing>) -> Self {
        self.prices = prices;
        self
    }

    pub fn resolve_cost_optimized(
        &self,
        model: &str,
        prices: &HashMap<String, ModelPricing>,
        required_vision: bool,
        required_tools: bool,
    ) -> anyhow::Result<(usize, String)> {
        let hint = model.strip_prefix("route:").or_else(|| {
            model.strip_prefix("hint:").inspect(|_| {
                tracing::warn!(
                    deprecated = "hint:",
                    replacement = "route:",
                    "model name uses deprecated `hint:` prefix; switch to `route:` (hint: still accepted for now)"
                );
            })
        });
        let is_cost_hint = matches!(hint, Some("cost-optimized" | "cheapest"));

        if !is_cost_hint {
            return self.resolve(model);
        }

        let mut candidates: Vec<(usize, String, f64)> = Vec::new();

        for (idx, route_model) in self.routes.values() {

            if let Some((_, provider)) = self.providers.get(*idx) {
                if required_vision && !provider.supports_vision() {
                    continue;
                }
                if required_tools && !provider.supports_native_tools() {
                    continue;
                }
            }

            if let Some(pricing) = prices.get(route_model) {
                let total_cost = pricing.input + pricing.output;
                candidates.push((*idx, route_model.clone(), total_cost));
            }
        }

        candidates.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

        if let Some((idx, route_model, _)) = candidates.into_iter().next() {
            return Ok((idx, route_model));
        }

        tracing::warn!(
            "No cost-optimized route found with matching pricing data, \
             falling back to default resolution"
        );
        self.resolve(model)
    }

    fn resolve_auto(&self, model: &str) -> anyhow::Result<(usize, String)> {
        let is_cost_prefix = model.starts_with("route:cost")
            || model.starts_with("route:cheap")
            || model.starts_with("hint:cost")
            || model.starts_with("hint:cheap");
        if is_cost_prefix && !self.prices.is_empty() {
            return self.resolve_cost_optimized(model, &self.prices, false, false);
        }
        self.resolve(model)
    }

    fn resolve(&self, model: &str) -> anyhow::Result<(usize, String)> {
        let prefixed = model.strip_prefix("route:").or_else(|| {
            model.strip_prefix("hint:").inspect(|_| {
                tracing::warn!(
                    deprecated = "hint:",
                    replacement = "route:",
                    "model name uses deprecated `hint:` prefix; switch to `route:` (hint: still accepted for now)"
                );
            })
        });
        if let Some(hint) = prefixed {
            if let Some((idx, resolved_model)) = self.routes.get(hint) {
                return Ok((*idx, resolved_model.clone()));
            }
            tracing::warn!(
                hint = hint,
                "Unknown route hint, falling back to default provider"
            );
            if !self.default_model.trim().is_empty() {
                return Ok((self.default_index, self.default_model.clone()));
            }
            if let Some((fallback_hint, (idx, route_model))) = self
                .routes
                .iter()
                .min_by(|a, b| a.0.cmp(b.0))
            {
                tracing::warn!(
                    hint = hint,
                    fallback_route = fallback_hint.as_str(),
                    fallback_model = route_model.as_str(),
                    "default_model is empty, falling back to first configured route"
                );
                return Ok((*idx, route_model.clone()));
            }
            anyhow::bail!(
                "Router cannot resolve route hint '{hint}': no matching route, \
                 empty default_model, and no configured routes are available"
            );
        }

        Ok((self.default_index, model.to_string()))
    }
}

#[derive(Debug, Clone)]
pub struct CostOptimizedStrategy {

    pub prices: HashMap<String, ModelPricing>,

    pub required_vision: bool,

    pub required_tools: bool,
}

impl CostOptimizedStrategy {

    pub fn new(prices: HashMap<String, ModelPricing>) -> Self {
        Self {
            prices,
            required_vision: false,
            required_tools: false,
        }
    }

    pub fn with_vision(mut self, required: bool) -> Self {
        self.required_vision = required;
        self
    }

    pub fn with_tools(mut self, required: bool) -> Self {
        self.required_tools = required;
        self
    }

    pub fn score(&self, model: &str) -> Option<f64> {
        self.prices.get(model).map(|p| p.input + p.output)
    }
}

#[async_trait]
impl Provider for RouterProvider {
    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let (provider_idx, resolved_model) = self.resolve_auto(model)?;

        let (provider_name, provider) = &self.providers[provider_idx];
        tracing::info!(
            provider = provider_name.as_str(),
            model = resolved_model.as_str(),
            "Router dispatching request"
        );

        provider
            .chat_with_system(system_prompt, message, &resolved_model, temperature)
            .await
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let (provider_idx, resolved_model) = self.resolve_auto(model)?;
        let (_, provider) = &self.providers[provider_idx];
        provider
            .chat_with_history(messages, &resolved_model, temperature)
            .await
    }

    async fn chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let (provider_idx, resolved_model) = self.resolve_auto(model)?;
        let (_, provider) = &self.providers[provider_idx];
        provider.chat(request, &resolved_model, temperature).await
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let (provider_idx, resolved_model) = self.resolve_auto(model)?;
        let (_, provider) = &self.providers[provider_idx];
        provider
            .chat_with_tools(messages, tools, &resolved_model, temperature)
            .await
    }

    fn supports_native_tools(&self) -> bool {
        self.providers
            .iter()
            .any(|(_, provider)| provider.supports_native_tools())
    }

    fn supports_streaming(&self) -> bool {
        self.providers
            .iter()
            .any(|(_, provider)| provider.supports_streaming())
    }

    fn supports_streaming_tool_events(&self) -> bool {
        self.providers
            .iter()
            .any(|(_, provider)| provider.supports_streaming_tool_events())
    }

    fn stream_chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> BoxStream<'static, StreamResult<StreamChunk>> {
        let (provider_idx, resolved_model) = match self.resolve_auto(model) {
            Ok(resolved) => resolved,
            Err(error) => {
                let detail = error.to_string();
                return stream::once(async move { Err(StreamError::Provider(detail)) }).boxed();
            }
        };
        let (provider_name, provider) = &self.providers[provider_idx];
        if !provider.supports_streaming() {
            let detail = format!(
                "routed provider '{provider_name}' does not support streaming for model '{resolved_model}'"
            );
            return stream::once(async move { Err(StreamError::Provider(detail)) }).boxed();
        }
        provider.stream_chat_with_system(
            system_prompt,
            message,
            &resolved_model,
            temperature,
            options,
        )
    }

    fn stream_chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> BoxStream<'static, StreamResult<StreamChunk>> {
        let (provider_idx, resolved_model) = match self.resolve_auto(model) {
            Ok(resolved) => resolved,
            Err(error) => {
                let message = error.to_string();
                return stream::once(async move { Err(StreamError::Provider(message)) }).boxed();
            }
        };
        let (provider_name, provider) = &self.providers[provider_idx];
        if !provider.supports_streaming() {
            let detail = format!(
                "routed provider '{provider_name}' does not support streaming for model '{resolved_model}'"
            );
            return stream::once(async move { Err(StreamError::Provider(detail)) }).boxed();
        }
        provider.stream_chat_with_history(messages, &resolved_model, temperature, options)
    }

    fn stream_chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
        options: StreamOptions,
    ) -> BoxStream<'static, StreamResult<StreamEvent>> {
        let (provider_idx, resolved_model) = match self.resolve_auto(model) {
            Ok(resolved) => resolved,
            Err(error) => {
                let message = error.to_string();
                return stream::once(async move { Err(StreamError::Provider(message)) }).boxed();
            }
        };
        let (provider_name, provider) = &self.providers[provider_idx];
        if !provider.supports_streaming() {
            let detail = format!(
                "routed provider '{provider_name}' does not support streaming for model '{resolved_model}'"
            );
            return stream::once(async move { Err(StreamError::Provider(detail)) }).boxed();
        }
        provider.stream_chat(request, &resolved_model, temperature, options)
    }

    fn supports_vision(&self) -> bool {
        self.providers
            .iter()
            .any(|(_, provider)| provider.supports_vision())
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        for (name, provider) in &self.providers {
            tracing::info!(provider = name, "Warming up routed provider");
            if let Err(e) = provider.warmup().await {
                tracing::warn!(provider = name, "Warmup failed (non-fatal): {e}");
            }
        }
        Ok(())
    }
}
