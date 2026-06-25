// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use dashmap::DashMap;
use once_cell::sync::Lazy;
use tokio_util::sync::CancellationToken;

pub struct ComputerRunRegistry {
    runs: DashMap<String, CancellationToken>,
}

impl ComputerRunRegistry {
    fn new() -> Self {
        Self {
            runs: DashMap::new(),
        }
    }

    pub fn register(&self, run_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.runs.insert(run_id.to_string(), token.clone());
        token
    }

    pub fn cancel(&self, run_id: &str) -> bool {
        if let Some(entry) = self.runs.get(run_id) {
            entry.value().cancel();
            true
        } else {
            false
        }
    }

    pub fn unregister(&self, run_id: &str) {
        self.runs.remove(run_id);
    }

    pub fn active_count(&self) -> usize {
        self.runs.len()
    }
}

static REGISTRY: Lazy<Arc<ComputerRunRegistry>> =
    Lazy::new(|| Arc::new(ComputerRunRegistry::new()));

pub fn run_registry() -> Arc<ComputerRunRegistry> {
    REGISTRY.clone()
}
