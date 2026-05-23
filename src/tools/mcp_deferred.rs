// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;

use crate::tools::mcp_client::McpRegistry;
use crate::tools::mcp_protocol::McpToolDef;
use crate::tools::mcp_tool::McpToolWrapper;
use crate::tools::traits::{Tool, ToolSpec};

#[derive(Debug, Clone)]
pub struct DeferredMcpToolStub {

    pub prefixed_name: String,

    pub description: String,

    def: McpToolDef,
}

impl DeferredMcpToolStub {
    pub fn new(prefixed_name: String, def: McpToolDef) -> Self {
        let description = def
            .description
            .clone()
            .unwrap_or_else(|| "MCP tool".to_string());
        Self {
            prefixed_name,
            description,
            def,
        }
    }

    pub fn activate(&self, registry: Arc<McpRegistry>) -> McpToolWrapper {
        McpToolWrapper::new(self.prefixed_name.clone(), self.def.clone(), registry)
    }
}

#[derive(Clone)]
pub struct DeferredMcpToolSet {

    pub stubs: Vec<DeferredMcpToolStub>,

    pub registry: Arc<McpRegistry>,
}

impl DeferredMcpToolSet {

    pub async fn from_registry(registry: Arc<McpRegistry>) -> Self {
        let names = registry.tool_names();
        let mut stubs = Vec::with_capacity(names.len());
        for name in names {
            if let Some(def) = registry.get_tool_def(&name).await {
                stubs.push(DeferredMcpToolStub::new(name, def));
            }
        }
        Self { stubs, registry }
    }

    pub fn stub_names(&self) -> Vec<&str> {
        self.stubs
            .iter()
            .map(|s| s.prefixed_name.as_str())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.stubs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stubs.is_empty()
    }

    pub fn get_by_name(&self, name: &str) -> Option<&DeferredMcpToolStub> {
        self.stubs.iter().find(|s| s.prefixed_name == name)
    }

    pub fn search(&self, query: &str, max_results: usize) -> Vec<&DeferredMcpToolStub> {
        let terms: Vec<String> = query
            .split_whitespace()
            .map(|t| t.to_ascii_lowercase())
            .collect();
        if terms.is_empty() {
            return self.stubs.iter().take(max_results).collect();
        }

        let mut scored: Vec<(&DeferredMcpToolStub, usize)> = self
            .stubs
            .iter()
            .filter_map(|stub| {
                let haystack = format!(
                    "{} {}",
                    stub.prefixed_name.to_ascii_lowercase(),
                    stub.description.to_ascii_lowercase()
                );
                let hits = terms
                    .iter()
                    .filter(|t| haystack.contains(t.as_str()))
                    .count();
                if hits > 0 { Some((stub, hits)) } else { None }
            })
            .collect();

        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored
            .into_iter()
            .take(max_results)
            .map(|(s, _)| s)
            .collect()
    }

    pub fn activate(&self, name: &str) -> Option<Box<dyn Tool>> {
        self.get_by_name(name).map(|stub| {
            let wrapper = stub.activate(Arc::clone(&self.registry));
            Box::new(wrapper) as Box<dyn Tool>
        })
    }

    pub fn tool_spec(&self, name: &str) -> Option<ToolSpec> {
        self.get_by_name(name).map(|stub| {
            let wrapper = stub.activate(Arc::clone(&self.registry));
            wrapper.spec()
        })
    }
}

pub struct ActivatedToolSet {
    tools: HashMap<String, Arc<dyn Tool>>,
    specs: HashMap<String, ToolSpec>,
    revision: u64,
}

