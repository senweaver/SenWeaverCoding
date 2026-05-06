// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::sync::Arc;

use anyhow::Result;
use tracing::{info, warn};

use super::types::{SopRun, SopStepResult};
use crate::memory::traits::{Memory, MemoryCategory};

const SOP_CATEGORY: &str = "sop";

pub struct SopAuditLogger {
    memory: Arc<dyn Memory>,
}

impl SopAuditLogger {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self { memory }
    }

    pub async fn log_run_start(&self, run: &SopRun) -> Result<()> {
        let key = run_key(&run.run_id);
        let content = serde_json::to_string_pretty(run)?;
        self.memory.store(&key, &content, category(), None).await?;
        info!(
            "SOP audit: run {} started for '{}'",
            run.run_id, run.sop_name
        );
        Ok(())
    }

    pub async fn log_step_result(&self, run_id: &str, result: &SopStepResult) -> Result<()> {
        let key = step_key(run_id, result.step_number);
        let content = serde_json::to_string_pretty(result)?;
        self.memory.store(&key, &content, category(), None).await?;
        Ok(())
    }

    pub async fn log_run_complete(&self, run: &SopRun) -> Result<()> {
        let key = run_key(&run.run_id);
        let content = serde_json::to_string_pretty(run)?;
        self.memory.store(&key, &content, category(), None).await?;
        info!(
            "SOP audit: run {} finished with status {}",
            run.run_id, run.status
        );
        Ok(())
    }

    pub async fn log_approval(&self, run: &SopRun, step_number: u32) -> Result<()> {
        let key = format!("sop_approval_{}_{step_number}", run.run_id);
        let content = serde_json::to_string_pretty(run)?;
        self.memory.store(&key, &content, category(), None).await?;
        info!(
            "SOP audit: run {} step {step_number} approved by operator",
            run.run_id
        );
        Ok(())
    }

    pub async fn log_timeout_auto_approve(&self, run: &SopRun, step_number: u32) -> Result<()> {
        let key = format!("sop_timeout_approve_{}_{step_number}", run.run_id);
        let content = serde_json::to_string_pretty(run)?;
        self.memory.store(&key, &content, category(), None).await?;
        info!(
            "SOP audit: run {} step {step_number} auto-approved after timeout",
            run.run_id
        );
        Ok(())
    }

    pub async fn get_run(&self, run_id: &str) -> Result<Option<SopRun>> {
        let key = run_key(run_id);
        match self.memory.get(&key).await? {
            Some(entry) => {
                let run: SopRun = serde_json::from_str(&entry.content).map_err(|e| {
                    warn!("SOP audit: failed to parse run {run_id}: {e}");
                    e
                })?;
                Ok(Some(run))
            }
            None => Ok(None),
        }
    }

    pub async fn list_runs(&self) -> Result<Vec<String>> {
        let entries = self.memory.list(Some(&category()), None).await?;
        let run_keys: Vec<String> = entries
            .into_iter()
            .filter(|e| e.key.starts_with("sop_run_"))
            .map(|e| e.key)
            .collect();
        Ok(run_keys)
    }
}

fn run_key(run_id: &str) -> String {
    format!("sop_run_{run_id}")
}

fn step_key(run_id: &str, step_number: u32) -> String {
    format!("sop_step_{run_id}_{step_number}")
}

fn category() -> MemoryCategory {
    MemoryCategory::Custom(SOP_CATEGORY.into())
}
