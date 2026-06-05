// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
pub mod runner;
pub mod types;
pub use types::{Hand, HandContext, HandRun, HandRunStatus};

use anyhow::{Context, Result};
use std::path::Path;

pub fn load_hands(hands_dir: &Path) -> Result<Vec<Hand>> {
    if !hands_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut hands = Vec::new();
    let entries = std::fs::read_dir(hands_dir)
        .with_context(|| format!("failed to read hands directory: {}", hands_dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read hand file: {}", path.display()))?;
        match toml::from_str::<Hand>(&content) {
            Ok(hand) => hands.push(hand),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "skipping malformed hand file");
            }
        }
    }

    Ok(hands)
}

pub fn load_hand_context(hands_dir: &Path, name: &str) -> Result<HandContext> {
    let path = hands_dir.join(name).join("context.json");
    if !path.exists() {
        return Ok(HandContext::new(name));
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read hand context: {}", path.display()))?;
    let ctx: HandContext = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse hand context: {}", path.display()))?;
    Ok(ctx)
}

pub fn save_hand_context(hands_dir: &Path, context: &HandContext) -> Result<()> {
    let dir = hands_dir.join(&context.hand_name);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create hand context dir: {}", dir.display()))?;
    let path = dir.join("context.json");
    let json = serde_json::to_string_pretty(context)?;
    std::fs::write(&path, json)
        .with_context(|| format!("failed to write hand context: {}", path.display()))?;
    Ok(())
}
