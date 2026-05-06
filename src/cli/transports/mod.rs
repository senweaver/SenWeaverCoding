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

#[async_trait]
pub trait Transport: Send + Sync {

    async fn send(&self, data: &str) -> Result<()>;

    async fn recv(&self) -> Result<Option<String>>;

    async fn close(&self) -> Result<()>;

    fn is_connected(&self) -> bool;

    fn name(&self) -> &str;
}

pub fn transport_for_url(url: &str) -> &'static str {
    if url.starts_with("wss://") || url.starts_with("ws://") {
        "websocket"
    } else if url.contains("/v2/") || url.contains("/sse") {
        "sse"
    } else {
        "websocket"
    }
}
