// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;

use crate::tools::Tool;

#[async_trait]
pub trait Peripheral: Send + Sync {

    fn name(&self) -> &str;

    fn board_type(&self) -> &str;

    async fn connect(&mut self) -> anyhow::Result<()>;

    async fn disconnect(&mut self) -> anyhow::Result<()>;

    async fn health_check(&self) -> bool;

    fn tools(&self) -> Vec<Box<dyn Tool>>;
}
