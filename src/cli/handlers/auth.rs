// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone)]
pub enum AuthMethod {
    ApiKey(String),
    OAuth {
        token: String,
        refresh_token: Option<String>,
    },
}

pub async fn prompt_api_key(provider: &str) -> Result<String> {
    println!("Enter your {} API key:", provider);
    let key = dialoguer::Password::new()
        .with_prompt("API Key")
        .interact()?;

    if key.trim().is_empty() {
        anyhow::bail!("API key cannot be empty");
    }

    Ok(key)
}

pub async fn save_api_key(workspace: &Path, provider: &str, key: &str) -> Result<()> {
    let config_dir = workspace.join(".senweavercoding");
    tokio::fs::create_dir_all(&config_dir).await?;

    let creds_path = config_dir.join("credentials.json");
    let mut creds: serde_json::Value = if creds_path.exists() {
        let content = tokio::fs::read_to_string(&creds_path).await?;
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    creds[provider] = serde_json::json!({
        "api_key": key,
        "saved_at": chrono::Utc::now().to_rfc3339(),
    });

    tokio::fs::write(&creds_path, serde_json::to_string_pretty(&creds)?).await?;
    println!("API key saved for provider '{}'", provider);

    Ok(())
}

pub async fn has_credentials(workspace: &Path, provider: &str) -> bool {
    let creds_path = workspace.join(".senweavercoding").join("credentials.json");
    if !creds_path.exists() {
        return false;
    }
    match tokio::fs::read_to_string(&creds_path).await {
        Ok(content) => serde_json::from_str::<serde_json::Value>(&content)
            .ok()
            .and_then(|v| v.get(provider).cloned())
            .is_some(),
        Err(_) => false,
    }
}

pub async fn list_providers(workspace: &Path) -> Result<Vec<String>> {
    let creds_path = workspace.join(".senweavercoding").join("credentials.json");
    if !creds_path.exists() {
        return Ok(Vec::new());
    }
    let content = tokio::fs::read_to_string(&creds_path).await?;
    let creds: serde_json::Value = serde_json::from_str(&content)?;
    let providers = creds
        .as_object()
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default();
    Ok(providers)
}
