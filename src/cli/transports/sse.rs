// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Server-Sent Events (SSE) transport — read via SSE stream, write via HTTP POST.

use super::Transport;
use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;

/// SSE transport configuration.
#[derive(Debug, Clone)]
pub struct SseConfig {
    pub read_url: String,
    pub write_url: String,
    pub auth_headers: Vec<(String, String)>,
    pub reconnect_delay_ms: u64,
}

impl Default for SseConfig {
    fn default() -> Self {
        Self {
            read_url: String::new(),
            write_url: String::new(),
            auth_headers: Vec::new(),
            reconnect_delay_ms: 1000,
        }
    }
}

/// SSE transport: reads from SSE stream, writes via HTTP POST.
pub struct SseTransport {
    config: SseConfig,
    connected: AtomicBool,
    inbox: Arc<tokio::sync::Mutex<mpsc::Receiver<String>>>,
    in_tx: mpsc::Sender<String>,
    write_url: String,
}

impl SseTransport {
    pub fn new(config: SseConfig) -> Self {
        let (in_tx, in_rx) = mpsc::channel(256);
        let write_url = config.write_url.clone();
        Self {
            config,
            connected: AtomicBool::new(false),
            inbox: Arc::new(tokio::sync::Mutex::new(in_rx)),
            in_tx,
            write_url,
        }
    }

    pub async fn connect(&mut self) -> Result<()> {
        tracing::info!(
            read = %self.config.read_url,
            write = %self.config.write_url,
            "Connecting SSE transport"
        );

        let read_url = self.config.read_url.clone();
        let auth_headers = self.config.auth_headers.clone();
        let reconnect_delay = self.config.reconnect_delay_ms;
        let tx = self.in_tx.clone();
        let connected = Arc::new(AtomicBool::new(true));
        let conn_flag = connected.clone();

        tokio::spawn(async move {
            loop {
                if !conn_flag.load(Ordering::SeqCst) {
                    break;
                }
                match Self::read_sse_stream(&read_url, &auth_headers, &tx).await {
                    Ok(()) => break,
                    Err(e) => {
                        tracing::warn!(error = %e, "SSE stream disconnected, reconnecting");
                        tokio::time::sleep(std::time::Duration::from_millis(reconnect_delay)).await;
                    }
                }
            }
        });

        self.connected.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn read_sse_stream(
        url: &str,
        auth_headers: &[(String, String)],
        tx: &mpsc::Sender<String>,
    ) -> Result<()> {
        let client = reqwest::Client::new();
        let mut req = client
            .get(url)
            .header("Accept", "text/event-stream")
            .header("Cache-Control", "no-cache");
        for (k, v) in auth_headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let response = req.send().await?;
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            let frames = parse_sse_frames(&buffer);
            if !frames.is_empty() {
                if let Some(last_double_newline) = buffer.rfind("\n\n") {
                    buffer = buffer[last_double_newline + 2..].to_string();
                }
                for frame in frames {
                    if tx.send(frame).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Transport for SseTransport {
    async fn send(&self, data: &str) -> Result<()> {
        if !self.connected.load(Ordering::SeqCst) {
            anyhow::bail!("SSE transport not connected");
        }
        let client = reqwest::Client::new();
        client
            .post(&self.write_url)
            .header("Content-Type", "application/json")
            .body(data.to_string())
            .send()
            .await?;
        Ok(())
    }

    async fn recv(&self) -> Result<Option<String>> {
        let mut inbox = self.inbox.lock().await;
        match inbox.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => Ok(None),
        }
    }

    async fn close(&self) -> Result<()> {
        self.connected.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn name(&self) -> &str {
        "sse"
    }
}

/// Parse SSE frames from a raw byte stream.
pub fn parse_sse_frames(data: &str) -> Vec<String> {
    let mut frames = Vec::new();
    let mut current = String::new();

    for line in data.lines() {
        if line.starts_with("data: ") {
            current.push_str(&line["data: ".len()..]);
        } else if line.is_empty() && !current.is_empty() {
            frames.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        frames.push(current);
    }

    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_frame() {
        let data = "data: {\"hello\":\"world\"}\n\n";
        let frames = parse_sse_frames(data);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], r#"{"hello":"world"}"#);
    }

    #[test]
    fn parse_multiple_frames() {
        let data = "data: frame1\n\ndata: frame2\n\n";
        let frames = parse_sse_frames(data);
        assert_eq!(frames.len(), 2);
    }

    #[test]
    fn transport_name() {
        let t = SseTransport::new(SseConfig::default());
        assert_eq!(t.name(), "sse");
    }
}
