// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use std::path::Path;

pub async fn list_agents(workspace: &Path) -> Result<Vec<AgentInfo>> {
    let config_path = workspace.join("config.toml");
    if !config_path.exists() {
        return Ok(Vec::new());
    }

    let content = tokio::fs::read_to_string(&config_path).await?;
    let config: toml::Value = toml::from_str(&content)?;

    let mut agents = Vec::new();
    if let Some(agents_table) = config.get("agents").and_then(|a| a.as_table()) {
        for (name, value) in agents_table {
            let provider = value
                .get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let model = value
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("default");
            agents.push(AgentInfo {
                name: name.clone(),
                provider: provider.to_string(),
                model: model.to_string(),
                source: AgentSource::Config,
            });
        }
    }

    Ok(agents)
}

#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub source: AgentSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSource {
    Config,
    Skill,
    Plugin,
}

impl std::fmt::Display for AgentSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config => write!(f, "config"),
            Self::Skill => write!(f, "skill"),
            Self::Plugin => write!(f, "plugin"),
        }
    }
}

pub fn print_agents(agents: &[AgentInfo]) {
    if agents.is_empty() {
        println!("No agents configured.");
        return;
    }

    println!(
        "{:<20} {:<15} {:<25} {}",
        "NAME", "PROVIDER", "MODEL", "SOURCE"
    );
    println!("{}", "-".repeat(70));
    for a in agents {
        println!(
            "{:<20} {:<15} {:<25} {}",
            a.name, a.provider, a.model, a.source
        );
    }
}
