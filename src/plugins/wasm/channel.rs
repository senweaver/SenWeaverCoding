// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::channels::traits::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;

pub struct WasmChannel {
    name: String,
    plugin_name: String,
}

impl WasmChannel {
    pub fn new(name: String, plugin_name: String) -> Self {
        Self { name, plugin_name }
    }
}

#[async_trait]
impl Channel for WasmChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {

        tracing::warn!(
            "WasmChannel '{}' (plugin: {}) send not yet connected: {}",
            self.name,
            self.plugin_name,
            message.content
        );
        Ok(())
    }

    async fn listen(&self, _tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {

        tracing::warn!(
            "WasmChannel '{}' (plugin: {}) listen not yet connected",
            self.name,
            self.plugin_name,
        );
        Ok(())
    }
}
