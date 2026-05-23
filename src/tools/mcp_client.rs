// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;
#[cfg(not(target_has_atomic = "64"))]
use std::sync::atomic::AtomicU32;
#[cfg(target_has_atomic = "64")]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::json;
use tokio::sync::Mutex;
use tokio::time::{Duration, timeout};

use crate::config::schema::McpServerConfig;
use crate::tools::mcp_protocol::{
    JsonRpcRequest, ListResourcesResult, MCP_PROTOCOL_VERSION, McpResource, McpResourceContent,
    McpToolDef, McpToolsListResult, ReadResourceResult,
};
use crate::tools::mcp_transport::{McpTransportConn, create_transport};

const RECV_TIMEOUT_SECS: u64 = 30;

const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 180;

const MAX_TOOL_TIMEOUT_SECS: u64 = 600;

struct McpServerInner {
    config: McpServerConfig,
    transport: Box<dyn McpTransportConn>,
    #[cfg(target_has_atomic = "64")]
    next_id: AtomicU64,
    #[cfg(not(target_has_atomic = "64"))]
    next_id: AtomicU32,
    tools: Vec<McpToolDef>,
}

#[derive(Clone)]
pub struct McpServer {
    inner: Arc<Mutex<McpServerInner>>,
}

impl McpServer {

    pub async fn connect(config: McpServerConfig) -> Result<Self> {

        let mut transport = create_transport(&config).with_context(|| {
            format!(
                "failed to create transport for MCP server `{}`",
                config.name
            )
        })?;

        let id = 1u64;
        let init_req = JsonRpcRequest::new(
            id,
            "initialize",
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "sen",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        );

        let init_resp = timeout(
            Duration::from_secs(RECV_TIMEOUT_SECS),
            transport.send_and_recv(&init_req),
        )
        .await
        .with_context(|| {
            format!(
                "MCP server `{}` timed out after {}s waiting for initialize response",
                config.name, RECV_TIMEOUT_SECS
            )
        })??;

        if init_resp.error.is_some() {
            bail!(
                "MCP server `{}` rejected initialize: {:?}",
                config.name,
                init_resp.error
            );
        }

        let notif = JsonRpcRequest::notification("notifications/initialized", json!({}));

        let _ = transport.send_and_recv(&notif).await;

        let id = 2u64;
        let list_req = JsonRpcRequest::new(id, "tools/list", json!({}));

        let list_resp = timeout(
            Duration::from_secs(RECV_TIMEOUT_SECS),
            transport.send_and_recv(&list_req),
        )
        .await
        .with_context(|| {
            format!(
                "MCP server `{}` timed out after {}s waiting for tools/list response",
                config.name, RECV_TIMEOUT_SECS
            )
        })??;

        let result = list_resp
            .result
            .ok_or_else(|| anyhow!("tools/list returned no result from `{}`", config.name))?;
        let tool_list: McpToolsListResult = serde_json::from_value(result)
            .with_context(|| format!("failed to parse tools/list from `{}`", config.name))?;

        let tool_count = tool_list.tools.len();

        let inner = McpServerInner {
            config,
            transport,
            #[cfg(target_has_atomic = "64")]
            next_id: AtomicU64::new(3),
            #[cfg(not(target_has_atomic = "64"))]
            next_id: AtomicU32::new(3),
            tools: tool_list.tools,
        };

        tracing::info!(
            "MCP server `{}` connected — {} tool(s) available",
            inner.config.name,
            tool_count
        );

        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    pub async fn tools(&self) -> Vec<McpToolDef> {
        self.inner.lock().await.tools.clone()
    }

    pub async fn name(&self) -> String {
        self.inner.lock().await.config.name.clone()
    }

    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let mut inner = self.inner.lock().await;
        let id = inner.next_id.fetch_add(1, Ordering::Relaxed) as u64;
        let req = JsonRpcRequest::new(
            id,
            "tools/call",
            json!({ "name": tool_name, "arguments": arguments }),
        );

        let tool_timeout = inner
            .config
            .tool_timeout_secs
            .unwrap_or(DEFAULT_TOOL_TIMEOUT_SECS)
            .min(MAX_TOOL_TIMEOUT_SECS);

        let resp = timeout(
            Duration::from_secs(tool_timeout),
            inner.transport.send_and_recv(&req),
        )
        .await
        .map_err(|_| {
            anyhow!(
                "MCP server `{}` timed out after {}s during tool call `{tool_name}`",
                inner.config.name,
                tool_timeout
            )
        })?
        .with_context(|| {
            format!(
                "MCP server `{}` error during tool call `{tool_name}`",
                inner.config.name
            )
        })?;

        if let Some(err) = resp.error {
            bail!("MCP tool `{tool_name}` error {}: {}", err.code, err.message);
        }
        Ok(resp.result.unwrap_or(serde_json::Value::Null))
    }

