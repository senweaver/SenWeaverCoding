// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::channels::traits::{Channel, ChannelMessage, SendMessage};
use crate::plugins::wasm::tool::{invoke_wasm_export, load_plugin};
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct WasmChannel {
    name: String,
    plugin_name: String,
    wasm_path: PathBuf,
}

impl WasmChannel {
    pub fn new(name: String, plugin_name: String, wasm_path: PathBuf) -> Self {
        Self {
            name,
            plugin_name,
            wasm_path,
        }
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[async_trait]
impl Channel for WasmChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        let wasm_path = self.wasm_path.display().to_string();
        let plugin_name = self.plugin_name.clone();
        let payload = serde_json::json!({
            "op": "send",
            "plugin": plugin_name,
            "channel": self.name,
            "content": message.content,
            "recipient": message.recipient,
            "subject": message.subject,
            "thread_ts": message.thread_ts,
        });
        let input = serde_json::to_vec(&payload)?;
        let output = tokio::task::spawn_blocking(move || {
            invoke_wasm_export(&wasm_path, "channel_send", &input)
                .or_else(|_| invoke_wasm_export(&wasm_path, "send", &input))
                .or_else(|_| invoke_wasm_export(&wasm_path, "call", &input))
        })
        .await
        .map_err(|e| anyhow::anyhow!("WasmChannel send join error: {e}"))??;
        tracing::debug!(
            channel = %self.name,
            plugin = %self.plugin_name,
            output = %output,
            "WasmChannel send completed"
        );
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let wasm_path = self.wasm_path.display().to_string();
        let plugin_name = self.plugin_name.clone();
        let channel_name = self.name.clone();
        let _ = load_plugin(&wasm_path)?;

        loop {
            let path = wasm_path.clone();
            let pname = plugin_name.clone();
            let cname = channel_name.clone();
            let payload = serde_json::json!({
                "op": "listen",
                "plugin": pname,
                "channel": cname,
            });
            let input = serde_json::to_vec(&payload)?;
            let output = tokio::task::spawn_blocking(move || {
                invoke_wasm_export(&path, "channel_listen", &input)
                    .or_else(|_| invoke_wasm_export(&path, "listen", &input))
                    .or_else(|_| invoke_wasm_export(&path, "call", &input))
            })
            .await
            .map_err(|e| anyhow::anyhow!("WasmChannel listen join error: {e}"))?;

            match output {
                Ok(raw) => {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
                        let messages = if let Some(arr) = value.as_array() {
                            arr.clone()
                        } else if let Some(arr) =
                            value.get("messages").and_then(|v| v.as_array())
                        {
                            arr.clone()
                        } else if value.get("content").is_some() {
                            vec![value]
                        } else {
                            Vec::new()
                        };
                        for item in messages {
                            let content = item
                                .get("content")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            if content.is_empty() {
                                continue;
                            }
                            let sender = item
                                .get("sender")
                                .and_then(|v| v.as_str())
                                .unwrap_or("wasm")
                                .to_string();
                            let reply_target = item
                                .get("reply_target")
                                .or_else(|| item.get("chat_id"))
                                .and_then(|v| v.as_str())
                                .unwrap_or(&sender)
                                .to_string();
                            let msg = ChannelMessage {
                                id: item
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string)
                                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                                sender,
                                reply_target,
                                content,
                                channel: channel_name.clone(),
                                timestamp: item
                                    .get("timestamp")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or_else(now_epoch_secs),
                                thread_ts: item
                                    .get("thread_ts")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string),
                                interruption_scope_id: None,
                                attachments: Vec::new(),
                            };
                            if tx.send(msg).await.is_err() {
                                return Ok(());
                            }
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        channel = %channel_name,
                        plugin = %plugin_name,
                        error = %err,
                        "WasmChannel listen invoke failed; backing off"
                    );
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }
}
