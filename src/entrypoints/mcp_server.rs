// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::security::SecurityPolicy;
use crate::services::mcp_server::McpServer;
use crate::tools::file::read::FileReadTool;
use crate::tools::glob::search::GlobSearchTool;
use crate::tools::traits::Tool;
use crate::tools::web::fetch::WebFetchTool;

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
                let bind = config.bind.unwrap_or_else(|| {
                    SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 8765))
                });
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