impl ActivatedToolSet {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            specs: HashMap::new(),
            revision: 0,
        }
    }

    pub fn activate(&mut self, name: String, tool: Arc<dyn Tool>) {
        let already = self.tools.contains_key(&name);
        self.tools.insert(name, tool);
        if !already {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub fn activate_spec(&mut self, name: String, spec: ToolSpec) {
        let already = self.specs.contains_key(&name);
        self.specs.insert(name, spec);
        if !already {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub fn is_activated(&self, name: &str) -> bool {
        self.tools.contains_key(name) || self.specs.contains_key(name)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn len(&self) -> usize {
        self.tools.len() + self.specs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty() && self.specs.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn get_resolved(&self, name: &str) -> Option<Arc<dyn Tool>> {
        if let Some(tool) = self.get(name) {
            return Some(tool);
        }
        if name.contains("__") {
            return None;
        }

        let mut resolved = None;
        for (tool_name, tool) in &self.tools {
            let Some((_, suffix)) = tool_name.split_once("__") else {
                continue;
            };
            if suffix != name {
                continue;
            }
            if resolved.is_some() {
                return None;
            }
            resolved = Some(Arc::clone(tool));
        }

        resolved
    }

    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        let mut out: Vec<ToolSpec> = self.tools.values().map(|t| t.spec()).collect();
        out.extend(self.specs.values().cloned());
        out
    }

    pub fn tool_names(&self) -> Vec<&str> {
        self.tools
            .keys()
            .chain(self.specs.keys())
            .map(|s| s.as_str())
            .collect()
    }
}

impl Default for ActivatedToolSet {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_deferred_tools_section(deferred: &DeferredMcpToolSet) -> String {
    if deferred.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str("## Deferred Tools\n\n");
    out.push_str(
        "The MCP tools listed below are available but their full schemas are NOT yet loaded. \
         To use any of them you MUST first call the `tool_search` tool to fetch their schemas:\n\
         - Exact: `tool_search(query=\"select:server__tool_a,server__tool_b\")`.\n\
         - Keyword: `tool_search(query=\"<1-4 keywords>\")`.\n\
         After activation, call the tool directly by its prefixed name — no need to invoke \
         `tool_search` again for the same tool.\n\n",
    );
    out.push_str("<available-deferred-tools>\n");
    for stub in &deferred.stubs {
        out.push_str(&stub.prefixed_name);
        out.push_str(" - ");
        out.push_str(&stub.description);
        out.push('\n');
    }
    out.push_str("</available-deferred-tools>\n");
    out
}

#[derive(Clone)]
pub struct DeferredBuiltinStub {
    pub name: String,
    pub description: String,
    pub spec: ToolSpec,
}

#[derive(Clone, Default)]
pub struct DeferredBuiltinToolSet {
    pub stubs: Vec<DeferredBuiltinStub>,
}

impl DeferredBuiltinToolSet {
    pub fn new() -> Self {
        Self { stubs: Vec::new() }
    }

    pub fn add_spec(&mut self, spec: ToolSpec) {
        self.stubs.push(DeferredBuiltinStub {
            name: spec.name.clone(),
            description: spec.description.clone(),
            spec,
        });
    }

    pub fn len(&self) -> usize {
        self.stubs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stubs.is_empty()
    }

    pub fn names(&self) -> Vec<&str> {
        self.stubs.iter().map(|s| s.name.as_str()).collect()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.stubs.iter().any(|s| s.name == name)
    }

    pub fn get_by_name(&self, name: &str) -> Option<&DeferredBuiltinStub> {
        self.stubs.iter().find(|s| s.name == name)
    }

    pub fn tool_spec(&self, name: &str) -> Option<ToolSpec> {
        self.get_by_name(name).map(|stub| stub.spec.clone())
    }

    pub fn search(&self, query: &str, max_results: usize) -> Vec<&DeferredBuiltinStub> {
        let terms: Vec<String> = query
            .split_whitespace()
            .map(|t| t.to_ascii_lowercase())
            .collect();
        if terms.is_empty() {
            return self.stubs.iter().take(max_results).collect();
        }
        let mut scored: Vec<(&DeferredBuiltinStub, usize)> = self
            .stubs
            .iter()
            .filter_map(|stub| {
                let haystack = format!(
                    "{} {}",
                    stub.name.to_ascii_lowercase(),
                    stub.description.to_ascii_lowercase()
                );
                let hits = terms
                    .iter()
                    .filter(|t| haystack.contains(t.as_str()))
                    .count();
                if hits > 0 { Some((stub, hits)) } else { None }
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored
            .into_iter()
            .take(max_results)
            .map(|(s, _)| s)
            .collect()
    }
}

pub fn build_deferred_builtin_section(deferred: &DeferredBuiltinToolSet) -> String {
    build_deferred_builtin_section_with_surface(
        deferred,
        crate::tools::tool_tier::ToolSurfaceBaseline::Both,
    )
}

pub fn build_deferred_builtin_section_with_surface(
    deferred: &DeferredBuiltinToolSet,
    surface: crate::tools::tool_tier::ToolSurfaceBaseline,
) -> String {
    if deferred.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str("## Deferred Built-in Tools\n\n");
    out.push_str(
        "The built-in tools listed below are available but their full schemas are NOT loaded \
         to keep the context window small. Activate via `tool_search`:\n\
         - Exact: `tool_search(query=\"select:name1,name2\")` — preferred when you know the tool name.\n\
         - Keyword: `tool_search(query=\"<1-4 keywords>\")` — fuzzy match on name + description.\n\
         Once a tool is activated, call it directly by its name on subsequent turns — do NOT \
         invoke `tool_search` again for the same tool. Activations persist per-workspace across sessions. \
         The `[Tier/Risk]` badge is informational metadata only and never blocks the call; \
         treat `HighRisk` as a hint to confirm intent before destructive operations.\n\n",
    );
    out.push_str("<available-deferred-builtin-tools>\n");
    for stub in &deferred.stubs {
        let entry = crate::tools::tool_tier::classify(&stub.name, surface);
        out.push_str(&stub.name);
        out.push_str(" [");
        out.push_str(entry.tier.as_str());
        out.push('/');
        out.push_str(entry.risk.as_str());
        out.push_str("] - ");
        out.push_str(&stub.description);
        out.push('\n');
    }
    out.push_str("</available-deferred-builtin-tools>\n");
    out
}