    pub async fn list_resources(&self) -> Result<Vec<McpResource>> {
        let mut inner = self.inner.lock().await;
        let id = inner.next_id.fetch_add(1, Ordering::Relaxed) as u64;
        let req = JsonRpcRequest::new(id, "resources/list", json!({}));

        let list_resp = timeout(
            Duration::from_secs(RECV_TIMEOUT_SECS),
            inner.transport.send_and_recv(&req),
        )
        .await
        .with_context(|| {
            format!(
                "MCP server `{}` timed out after {}s waiting for resources/list response",
                inner.config.name, RECV_TIMEOUT_SECS
            )
        })??;

        if let Some(err) = list_resp.error {
            bail!(
                "MCP server `{}` resources/list error {}: {}",
                inner.config.name,
                err.code,
                err.message
            );
        }

        let result = list_resp.result.ok_or_else(|| {
            anyhow!(
                "resources/list returned no result from `{}`",
                inner.config.name
            )
        })?;
        let parsed: ListResourcesResult = serde_json::from_value(result).with_context(|| {
            format!(
                "failed to parse resources/list from `{}`",
                inner.config.name
            )
        })?;
        Ok(parsed.resources)
    }

    pub async fn read_resource(&self, uri: &str) -> Result<Vec<McpResourceContent>> {
        let mut inner = self.inner.lock().await;
        let id = inner.next_id.fetch_add(1, Ordering::Relaxed) as u64;
        let req = JsonRpcRequest::new(id, "resources/read", json!({ "uri": uri }));

        let tool_timeout = inner
            .config
            .tool_timeout_secs
            .unwrap_or(DEFAULT_TOOL_TIMEOUT_SECS)
            .min(MAX_TOOL_TIMEOUT_SECS);

        let resp = timeout(
            Duration::from_secs(tool_timeout),
            inner.transport.send_and_recv(&req),
        )
        .await
        .map_err(|_| {
            anyhow!(
                "MCP server `{}` timed out after {}s during resources/read",
                inner.config.name,
                tool_timeout
            )
        })?
        .with_context(|| {
            format!(
                "MCP server `{}` error during resources/read",
                inner.config.name
            )
        })?;

        if let Some(err) = resp.error {
            bail!(
                "MCP server `{}` resources/read error {}: {}",
                inner.config.name,
                err.code,
                err.message
            );
        }

        let result = resp.result.ok_or_else(|| {
            anyhow!(
                "resources/read returned no result from `{}`",
                inner.config.name
            )
        })?;
        let parsed: ReadResourceResult = serde_json::from_value(result).with_context(|| {
            format!(
                "failed to parse resources/read from `{}`",
                inner.config.name
            )
        })?;
        Ok(parsed.contents)
    }
}

pub struct McpRegistry {
    servers: Vec<McpServer>,

    tool_index: HashMap<String, (usize, String)>,
}

impl McpRegistry {
    pub fn empty() -> Self {
        Self {
            servers: Vec::new(),
            tool_index: HashMap::new(),
        }
    }

