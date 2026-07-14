// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::agent::TurnEvent;
use crate::providers::ChatMessage;

#[derive(Debug, Clone)]
pub struct TurnExperienceSummary {
    pub user_query: String,
    pub assistant_response: String,
    pub tools_used: Vec<String>,
    pub tool_results: Vec<(String, bool)>,
}

#[async_trait]
pub trait ResponseCacheHook: Send + Sync {
    fn build_key(&self, messages: &[ChatMessage], model: &str) -> Option<String>;

    async fn try_hit(&self, key: &str, user_message: &str) -> Option<String>;

    async fn write_back(
        &self,
        key: &str,
        model: &str,
        response: &str,
        output_tokens: u32,
    );
}

#[async_trait]
pub trait MemorySessionHook: Send + Sync {
    async fn on_turn_start(&self, user_message: &str);

    async fn on_turn_end(&self, assistant_message: &str, tools_used: &[String]);
}

#[async_trait]
pub trait TurnPreambleHook: Send + Sync {
    async fn apply(
        &self,
        user_message: &str,
        event_tx: &mpsc::Sender<TurnEvent>,
    ) -> Result<()>;
}

#[async_trait]
pub trait GuiModelSwitchHook: Send + Sync {
    async fn poll(&self, event_tx: &mpsc::Sender<TurnEvent>) -> Option<String>;
}

#[async_trait]
pub trait IterationContextBudgetHook: Send + Sync {
    async fn prepare(
        &self,
        iteration: usize,
        event_tx: &mpsc::Sender<TurnEvent>,
    );
}

#[async_trait]
pub trait ExperienceRecorderHook: Send + Sync {
    async fn record(&self, summary: &TurnExperienceSummary);
}

#[async_trait]
pub trait PlanModeNudgeHook: Send + Sync {
    async fn try_inject(
        &self,
        iteration: usize,
        history: &mut Vec<ChatMessage>,
        event_tx: &mpsc::Sender<TurnEvent>,
    ) -> bool;
}
