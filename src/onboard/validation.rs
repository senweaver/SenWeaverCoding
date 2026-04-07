// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

//! Configuration validation module.
//!
//! Provides runtime validation for Config to ensure all fields are valid
//! and cross-field constraints are satisfied.

use anyhow::{Result, bail};

/// Validate a complete configuration.
/// Returns Ok if valid, or an error describing the first validation failure.
pub fn validate_config(config: &crate::config::Config) -> Result<()> {
    // Validate API key format if provided
    if let Some(key) = &config.api_key {
        if key.is_empty() {
            bail!("API key cannot be empty when provider is set");
        }
        validate_provider_key_compatibility(key, config.default_provider.as_deref())?;
    }

    // Validate provider configuration
    validate_provider_config(config)?;

    // Validate API URL if provided
    if let Some(ref url) = config.api_url {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            bail!("api_url must start with 'http://' or 'https://': {}", url);
        }
    }

    // Validate timeout
    if config.provider_timeout_secs == 0 {
        bail!("provider_timeout_secs cannot be zero");
    }

    Ok(())
}

/// Validate that API key format matches the provider.
fn validate_provider_key_compatibility(key: &str, provider: Option<&str>) -> Result<()> {
    match provider {
        Some("anthropic") if !key.starts_with("sk-ant-") => {
            bail!("Anthropic API key must start with 'sk-ant-'");
        }
        Some("openai") if !key.starts_with("sk-") && !key.starts_with("o1-") && !key.starts_with("gpt-") => {
            bail!("OpenAI API key must start with 'sk-', 'o1-', or 'gpt-'");
        }
        Some(provider_name) => {
            // For other providers, just check basic validity
            if key.len() < 10 {
                bail!("API key for {} appears too short", provider_name);
            }
        }
        None => {
            // No provider specified - accept the key but warn
            tracing::warn!("API key provided but no default_provider set");
        }
    }
    Ok(())
}

/// Validate provider configuration consistency.
fn validate_provider_config(config: &crate::config::Config) -> Result<()> {
    // Check that default_model is set if default_provider is set
    if config.default_provider.is_some() && config.default_model.is_none() {
        bail!("default_provider is set but default_model is not specified");
    }

    // Validate temperature range (0.0 to 2.0)
    let temp = config.default_temperature;
    if temp < 0.0 || temp > 2.0 {
        bail!("default_temperature must be between 0.0 and 2.0, got {}", temp);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_empty_api_key() {
        let mut config = crate::config::Config::default();
        config.api_key = Some(String::new());
        config.default_provider = Some("anthropic".to_string());

        let result = validate_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("API key cannot be empty"));
    }

    #[test]
    fn test_validate_anthropic_key_format() {
        let mut config = crate::config::Config::default();
        config.api_key = Some("invalid-key".to_string());
        config.default_provider = Some("anthropic".to_string());

        let result = validate_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("must start with 'sk-ant-'"));
    }

    #[test]
    fn test_validate_openai_key_format() {
        let mut config = crate::config::Config::default();
        config.api_key = Some("invalid-key".to_string());
        config.default_provider = Some("openai".to_string());

        let result = validate_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("must start with 'sk-'"));
    }

    #[test]
    fn test_validate_valid_anthropic_key() {
        let mut config = crate::config::Config::default();
        config.api_key = Some("sk-ant-api03-xxxx".to_string());
        config.default_provider = Some("anthropic".to_string());
        config.default_model = Some("claude-3-5-sonnet-20241022".to_string());

        let result = validate_config(&config);
        assert!(result.is_ok(), "Valid config should pass: {:?}", result);
    }

    #[test]
    fn test_validate_temperature_range() {
        let mut config = crate::config::Config::default();
        config.api_key = Some("sk-ant-api03-xxxx".to_string());
        config.default_provider = Some("anthropic".to_string());
        config.default_model = Some("claude-3-5-sonnet-20241022".to_string());
        config.default_temperature = 2.5;

        let result = validate_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("default_temperature"));
    }

    #[test]
    fn test_validate_timeout_zero() {
        let mut config = crate::config::Config::default();
        config.api_key = Some("sk-ant-api03-xxxx".to_string());
        config.default_provider = Some("anthropic".to_string());
        config.default_model = Some("claude-3-5-sonnet-20241022".to_string());
        config.provider_timeout_secs = 0;

        let result = validate_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("provider_timeout_secs cannot be zero"));
    }

    #[test]
    fn test_validate_api_url_format() {
        let mut config = crate::config::Config::default();
        config.api_key = Some("sk-ant-api03-xxxx".to_string());
        config.default_provider = Some("anthropic".to_string());
        config.default_model = Some("claude-3-5-sonnet-20241022".to_string());
        config.api_url = Some("invalid-url".to_string());

        let result = validate_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("api_url must start with"));
    }

    #[test]
    fn test_validate_provider_without_model() {
        let mut config = crate::config::Config::default();
        // Reset to have no model set
        config.default_model = None;

        let result = validate_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("default_model is not specified"));
    }
}
