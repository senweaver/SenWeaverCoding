// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scope {

    Global,

    Team(TeamId),

    Agent(AgentId),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TeamId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LayerPriority(u32);

impl LayerPriority {
    pub fn for_scope(scope: &Scope) -> Self {
        match scope {
            Scope::Global => LayerPriority(0),
            Scope::Team(_) => LayerPriority(50),
            Scope::Agent(_) => LayerPriority(100),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContextEntry {
    pub value: ContextValue,
    pub version: u64,
    pub timestamp: std::time::Instant,
    pub scope: Scope,
}

impl ContextEntry {
    pub fn new(value: ContextValue, scope: Scope) -> Self {
        Self {
            value,
            version: 1,
            timestamp: std::time::Instant::now(),
            scope,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ContextValue {
    Text(String),
    Json(serde_json::Value),
    Binary(Vec<u8>),
    Structured(ContextData),
}

#[derive(Debug, Clone, Default)]
pub struct ContextData {
    pub fields: std::collections::HashMap<String, ContextValue>,
}

impl ContextData {
    pub fn get(&self, key: &str) -> Option<&ContextValue> {
        self.fields.get(key)
    }

    pub fn set(&mut self, key: String, value: ContextValue) {
        self.fields.insert(key, value);
    }
}

#[derive(Debug)]
pub struct ContextLayer {
    priority: LayerPriority,
    scope: Scope,
    entries: RwLock<std::collections::HashMap<String, ContextEntry>>,
}

impl ContextLayer {
    pub fn new(priority: LayerPriority, scope: Scope) -> Self {
        Self {
            priority,
            scope,
            entries: RwLock::new(std::collections::HashMap::new()),
        }
    }

    pub fn get(&self, key: &str) -> Option<(LayerPriority, ContextEntry)> {
        self.entries
            .read()
            .get(key)
            .map(|e| (self.priority, e.clone()))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum ConflictStrategy {

    #[default]
    PriorityMerge,

    LastWriteWins,

    ConsensusMerge,
}

pub struct LayeredContext {
    layers: RwLock<Vec<Arc<ContextLayer>>>,
    version_counter: Arc<std::sync::atomic::AtomicU64>,
}

impl Default for LayeredContext {
    fn default() -> Self {
        Self::new()
    }
}

impl LayeredContext {
    pub fn new() -> Self {
        Self {
            layers: RwLock::new(Vec::new()),
            version_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub fn set(&self, key: impl Into<String>, value: ContextValue, scope: Scope) {
        let priority = LayerPriority::for_scope(&scope);
        let layer = self.layer_for_priority(priority, &scope);
        let entry = ContextEntry::new(value, scope);
        layer.entries.write().insert(key.into(), entry);
        self.bump_version();
    }

    pub fn overwrite(&self, key: impl Into<String>, value: ContextValue, scope: Scope) {
        let key_str = key.into();

        let layers = self.layers.read();
        for layer in layers.iter() {
            layer.entries.write().remove(&key_str);
        }
        drop(layers);

        self.set(&key_str, value, scope);
    }

    pub fn get(&self, key: &str) -> Option<(ContextValue, Scope)> {

        let layers = self.layers.read();
        for layer in layers.iter().rev() {
            if let Some(entry) = layer.entries.read().get(key) {
                return Some((entry.value.clone(), layer.scope.clone()));
            }
        }
        None
    }

    pub fn get_scoped(&self, key: &str, scope: &Scope) -> Option<ContextValue> {
        let priority = LayerPriority::for_scope(scope);
        let layers = self.layers.read();
        for layer in layers.iter() {
            if layer.priority == priority {
                return layer.entries.read().get(key).map(|e| e.value.clone());
            }
        }
        None
    }

    pub fn keys_for_scope(&self, scope: &Scope) -> Vec<String> {
        let priority = LayerPriority::for_scope(scope);
        let layers = self.layers.read();
        let mut keys = Vec::new();
        for layer in layers.iter().rev() {
            if layer.priority >= priority {
                for key in layer.entries.read().keys() {
                    keys.push(key.clone());
                }
            }
        }
        keys.sort();
        keys.dedup();
        keys
    }

    pub fn delete(&self, key: &str, scope: &Scope) -> bool {
        let priority = LayerPriority::for_scope(scope);
        let layers = self.layers.read();
        for layer in layers.iter() {
            if layer.priority == priority {
                return layer.entries.write().remove(key).is_some();
            }
        }
        false
    }

    pub fn snapshot(&self) -> ContextSnapshot {
        let layers = self.layers.read();
        let mut entries = Vec::new();
        for layer in layers.iter() {
            for (k, entry) in layer.entries.read().iter() {
                entries.push((k.clone(), entry.value.clone()));
            }
        }
        drop(layers);
        ContextSnapshot {
            version: self.version(),
            entries,
        }
    }

    pub fn restore(&self, snapshot: &ContextSnapshot) {

        {
            let layers = self.layers.read();
            for layer in layers.iter() {
                layer.entries.write().clear();
            }
        }

        for (key, value) in &snapshot.entries {
            self.set(key.clone(), value.clone(), Scope::Global);
        }
    }

    pub fn version(&self) -> u64 {
        self.version_counter
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn bump_version(&self) {
        self.version_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn layer_for_priority(&self, priority: LayerPriority, scope: &Scope) -> Arc<ContextLayer> {

        {
            let layers = self.layers.read();
            for layer in layers.iter() {
                if layer.priority == priority {
                    return Arc::clone(layer);
                }
            }
        }

        let new_layer = Arc::new(ContextLayer::new(priority, scope.clone()));
        let mut layers = self.layers.write();
        let pos = layers.iter().position(|l| l.priority > priority);
        match pos {
            Some(idx) => layers.insert(idx, Arc::clone(&new_layer)),
            None => layers.push(Arc::clone(&new_layer)),
        }
        Arc::clone(&new_layer)
    }
}

#[derive(Debug, Clone)]
pub struct ContextSnapshot {
    pub version: u64,
    pub entries: Vec<(String, ContextValue)>,
}
