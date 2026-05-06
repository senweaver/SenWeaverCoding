// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Public entrypoint that exposes SenWeaverCoding's tool surface as
//! an embedded MCP **server**.
//!
//! The actual protocol implementation lives in
//! [`crate::services::mcp_server`]; this façade exists so the CLI
//! (`sen mcp serve`) and SDK consumers can share one entry point and
//! one config struct without each rebuilding the tool list.
//!
//! Inverts the direction of [`crate::services::mcp_manager`] /
//! [`crate::tools::mcp_client`], which run as clients calling into
//! external MCP servers.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::security::SecurityPolicy;
use crate::services::mcp_server::McpServer;
use crate::tools::file_read::FileReadTool;
use crate::tools::glob_search::GlobSearchTool;
use crate::tools::traits::Tool;
use crate::tools::web_fetch::WebFetchTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {

    pub transport: McpServerTransport,

    pub cwd: PathBuf,

    pub bind: Option<SocketAddr>,

    pub allowed_tools: Vec<String>,

    pub denied_tools: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpServerTransport {
    Stdio,
    Sse,

    Streamable,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            transport: McpServerTransport::Stdio,
            cwd: std::env::current_dir().unwrap_or_default(),
            bind: None,
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
        }
    }
}

pub struct McpServerEntrypoint;

impl McpServerEntrypoint {

    pub async fn run(
        config: McpServerConfig,
        tools: Vec<Arc<dyn Tool>>,
    ) -> anyhow::Result<()> {
        tracing::info!(
            transport = ?config.transport,
            cwd = %config.cwd.display(),
            "Starting MCP server entrypoint"
        );

        let filtered = filter_tools(tools, &config.allowed_tools, &config.denied_tools);
        let server = McpServer::from_tools(filtered);
        if server.exposed_tool_count() == 0 {
            tracing::warn!(
                target: "mcp.server",
                "MCP server starting with zero exposed tools; clients will see an empty tools/list"
            );
        }

        match config.transport {
            McpServerTransport::Stdio => {
                crate::services::mcp_server::stdio::serve(server).await
            }
            McpServerTransport::Sse | McpServerTransport::Streamable => {
                let bind = config
                    .bind
                    .unwrap_or_else(|| "127.0.0.1:8765".parse().unwrap());
                crate::services::mcp_server::sse::serve(server, bind).await
            }
        }
    }

    pub async fn run_default(config: McpServerConfig) -> anyhow::Result<()> {
        let app_config = Config::load_or_init_sync();
        let tools = default_tool_surface(&app_config);
        Self::run(config, tools).await
    }

    pub fn list_default_tools(config: &McpServerConfig) -> Vec<String> {
        let app_config = Config::load_or_init_sync();
        let tools = default_tool_surface(&app_config);
        tools
            .into_iter()
            .filter(|t| t.mcp_safe())
            .map(|t| t.name().to_string())
            .filter(|name| {
                if config.denied_tools.iter().any(|n| n == name) {
                    return false;
                }
                if !config.allowed_tools.is_empty()
                    && !config.allowed_tools.iter().any(|n| n == name)
                {
                    return false;
                }
                true
            })
            .collect()
    }
}

fn default_tool_surface(app_config: &Config) -> Vec<Arc<dyn Tool>> {
    let security: Arc<SecurityPolicy> = Arc::new(SecurityPolicy::from_config(
        &app_config.autonomy,
        &app_config.workspace_dir,
    ));

    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
    tools.push(Arc::new(FileReadTool::new(security.clone())));
    tools.push(Arc::new(GlobSearchTool::new(security.clone())));
    if app_config.web_fetch.enabled {
        tools.push(Arc::new(WebFetchTool::new(
            security.clone(),
            app_config.web_fetch.allowed_domains.clone(),
            app_config.web_fetch.blocked_domains.clone(),
            app_config.web_fetch.max_response_size,
            app_config.web_fetch.timeout_secs,
            app_config.web_fetch.firecrawl.clone(),
            app_config.web_fetch.allowed_private_hosts.clone(),
        )));
    }
    tools
}

fn filter_tools(
    tools: Vec<Arc<dyn Tool>>,
    allowed_tools: &[String],
    denied_tools: &[String],
) -> Vec<Arc<dyn Tool>> {
    tools
        .into_iter()
        .filter(|t| {
            let name = t.name();
            if denied_tools.iter().any(|n| n == name) {
                return false;
            }
            if !allowed_tools.is_empty()
                && !allowed_tools.iter().any(|n| n == name)
            {
                return false;
            }
            true
        })
        .collect()
}
