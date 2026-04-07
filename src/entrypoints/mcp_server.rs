// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// MCP server entrypoint — mirrors claude-code-typescript-src`entrypoints/mcp.ts`.
// Runs SenWeaverCoding as an MCP server, exposing its tools and resources
// to other MCP clients.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Transport mode: "stdio" or "sse".
    pub transport: McpServerTransport,
    /// Working directory for tool execution.
    pub cwd: PathBuf,
    /// Model to use for agent-powered tools.
    pub model: Option<String>,
    /// Tool allow-list (empty = expose all).
    pub allowed_tools: Vec<String>,
    /// Tool deny-list.
    pub denied_tools: Vec<String>,
    /// Whether to expose memory as MCP resources.
    pub expose_memory: bool,
    /// Whether to expose skills as MCP prompts.
    pub expose_skills: bool,
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
            model: None,
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
            expose_memory: true,
            expose_skills: true,
        }
    }
}

/// MCP server entrypoint.
pub struct McpServerEntrypoint;

impl McpServerEntrypoint {
    /// Run as an MCP server.
    pub async fn run(config: McpServerConfig) -> anyhow::Result<()> {
        tracing::info!(
            transport = ?config.transport,
            cwd = %config.cwd.display(),
            "Starting MCP server entrypoint"
        );

        // The MCP server would:
        // 1. Register all enabled tools as MCP tool definitions
        // 2. Register memory entries as MCP resources
        // 3. Register skills as MCP prompts
        // 4. Start the appropriate transport (stdio/SSE)
        // 5. Handle incoming MCP requests
        //
        // This integrates with the existing tools/mcp_*.rs infrastructure
        // but inverts the direction: instead of being a client TO MCP servers,
        // SenWeaverCoding becomes an MCP server itself.

        match config.transport {
            McpServerTransport::Stdio => {
                tracing::info!("MCP server running on stdio");
                // Read JSON-RPC from stdin, write to stdout
            }
            McpServerTransport::Sse => {
                tracing::info!("MCP server running via SSE");
                // Start HTTP server with SSE endpoint
            }
            McpServerTransport::Streamable => {
                tracing::info!("MCP server running via streamable HTTP");
            }
        }

        Ok(())
    }
}
