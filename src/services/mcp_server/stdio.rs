// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use super::{McpError, McpServer};

const MAX_INFLIGHT_REQUESTS: usize = 256;

pub async fn serve(server: McpServer) -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();

    let (tx, mut rx) = mpsc::channel::<Value>(64);
    let writer_handle = crate::runtime::spawn_supervised("mcp.server.stdio.writer", async move {
        let mut stdout = tokio::io::stdout();
        while let Some(message) = rx.recv().await {
            let line = match serde_json::to_string(&message) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!(
                        target: "mcp.server.stdio",
                        error = %e,
                        "failed to serialise outbound MCP frame; dropping"
                    );
                    continue;
                }
            };
            if let Err(e) = stdout.write_all(line.as_bytes()).await {
                tracing::error!(
                    target: "mcp.server.stdio",
                    error = %e,
                    "stdout write failed; aborting writer"
                );
                break;
            }
            if let Err(e) = stdout.write_all(b"\n").await {
                tracing::error!(
                    target: "mcp.server.stdio",
                    error = %e,
                    "stdout newline write failed; aborting writer"
                );
                break;
            }
            if let Err(e) = stdout.flush().await {
                tracing::error!(
                    target: "mcp.server.stdio",
                    error = %e,
                    "stdout flush failed; aborting writer"
                );
                break;
            }
        }
    });

    let server = Arc::new(server);
    let inflight = Arc::new(tokio::sync::Semaphore::new(MAX_INFLIGHT_REQUESTS));
    tracing::info!(
        target: "mcp.server.stdio",
        tools = server.exposed_tool_count(),
        "MCP stdio transport ready"
    );

    loop {
        let line = match reader.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => {
                tracing::info!(
                    target: "mcp.server.stdio",
                    "stdin closed; shutting down"
                );
                break;
            }
            Err(e) => {
                tracing::error!(
                    target: "mcp.server.stdio",
                    error = %e,
                    "stdin read failed; shutting down"
                );
                break;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let permit = match inflight.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => break,
        };

        let parsed: Result<Value, _> = serde_json::from_str(trimmed);
        let server = server.clone();
        let tx = tx.clone();
        crate::runtime::spawn_supervised("mcp.server.stdio.request", async move {
            let _permit = permit;
            match parsed {
                Ok(req) => {
                    if let Some(resp) = server.dispatch(req).await {
                        let _ = tx.send(resp).await;
                    }
                }
                Err(e) => {
                    let err = McpError::parse_error(format!(
                        "invalid JSON-RPC frame: {e}"
                    ));
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": serde_json::Value::Null,
                        "error": err.to_value(),
                    });
                    let _ = tx.send(resp).await;
                }
            }
        });
    }

    drop(tx);
    let _ = writer_handle.into_inner().await;
    Ok(())
}
