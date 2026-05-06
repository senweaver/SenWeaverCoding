// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::Tunnel;
use anyhow::Result;

pub struct NoneTunnel;

#[async_trait::async_trait]
impl Tunnel for NoneTunnel {
    fn name(&self) -> &str {
        "none"
    }

    async fn start(&self, local_host: &str, local_port: u16) -> Result<String> {
        Ok(format!("http://{local_host}:{local_port}"))
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }

    async fn health_check(&self) -> bool {
        true
    }

    fn public_url(&self) -> Option<String> {
        None
    }
}
