// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// MCP manager service — mirrors claude-code-typescript-src`services/mcp/`.
// Manages Model Context Protocol server connections, tool discovery,
// resource listing, and approval workflows.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types (mirrors services/mcp/types.ts)
// ---------------------------------------------------------------------------

/// Status of an MCP server connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpServerStatus {
    Disconnected,
    Connecting,
    Connected,
    Error,
    Disabled,
}

/// An MCP server connection entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConnection {
    pub name: String,
    pub transport: McpTransport,
    pub status: McpServerStatus,
    pub tools: Vec<McpToolDef>,
    pub resources: Vec<McpResource>,
    pub error: Option<String>,
    pub enabled: bool,
}

/// Transport configuration for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    Sse {
        url: String,
        headers: HashMap<String, String>,
    },
    Streamable {
        url: String,
        headers: HashMap<String, String>,
    },
}

/// An MCP tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
    pub server_name: String,
}

/// An MCP resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
    pub server_name: String,
}

/// Approval state for an MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpApprovalState {
    Pending,
    Approved,
    Denied,
    AlwaysAllow,
}

// ---------------------------------------------------------------------------
// MCP Manager
// ---------------------------------------------------------------------------

/// Central manager for all MCP server connections.
#[derive(Clone)]
pub struct McpManager {
    inner: Arc<RwLock<McpManagerInner>>,
}

struct McpManagerInner {
    servers: HashMap<String, McpServerConnection>,
    approval_states: HashMap<String, McpApprovalState>,
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(McpManagerInner {
                servers: HashMap::new(),
                approval_states: HashMap::new(),
            })),
        }
    }

    /// Register a new MCP server.
    pub async fn add_server(&self, name: &str, transport: McpTransport) {
        let mut inner = self.inner.write().await;
        inner.servers.insert(
            name.to_string(),
            McpServerConnection {
                name: name.to_string(),
                transport,
                status: McpServerStatus::Disconnected,
                tools: Vec::new(),
                resources: Vec::new(),
                error: None,
                enabled: true,
            },
        );
    }

    /// Remove an MCP server.
    pub async fn remove_server(&self, name: &str) -> bool {
        let mut inner = self.inner.write().await;
        inner.servers.remove(name).is_some()
    }

    /// Get a server's connection info.
    pub async fn get_server(&self, name: &str) -> Option<McpServerConnection> {
        let inner = self.inner.read().await;
        inner.servers.get(name).cloned()
    }

    /// List all servers.
    pub async fn list_servers(&self) -> Vec<McpServerConnection> {
        let inner = self.inner.read().await;
        inner.servers.values().cloned().collect()
    }

    /// Get all tools from all connected servers.
    pub async fn all_tools(&self) -> Vec<McpToolDef> {
        let inner = self.inner.read().await;
        inner
            .servers
            .values()
            .filter(|s| s.status == McpServerStatus::Connected && s.enabled)
            .flat_map(|s| s.tools.clone())
            .collect()
    }

    /// Get all resources from all connected servers.
    pub async fn all_resources(&self) -> Vec<McpResource> {
        let inner = self.inner.read().await;
        inner
            .servers
            .values()
            .filter(|s| s.status == McpServerStatus::Connected && s.enabled)
            .flat_map(|s| s.resources.clone())
            .collect()
    }

    /// Update a server's status.
    pub async fn set_server_status(
        &self,
        name: &str,
        status: McpServerStatus,
        error: Option<String>,
    ) {
        let mut inner = self.inner.write().await;
        if let Some(server) = inner.servers.get_mut(name) {
            server.status = status;
            server.error = error;
        }
    }

    /// Update a server's discovered tools.
    pub async fn set_server_tools(&self, name: &str, tools: Vec<McpToolDef>) {
        let mut inner = self.inner.write().await;
        if let Some(server) = inner.servers.get_mut(name) {
            server.tools = tools;
        }
    }

    /// Update a server's discovered resources.
    pub async fn set_server_resources(&self, name: &str, resources: Vec<McpResource>) {
        let mut inner = self.inner.write().await;
        if let Some(server) = inner.servers.get_mut(name) {
            server.resources = resources;
        }
    }

    /// Get approval state for a server.
    pub async fn approval_state(&self, name: &str) -> McpApprovalState {
        let inner = self.inner.read().await;
        inner
            .approval_states
            .get(name)
            .copied()
            .unwrap_or(McpApprovalState::Pending)
    }

    /// Set approval state for a server.
    pub async fn set_approval_state(&self, name: &str, state: McpApprovalState) {
        let mut inner = self.inner.write().await;
        inner.approval_states.insert(name.to_string(), state);
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}
