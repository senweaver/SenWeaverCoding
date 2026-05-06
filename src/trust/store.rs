// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Persistent trust-score storage.
//!
//! Scores are kept in a JSON file (`trust_scores.json`) inside the workspace
//! data directory.  Writes are atomic (write-to-temp then rename) so a crash
//! mid-flush never corrupts the file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::trust::types::TrustScore;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct TrustStoreData {
    scores: HashMap<String, TrustScore>,
}

pub struct TrustStore {
    path: PathBuf,
}

impl TrustStore {

    pub fn new(dir: &Path) -> Self {
        Self {
            path: dir.join("trust_scores.json"),
        }
    }

    pub fn load(&self) -> HashMap<String, TrustScore> {
        let Ok(bytes) = std::fs::read(&self.path) else {
            return HashMap::new();
        };
        serde_json::from_slice::<TrustStoreData>(&bytes)
            .map(|d| d.scores)
            .unwrap_or_default()
    }

    pub fn save(&self, scores: &HashMap<String, TrustScore>) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = TrustStoreData {
            scores: scores.clone(),
        };
        let json = serde_json::to_string_pretty(&data)?;

        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json.as_bytes())?;
        atomic_rename(&tmp, &self.path)?;
        Ok(())
    }
}

fn atomic_rename(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}
