// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Built-in `tool_search` tool for on-demand MCP tool schema loading.
//!
//! When `mcp.deferred_loading` is enabled, this tool lets the LLM discover and
//! activate deferred MCP tools. Supports two query modes:
//! - `select:name1,name2` — fetch exact tools by prefixed name.
//! - Free-text keyword search — returns the best-matching stubs.

use std::fmt::Write;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::tools::mcp_deferred::{ActivatedToolSet, DeferredBuiltinToolSet, DeferredMcpToolSet};
use crate::tools::traits::{Tool, ToolResult};

const DEFAULT_MAX_RESULTS: usize = 5;

pub struct ToolSearchTool {
    deferred: DeferredMcpToolSet,
    builtin: DeferredBuiltinToolSet,
    activated: Arc<Mutex<ActivatedToolSet>>,
}

impl ToolSearchTool {
    pub fn new(deferred: DeferredMcpToolSet, activated: Arc<Mutex<ActivatedToolSet>>) -> Self {
        Self {
            deferred,
            builtin: DeferredBuiltinToolSet::new(),
            activated,
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
        }
    }

    pub fn with_builtin(mut self, builtin: DeferredBuiltinToolSet) -> Self {
        self.builtin = builtin;
        self
    }
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "tool_search"
    }

    fn description(&self) -> &str {
        "Fetch full schema definitions for deferred tools (built-in and/or MCP) so they can be called. \
         Use \"select:name1,name2\" for exact match or keywords to search. \
         Activated tools become callable for the rest of the conversation."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "description": "Query to find deferred tools. Use \"select:<tool_name>\" for direct selection, or keywords to search.",
                    "type": "string"
                },
                "max_results": {
                    "description": "Maximum number of results to return (default: 5)",
                    "type": "number",
                    "default": DEFAULT_MAX_RESULTS
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim();

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

            let names: Vec<&str> = names_str.split(',').map(str::trim).collect();
            return self.select_tools(&names);
        }

        let mcp_results = self.deferred.search(query, max_results);
        let builtin_results = self.builtin.search(query, max_results);

        if mcp_results.is_empty() && builtin_results.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: "No matching deferred tools found.".into(),
                error: None,
            });
        }

        let mut output = String::from("<functions>\n");
        let mut activated_count = 0;
        let mut guard = self.activated.lock();

        for stub in &mcp_results {
            if let Some(spec) = self.deferred.tool_spec(&stub.prefixed_name) {
                if !guard.is_activated(&stub.prefixed_name) {
                    if let Some(tool) = self.deferred.activate(&stub.prefixed_name) {
                        guard.activate(stub.prefixed_name.clone(), Arc::from(tool));
                        activated_count += 1;
                    }
                }
                let _ = writeln!(
                    output,
                    "<function>{{\"name\": \"{}\", \"description\": \"{}\", \"parameters\": {}}}</function>",
                    spec.name,
                    spec.description.replace('"', "\\\""),
                    spec.parameters
                );
            }
        }

        for stub in &builtin_results {
            if let Some(spec) = self.builtin.tool_spec(&stub.name) {
                if !guard.is_activated(&stub.name) {
                    guard.activate_spec(stub.name.clone(), spec.clone());
                    activated_count += 1;
                }
                let _ = writeln!(
                    output,
                    "<function>{{\"name\": \"{}\", \"description\": \"{}\", \"parameters\": {}}}</function>",
                    spec.name,
                    spec.description.replace('"', "\\\""),
                    spec.parameters
                );
            }
        }

        output.push_str("</functions>\n");
        drop(guard);

        tracing::debug!(
            "tool_search: query={query:?}, mcp_matched={}, builtin_matched={}, activated={activated_count}",
            mcp_results.len(),
            builtin_results.len()
        );

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

impl ToolSearchTool {
    fn select_tools(&self, names: &[&str]) -> anyhow::Result<ToolResult> {
        let mut output = String::from("<functions>\n");
        let mut not_found = Vec::new();
        let mut activated_count = 0;
        let mut guard = self.activated.lock();

        for name in names {
            if name.is_empty() {
                continue;
            }
            if let Some(spec) = self.deferred.tool_spec(name) {
                if !guard.is_activated(name) {
                    if let Some(tool) = self.deferred.activate(name) {
                        guard.activate(String::from(*name), Arc::from(tool));
                        activated_count += 1;
                    }
                }
                let _ = writeln!(
                    output,
                    "<function>{{\"name\": \"{}\", \"description\": \"{}\", \"parameters\": {}}}</function>",
                    spec.name,
                    spec.description.replace('"', "\\\""),
                    spec.parameters
                );
                continue;
            }
            if let Some(spec) = self.builtin.tool_spec(name) {
                if !guard.is_activated(name) {
                    guard.activate_spec(String::from(*name), spec.clone());
                    activated_count += 1;
                }
                let _ = writeln!(
                    output,
                    "<function>{{\"name\": \"{}\", \"description\": \"{}\", \"parameters\": {}}}</function>",
                    spec.name,
                    spec.description.replace('"', "\\\""),
                    spec.parameters
                );
                continue;
            }
            not_found.push(*name);
        }

        output.push_str("</functions>\n");
        drop(guard);

        if !not_found.is_empty() {
            let _ = write!(output, "\nNot found: {}", not_found.join(", "));
        }

        tracing::debug!(
            "tool_search select: requested={}, activated={activated_count}, not_found={}",
            names.len(),
            not_found.len()
        );

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}
