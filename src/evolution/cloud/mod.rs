// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod openai_files;
pub mod huggingface_dataset;
pub mod rl_dataset_server;
pub mod tinker;
pub mod fireworks;
pub mod webhook;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::Utc;
use std::path::Path;
use std::time::Instant;

use super::EvolutionEngine;
use super::types::{CloudTarget, CloudTargetKind, ExportRecord, PushReceipt};

#[async_trait]
pub trait CloudPushTarget: Send + Sync {
    async fn push(
        &self,
        target: &CloudTarget,
        secret: Option<&str>,
        export: &ExportRecord,
        file_path: &Path,
    ) -> Result<PushOutcome>;
}

pub struct PushOutcome {
    pub status: String,
    pub response_excerpt: Option<String>,
}

pub fn target_for(kind: CloudTargetKind) -> Box<dyn CloudPushTarget> {
    match kind {
        CloudTargetKind::OpenaiFiles => Box::new(openai_files::OpenaiFilesTarget),
        CloudTargetKind::HuggingfaceDataset => Box::new(huggingface_dataset::HuggingfaceTarget),
        CloudTargetKind::RlDatasetServer => Box::new(rl_dataset_server::RlDatasetServerTarget),
        CloudTargetKind::Tinker => Box::new(tinker::TinkerTarget),
        CloudTargetKind::Fireworks => Box::new(fireworks::FireworksTarget),
        CloudTargetKind::Webhook => Box::new(webhook::WebhookTarget),
    }
}

pub async fn push_export_to_target(
    engine: &EvolutionEngine,
    target_id: &str,
    export_id: &str,
) -> Result<PushReceipt> {
    if !engine.persist_training_data() {
        return Err(anyhow!("persistence_disabled"));
    }
    let store = engine.store();
    let target = store
        .get_cloud_target(target_id)?
        .ok_or_else(|| anyhow!("target_not_found"))?;
    if !target.enabled {
        return Err(anyhow!("target_disabled"));
    }
    let export = store
        .get_export(export_id)?
        .ok_or_else(|| anyhow!("export_not_found"))?;
    let path = std::path::PathBuf::from(&export.path);
    if !path.exists() {
        return Err(anyhow!("export_file_missing"));
    }
    let secret = resolve_secret(&target);
    let pusher = target_for(target.kind);
    let start = Instant::now();
    let outcome = pusher.push(&target, secret.as_deref(), &export, &path).await?;
    let latency_ms = start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let receipt = PushReceipt {
        id: format!("push_{}", uuid::Uuid::new_v4().simple()),
        export_id: export.id.clone(),
        target_id: target.id.clone(),
        status: outcome.status,
        latency_ms: Some(latency_ms),
        response_excerpt: outcome.response_excerpt,
        ts: Utc::now(),
    };
    store.record_push_receipt(&receipt)?;
    let _ = store.set_target_last_pushed(&target.id, Utc::now());
    Ok(receipt)
}

fn resolve_secret(target: &CloudTarget) -> Option<String> {
    let secret_ref = target.secret_ref.as_ref()?;
    if let Ok(value) = std::env::var(secret_ref) {
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

pub fn truncate_excerpt(text: &str, max_chars: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= max_chars {
        return Some(text.to_string());
    }
    Some(text.chars().take(max_chars).collect())
}
