// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use uuid::Uuid;

pub struct CliChannel;

impl CliChannel {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Channel for CliChannel {
    fn name(&self) -> &str {
        "cli"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        println!("{}", message.content);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin);

        loop {
            let mut raw = Vec::new();
            let n = reader.read_until(b'\n', &mut raw).await;
            match n {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&raw).trim().to_string();
            if line.is_empty() {
                continue;
            }
            if line == "/quit" || line == "/exit" {
                break;
            }

            let msg = ChannelMessage {
                id: Uuid::new_v4().to_string(),
                sender: "user".to_string(),
                reply_target: "user".to_string(),
                content: line,
                channel: "cli".to_string(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                thread_ts: None,
                interruption_scope_id: None,
                attachments: vec![],
            };

            if crate::channels::forward_channel_message("cli", &tx, msg).is_closed() {
                break;
            }
        }
        Ok(())
    }
}
