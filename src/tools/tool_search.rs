// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fmt::Write;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::{Mutex, RwLock};

use crate::security::permissions::ToolActivationGateHandle;
use crate::tools::mcp_deferred::{ActivatedToolSet, DeferredBuiltinToolSet, DeferredMcpToolSet};
use crate::tools::tool_tier::{BuiltinToolTier, ToolSurfaceBaseline, ToolTierEntry, classify};
use crate::tools::traits::{Tool, ToolResult, ToolSpec};

const DEFAULT_MAX_RESULTS: usize = 5;

pub struct ToolSearchTool {
    deferred: DeferredMcpToolSet,
    builtin: DeferredBuiltinToolSet,
    activated: Arc<Mutex<ActivatedToolSet>>,
    surface: Arc<RwLock<ToolSurfaceBaseline>>,
    workspace_key: Arc<RwLock<String>>,
    #[allow(dead_code)]
    gate: Option<ToolActivationGateHandle>,
    #[allow(dead_code)]
    allowlist: Arc<RwLock<Vec<String>>>,
}

impl ToolSearchTool {
    pub fn new(deferred: DeferredMcpToolSet, activated: Arc<Mutex<ActivatedToolSet>>) -> Self {
        Self {
            deferred,
            builtin: DeferredBuiltinToolSet::new(),
            activated,
            surface: Arc::new(RwLock::new(ToolSurfaceBaseline::Both)),
            workspace_key: Arc::new(RwLock::new(String::new())),
            gate: None,
            allowlist: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn new_builtin_only(
        builtin: DeferredBuiltinToolSet,
        activated: Arc<Mutex<ActivatedToolSet>>,
    ) -> Self {
        Self {
            deferred: DeferredMcpToolSet {
                stubs: Vec::new(),
                registry: Arc::new(crate::tools::McpRegistry::empty()),
            },
            builtin,
            activated,
            surface: Arc::new(RwLock::new(ToolSurfaceBaseline::Both)),
            workspace_key: Arc::new(RwLock::new(String::new())),
            gate: None,
            allowlist: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn with_builtin(mut self, builtin: DeferredBuiltinToolSet) -> Self {
        self.builtin = builtin;
        self
    }

    pub fn with_surface(self, surface: ToolSurfaceBaseline) -> Self {
        *self.surface.write() = surface;
        self
    }

    pub fn surface_handle(&self) -> Arc<RwLock<ToolSurfaceBaseline>> {
        Arc::clone(&self.surface)
    }

    pub fn with_workspace_key(self, key: String) -> Self {
        *self.workspace_key.write() = key;
        self
    }

    pub fn workspace_key_handle(&self) -> Arc<RwLock<String>> {
        Arc::clone(&self.workspace_key)
    }

    pub fn with_gate(mut self, gate: Option<ToolActivationGateHandle>) -> Self {
        self.gate = gate;
        self
    }

    pub fn with_allowlist(self, allowlist: Vec<String>) -> Self {
        *self.allowlist.write() = allowlist;
        self
    }

    #[allow(dead_code)]
    pub fn allowlist_handle(&self) -> Arc<RwLock<Vec<String>>> {
        Arc::clone(&self.allowlist)
    }

    #[allow(dead_code)]
    fn is_in_allowlist(&self, name: &str) -> bool {
        let guard = self.allowlist.read();
        guard.iter().any(|n| n == name)
    }

    fn current_workspace_key(&self) -> String {
        self.workspace_key.read().clone()
    }

    fn current_surface(&self) -> ToolSurfaceBaseline {
        *self.surface.read()
    }

    async fn evaluate_activation(&self, name: &str) -> ActivationOutcome {
        let entry: ToolTierEntry = classify(name, self.current_surface());
        ActivationOutcome::Allowed { entry }
    }

    fn note_activation(&self, name: &str) {
        self.note_activations(std::iter::once(name));
    }

    fn note_activations<I, S>(&self, names: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let collected: Vec<String> = names
            .into_iter()
            .map(|n| n.as_ref().to_string())
            .filter(|n| !n.is_empty())
            .collect();
        if collected.is_empty() {
            return;
        }
        if let Some(svc) = crate::services::try_get_services() {
            svc.record_tool_search_activations(collected.len() as u64);
            let store = Arc::clone(&svc.tool_activation_store);
            let workspace_key = self.current_workspace_key();
            if workspace_key.is_empty() {
                return;
            }
            tokio::spawn(async move {
                if let Err(e) = store.add_many(&workspace_key, &collected).await {
                    tracing::warn!(
                        target: "tool_search.persist",
                        count = collected.len(),
                        error = %e,
                        "failed to persist tool activation batch"
                    );
                }
            });
        }
    }

    #[allow(dead_code)]
    fn promote_allowlist(&self, name: &str) {
        {
            let mut guard = self.allowlist.write();
            if guard.iter().any(|n| n == name) {
                return;
            }
            guard.push(name.to_string());
        }
        let name_owned = name.to_string();
        tokio::spawn(async move {
            if let Some(svc) = crate::services::try_get_services() {
                let config = svc.config();
                let mut updated = (*config).clone();
                if updated
                    .permissions
                    .tool_allowlist
                    .iter()
                    .any(|n| n == &name_owned)
                {
                    return;
                }
                updated.permissions.tool_allowlist.push(name_owned.clone());
                if let Err(e) = updated.save().await {
                    tracing::warn!(
                        target: "tool_search.persist",
                        error = %e,
                        "failed to persist tool_allowlist update"
                    );
                    return;
                }
                svc.update_config(updated);
            }
        });
    }

    #[allow(dead_code)]
    fn note_high_risk_blocked(&self) {
        if let Some(svc) = crate::services::try_get_services() {
            svc.record_tool_search_high_risk_blocked();
        }
    }
}

#[allow(dead_code)]
enum ActivationOutcome {
    Allowed { entry: ToolTierEntry },
    PromoteAllowlist { entry: ToolTierEntry },
    Denied,
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "tool_search"
    }

    fn description(&self) -> &str {
        "Fetch full schema definitions for deferred tools (built-in and/or MCP) so they can be called. \
         Two query modes are supported:\n\
         1. Exact selection: query=\"select:name1,name2\" — activates the listed tools by exact name (comma-separated, supports built-in tool names and MCP prefixed names like \"server__tool\").\n\
         2. Keyword search: query=\"cron schedule\" — scores stub name+description and returns the top-N matches.\n\
         After activation, call the tool directly by its name in subsequent turns; you do NOT need to call tool_search again for the same tool. Examples: query=\"select:cron_add,cron_list\", query=\"weather forecast\"."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "description": "Either \"select:<name1>,<name2>,...\" for exact activation (recommended when you know the tool name), or free-text keywords (1-4 words) to search by name and description.",
                    "type": "string"
                },
                "max_results": {
                    "description": "Maximum number of results to return for keyword search (default: 5; ignored in select mode).",
                    "type": "number",
                    "default": DEFAULT_MAX_RESULTS
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let started_at = std::time::Instant::now();
        let outcome = self.execute_inner(args).await;
        let elapsed_ms = started_at.elapsed().as_millis() as u64;
        if let Some(svc) = crate::services::try_get_services() {
            svc.record_tool_search_invocation(elapsed_ms);
        }
        outcome
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

impl ToolSearchTool {
    async fn execute_inner(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();

        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|v| usize::try_from(v).unwrap_or(DEFAULT_MAX_RESULTS))
            .unwrap_or(DEFAULT_MAX_RESULTS);

        if query.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("query parameter is required".into()),
            });
        }

        if let Some(names_str) = query.strip_prefix("select:") {
            let owned_names: Vec<String> = names_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let name_refs: Vec<&str> = owned_names.iter().map(String::as_str).collect();
            return self.select_tools(&name_refs).await;
        }

        let mcp_results = self
            .deferred
            .search(&query, max_results)
            .into_iter()
            .map(|stub| stub.prefixed_name.clone())
            .collect::<Vec<_>>();
        let builtin_results = self
            .builtin
            .search(&query, max_results)
            .into_iter()
            .map(|stub| stub.name.clone())
            .collect::<Vec<_>>();

        if mcp_results.is_empty() && builtin_results.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: "No matching deferred tools found.".into(),
                error: None,
            });
        }

        let mut output = String::from("<functions>\n");
        let mut activated_count = 0usize;
        let mut activated_names: Vec<String> = Vec::new();

        for name in &mcp_results {
            if let Some(spec) = self.deferred.tool_spec(name) {
                if let ActivationOutcome::Allowed { .. } =
                    self.evaluate_activation(name).await
                {
                    let mut guard = self.activated.lock();
                    if !guard.is_activated(name) {
                        if let Some(tool) = self.deferred.activate(name) {
                            guard.activate(name.clone(), Arc::from(tool));
                            activated_count += 1;
                            activated_names.push(name.clone());
                        }
                    }
                    drop(guard);
                    append_spec(&mut output, &spec);
                } else {
                    continue;
                }
            }
        }

        for name in &builtin_results {
            if let Some(spec) = self.builtin.tool_spec(name) {
                if let ActivationOutcome::Allowed { .. } =
                    self.evaluate_activation(name).await
                {
                    let mut guard = self.activated.lock();
                    if !guard.is_activated(name) {
                        guard.activate_spec(name.clone(), spec.clone());
                        activated_count += 1;
                        activated_names.push(name.clone());
                    }
                    drop(guard);
                    append_spec(&mut output, &spec);
                } else {
                    continue;
                }
            }
        }

        if !activated_names.is_empty() {
            self.note_activations(activated_names);
        }

        output.push_str("</functions>\n");

        tracing::debug!(
            "tool_search: query={query:?}, mcp_matched={}, builtin_matched={}, activated={activated_count}",
            mcp_results.len(),
            builtin_results.len(),
        );

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }

    async fn select_tools(&self, names: &[&str]) -> anyhow::Result<ToolResult> {
        let mut output = String::from("<functions>\n");
        let mut not_found: Vec<String> = Vec::new();
        let mut activated_count = 0usize;
        let mut activated_names: Vec<String> = Vec::new();

        for name in names {
            if name.is_empty() {
                continue;
            }
            if let Some(spec) = self.deferred.tool_spec(name) {
                if let ActivationOutcome::Allowed { .. } =
                    self.evaluate_activation(name).await
                {
                    let mut guard = self.activated.lock();
                    if !guard.is_activated(name) {
                        if let Some(tool) = self.deferred.activate(name) {
                            guard.activate((*name).to_string(), Arc::from(tool));
                            activated_count += 1;
                            activated_names.push((*name).to_string());
                        }
                    }
                    drop(guard);
                    append_spec(&mut output, &spec);
                } else {
                    continue;
                }
                continue;
            }
            if let Some(spec) = self.builtin.tool_spec(name) {
                if let ActivationOutcome::Allowed { .. } =
                    self.evaluate_activation(name).await
                {
                    let mut guard = self.activated.lock();
                    if !guard.is_activated(name) {
                        guard.activate_spec((*name).to_string(), spec.clone());
                        activated_count += 1;
                        activated_names.push((*name).to_string());
                    }
                    drop(guard);
                    append_spec(&mut output, &spec);
                } else {
                    continue;
                }
                continue;
            }
            not_found.push((*name).to_string());
        }

        if !activated_names.is_empty() {
            self.note_activations(activated_names);
        }

        output.push_str("</functions>\n");

        if !not_found.is_empty() {
            let _ = write!(output, "\nNot found: {}", not_found.join(", "));
        }

        tracing::debug!(
            "tool_search select: requested={}, activated={activated_count}, not_found={}",
            names.len(),
            not_found.len(),
        );

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }

    pub fn activate_from_history(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        let mut guard = self.activated.lock();
        if guard.is_activated(name) {
            return false;
        }
        if let Some(tool) = self.deferred.activate(name) {
            guard.activate(name.to_string(), Arc::from(tool));
            return true;
        }
        if let Some(spec) = self.builtin.tool_spec(name) {
            guard.activate_spec(name.to_string(), spec);
            return true;
        }
        false
    }
}

fn append_spec(output: &mut String, spec: &ToolSpec) {
    let _ = writeln!(
        output,
        "<function>{{\"name\": \"{}\", \"description\": \"{}\", \"parameters\": {}}}</function>",
        spec.name,
        spec.description.replace('"', "\\\""),
        spec.parameters
    );
}

#[allow(dead_code)]
fn entry_tier_label(entry: &ToolTierEntry) -> &'static str {
    match entry.tier {
        BuiltinToolTier::Core => "Core",
        BuiltinToolTier::Enhanced => "Enhanced",
        BuiltinToolTier::OnDemand => "OnDemand",
    }
}
