// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Network transports for remote CLI sessions.
//!
//! Defines the `Transport` trait and provides WebSocket, SSE, and
//! batch-upload implementations.

pub mod batch_uploader;
pub mod sse;
pub mod websocket;

use anyhow::Result;
use async_trait::async_trait;

/// Transport protocol for bidirectional message exchange.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send a message string to the remote endpoint.
    async fn send(&self, data: &str) -> Result<()>;

    /// Receive the next message from the remote endpoint.
    /// Returns `None` when the connection is closed.
    async fn recv(&self) -> Result<Option<String>>;

    /// Close the transport.
    async fn close(&self) -> Result<()>;

    /// Check if the transport is currently connected.
    fn is_connected(&self) -> bool;

    /// Transport name for logging.
    fn name(&self) -> &str;
}

/// Select a transport based on URL scheme.
pub fn transport_for_url(url: &str) -> &'static str {
    if url.starts_with("wss://") || url.starts_with("ws://") {
        "websocket"
    } else if url.contains("/v2/") || url.contains("/sse") {
        "sse"
    } else {
        "websocket"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_selects_websocket() {
        assert_eq!(transport_for_url("wss://example.com/session"), "websocket");
        assert_eq!(transport_for_url("ws://localhost:8080"), "websocket");
    }

    #[test]
    fn url_selects_sse() {
        assert_eq!(
            transport_for_url("https://api.example.com/v2/session"),
            "sse"
        );
    }
}