    pub async fn connect_all(configs: &[McpServerConfig]) -> Result<Self> {
        let enabled: Vec<&McpServerConfig> = configs
            .iter()
            .filter(|c| {
                if !c.enabled {
                    tracing::info!(
                        server = %c.name,
                        "MCP server marked disabled in config; skipping connect_all"
                    );
                }
                c.enabled
            })
            .collect();

        if enabled.is_empty() {
            return Ok(Self::empty());
        }

        let started = std::time::Instant::now();
        let attempts: Vec<_> = enabled
            .iter()
            .map(|cfg| {
                let cfg = (*cfg).clone();
                let name = cfg.name.clone();
                async move {
                    let started_one = std::time::Instant::now();
                    let outcome = McpServer::connect(cfg).await;
                    (name, started_one.elapsed(), outcome)
                }
            })
            .collect();

        let results = futures_util::future::join_all(attempts).await;

        let mut servers = Vec::with_capacity(results.len());
        let mut tool_index = HashMap::new();
        for (name, elapsed, outcome) in results {
            match outcome {
                Ok(server) => {
                    let server_idx = servers.len();
                    let tools = server.tools().await;
                    for tool in &tools {
                        let prefixed = format!("{}__{}", name, tool.name);
                        tool_index.insert(prefixed, (server_idx, tool.name.clone()));
                    }
                    tracing::info!(
                        server = %name,
                        tool_count = tools.len(),
                        elapsed_ms = elapsed.as_millis() as u64,
                        "MCP server connected"
                    );
                    servers.push(server);
                }
                Err(e) => {
                    tracing::error!(
                        server = %name,
                        elapsed_ms = elapsed.as_millis() as u64,
                        "Failed to connect to MCP server `{name}`: {e:#}"
                    );
                }
            }
        }

        tracing::info!(
            total_elapsed_ms = started.elapsed().as_millis() as u64,
            connected = servers.len(),
            attempted = enabled.len(),
            "MCP connect_all completed"
        );

        Ok(Self {
            servers,
            tool_index,
        })
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.tool_index.keys().cloned().collect()
    }

    pub async fn get_tool_def(&self, prefixed_name: &str) -> Option<McpToolDef> {
        let (server_idx, original_name) = self.tool_index.get(prefixed_name)?;
        let inner = self.servers[*server_idx].inner.lock().await;
        inner
            .tools
            .iter()
            .find(|t| &t.name == original_name)
            .cloned()
    }

    pub async fn call_tool(
        &self,
        prefixed_name: &str,
        arguments: serde_json::Value,
    ) -> Result<String> {
        let (server_idx, original_name) = self
            .tool_index
            .get(prefixed_name)
            .ok_or_else(|| anyhow!("unknown MCP tool `{prefixed_name}`"))?;
        let result = self.servers[*server_idx]
            .call_tool(original_name, arguments)
            .await?;
        serde_json::to_string_pretty(&result)
            .with_context(|| format!("failed to serialize result of MCP tool `{prefixed_name}`"))
    }

    pub async fn list_resources(&self, server_name: Option<&str>) -> Result<Vec<McpResource>> {
        match server_name {
            None => {
                let mut all = Vec::new();
                for server in &self.servers {
                    match server.list_resources().await {
                        Ok(mut resources) => all.append(&mut resources),
                        Err(e) => {
                            tracing::warn!(
                                "resources/list failed for MCP server `{}`: {:#}",
                                server.name().await,
                                e
                            );
                        }
                    }
                }
                Ok(all)
            }
            Some(name) => {
                for server in &self.servers {
                    if server.name().await == name {
                        return server.list_resources().await;
                    }
                }
                bail!("unknown MCP server `{name}`")
            }
        }
    }

    pub async fn read_resource(
        &self,
        server_name: &str,
        uri: &str,
    ) -> Result<Vec<McpResourceContent>> {
        for server in &self.servers {
            if server.name().await == server_name {
                return server.read_resource(uri).await;
            }
        }
        bail!("unknown MCP server `{server_name}`")
    }

    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    pub fn server_count(&self) -> usize {
        self.servers.len()
    }

    pub fn tool_count(&self) -> usize {
        self.tool_index.len()
    }
}
