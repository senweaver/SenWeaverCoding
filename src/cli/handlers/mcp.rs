// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! MCP server management handler — add/remove/list/auth MCP connections.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// MCP server configuration entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    pub name: String,
    pub transport: McpTransportType,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: std::collections::HashMap<String, String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTransportType {
    Stdio,
    Sse,
    StreamableHttp,
}

impl std::fmt::Display for McpTransportType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stdio => write!(f, "stdio"),
            Self::Sse => write!(f, "sse"),
            Self::StreamableHttp => write!(f, "http"),
        }
    }
}

/// List configured MCP servers.
pub async fn list_servers(workspace: &Path) -> Result<Vec<McpServerEntry>> {
    let config_path = workspace.join(".senweavercoding").join("mcp_servers.json");
    if !config_path.exists() {
        return Ok(Vec::new());
    }
    let content = tokio::fs::read_to_string(&config_path).await?;
    let servers: Vec<McpServerEntry> = serde_json::from_str(&content)?;
    Ok(servers)
}

/// Add an MCP server configuration.
pub async fn add_server(workspace: &Path, entry: McpServerEntry) -> Result<()> {
    let config_path = workspace.join(".senweavercoding").join("mcp_servers.json");
    let mut servers = if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path).await?;
        serde_json::from_str::<Vec<McpServerEntry>>(&content).unwrap_or_default()
    } else {
        Vec::new()
    };

    if servers.iter().any(|s| s.name == entry.name) {
        anyhow::bail!("MCP server '{}' already exists", entry.name);
    }

    servers.push(entry.clone());
    let dir = config_path.parent().unwrap();
    tokio::fs::create_dir_all(dir).await?;
    tokio::fs::write(&config_path, serde_json::to_string_pretty(&servers)?).await?;
    println!("Added MCP server '{}'", entry.name);
    Ok(())
}

/// Remove an MCP server by name.
pub async fn remove_server(workspace: &Path, name: &str) -> Result<()> {
    let config_path = workspace.join(".senweavercoding").join("mcp_servers.json");
    if !config_path.exists() {
        anyhow::bail!("No MCP servers configured");
    }
    let content = tokio::fs::read_to_string(&config_path).await?;
    let mut servers: Vec<McpServerEntry> = serde_json::from_str(&content)?;
    let before = servers.len();
    servers.retain(|s| s.name != name);
    if servers.len() == before {
        anyhow::bail!("MCP server '{}' not found", name);
    }
    tokio::fs::write(&config_path, serde_json::to_string_pretty(&servers)?).await?;
    println!("Removed MCP server '{}'", name);
    Ok(())
}

/// Print MCP servers in a table format.
pub fn print_servers(servers: &[McpServerEntry]) {
    if servers.is_empty() {
        println!("No MCP servers configured.");
        return;
    }

    println!(
        "{:<20} {:<10} {:<8} {}",
        "NAME", "TRANSPORT", "ENABLED", "URL/COMMAND"
    );
    println!("{}", "-".repeat(70));
    for s in servers {
        let target = s.url.as_deref().or(s.command.as_deref()).unwrap_or("-");
        println!(
            "{:<20} {:<10} {:<8} {}",
            s.name,
            s.transport,
            if s.enabled { "yes" } else { "no" },
            target
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_display() {
        assert_eq!(McpTransportType::Stdio.to_string(), "stdio");
        assert_eq!(McpTransportType::Sse.to_string(), "sse");
    }

    #[test]
    fn server_entry_serde() {
        let entry = McpServerEntry {
            name: "test".into(),
            transport: McpTransportType::Stdio,
            url: None,
            command: Some("node".into()),
            args: vec!["server.js".into()],
            env: std::collections::HashMap::new(),
            enabled: true,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: McpServerEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "test");
        assert_eq!(parsed.transport, McpTransportType::Stdio);
    }
}
