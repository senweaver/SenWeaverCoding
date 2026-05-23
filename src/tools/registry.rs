// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use dashmap::DashMap;
use serde_json::Value;

use crate::tools::traits::{Tool, ToolSpec};

#[derive(Default)]
pub struct ToolRegistry {

    by_name: DashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {

    #[inline]
    pub fn new() -> Self {
        Self {
            by_name: DashMap::new(),
        }
    }

    pub fn from_boxed(tools: Vec<Box<dyn Tool>>) -> Self {
        let reg = Self::new();
        for tool in tools {
            reg.register(Arc::from(tool));
        }
        reg
    }

    pub fn from_arc_tools(tools: Vec<Arc<dyn Tool>>) -> Self {
        let reg = Self::new();
        for tool in tools {
            reg.register(tool);
        }
        reg
    }

    #[inline]
    pub fn register(&self, tool: Arc<dyn Tool>) {
        self.by_name.insert(tool.name().to_string(), tool);
    }

    #[inline]
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.by_name.get(name).map(|entry| Arc::clone(&entry))
    }

    #[inline]
    pub fn contains(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    pub fn schema_snapshot(&self) -> Vec<ToolSpec> {
        self.by_name.iter().map(|entry| entry.spec()).collect()
    }

    pub fn schema_snapshot_arc(&self) -> Arc<[ToolSpec]> {
        let specs = self.schema_snapshot();
        Arc::from(specs)
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.by_name
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = Arc<dyn Tool>> + '_ {
        self.by_name.iter().map(|entry| Arc::clone(&entry))
    }

    pub fn schema_map(&self) -> std::sync::Arc<DashMap<String, Value>> {
        let map = std::sync::Arc::new(DashMap::new());
        for entry in self.by_name.iter() {
            map.insert(entry.key().clone(), entry.parameters_schema());
        }
        map
    }
}
