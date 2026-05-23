// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use super::traits::ToolSpec;

pub struct ToolSpecCache {
    entries: RwLock<HashMap<CacheKey, Arc<str>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    content_hash: u64,
    provider_id: Arc<str>,
}

impl Default for ToolSpecCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolSpecCache {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    pub fn get_or_compute<F>(&self, provider_id: &str, specs: &[ToolSpec], compute: F) -> Arc<str>
    where
        F: FnOnce(&[ToolSpec]) -> String,
    {
        let hash = hash_specs(specs);
        let key = CacheKey {
            content_hash: hash,
            provider_id: Arc::from(provider_id),
        };

        if let Some(cached) = self.entries.read().get(&key) {
            return cached.clone();
        }

        let serialized: Arc<str> = Arc::from(compute(specs));
        self.entries.write().insert(key, serialized.clone());
        serialized
    }

    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.read().is_empty()
    }

    pub fn clear(&self) {
        self.entries.write().clear();
    }
}

fn hash_specs(specs: &[ToolSpec]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for spec in specs {
        spec.name.hash(&mut hasher);
        spec.description.hash(&mut hasher);

        let s = spec.parameters.to_string();
        s.hash(&mut hasher);
    }
    hasher.finish()
}
