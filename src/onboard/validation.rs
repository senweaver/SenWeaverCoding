// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Result, bail};

pub fn validate_config(config: &crate::config::Config) -> Result<()> {

    if let Some(key) = &config.api_key {
        if key.is_empty() {
            bail!("API key cannot be empty when provider is set");
        }
        validate_provider_key_compatibility(key, config.default_provider.as_deref())?;
    }

    validate_provider_config(config)?;

    if let Some(ref url) = config.api_url {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            bail!("api_url must start with 'http://' or 'https://': {}", url);
        }
    }

    if config.provider_timeout_secs == 0 {
        bail!("provider_timeout_secs cannot be zero");
    }

    Ok(())
}

fn validate_provider_key_compatibility(key: &str, provider: Option<&str>) -> Result<()> {
    match provider {
        Some("anthropic") if !key.starts_with("sk-ant-") => {
            bail!("Anthropic API key must start with 'sk-ant-'");
        }
        Some("openai")
            if !key.starts_with("sk-") && !key.starts_with("o1-") && !key.starts_with("gpt-") =>
        {
            bail!("OpenAI API key must start with 'sk-', 'o1-', or 'gpt-'");
        }
        Some(provider_name) => {

            if key.len() < 10 {
                bail!("API key for {} appears too short", provider_name);
            }
        }
        None => {

            tracing::warn!("API key provided but no default_provider set");
        }
    }
    Ok(())
}

fn validate_provider_config(config: &crate::config::Config) -> Result<()> {

    if config.default_provider.is_some() && config.default_model.is_none() {
        bail!("default_provider is set but default_model is not specified");
    }

    let temp = config.default_temperature;
    if !(0.0..=2.0).contains(&temp) {
        bail!(
            "default_temperature must be between 0.0 and 2.0, got {}",
            temp
        );
    }

    Ok(())
}
