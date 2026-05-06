// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! structured planner output for `CodeEditFlow`.
//!
//! The D5.2 planner returned a free-form list of
//! `path :: description` lines.  That format made it impossible for
//! the executor to:
//!
//! 1. understand step dependencies (so independent edits could run in
//!    parallel — task 3.5);
//! 2. honour token budgeting (no `est_tokens`, no risk hint);
//! 3. distinguish create / modify / delete / rename / review steps
//!    (the executor was forced to assume "modify").
//!
//! replaces the line-based prompt with a strict JSON contract
//! validated by [`validate_planner_response`].  The shape is exposed
//! to the LLM via `PLANNER_JSON_SCHEMA` (a literal embedded in the
//! prompt) so the model can be told exactly what to emit.  The
//! validator is used twice: once for the first response, and once
//! after a self-correct retry.  A second failure degrades to a
//! single catch-all step tagged with `planner_degraded=true` so
//! downstream verification still has something to verify and the
//! refine loop can attempt recovery.
//!
//! ## Data flow
//!
//! ```text
//!   ctx.goal + workspace_root + focus_files + symbols
//!     → CODE_EDIT_PLANNER_PROMPT_V2   (rendered with PLANNER_JSON_SCHEMA)
//!     → LLM                            (must reply with JSON only)
//!     → validate_planner_response      (parse + business rules)
//!     → auto_expand_with_symbol_graph  (optional, default ON)
//!     → PlanDependencyGraph::build     (validates depends_on, no cycles)
//!     → topo_layers                    (Kahn — each layer is JoinSet-able)
//!     → ctx.scratchpad["code_edit.plan_dag"] = JSON
//!     → Vec<Step> for PlanExecVerifyFlow::run_layered
//! ```
//!
//! The PlanDependencyGraph is intentionally tiny — we only need a
//! validator + topo layer producer.  will lift the same
//! shape into the multi-agent scheduler so the planner contract
//! stays portable.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::code_intel::symbol_graph::SymbolGraph;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanStepJson {

    pub id: String,

    pub path: String,
    pub kind: PlanStepKind,

    pub description: String,

    #[serde(default)]
    pub depends_on: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub est_tokens: Option<u32>,

    #[serde(default = "default_risk")]
    pub risk: RiskLevel,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected_scope: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_path: Option<String>,
}

fn default_risk() -> RiskLevel {
    RiskLevel::Medium
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepKind {

    Create,

    Modify,

    Delete,

    Rename,

    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlannerResponse {
    pub steps: Vec<PlanStepJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PlanParseError {
    #[error("planner response was not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("plan has no steps")]
    EmptySteps,
    #[error("step id is empty at index {index}")]
    EmptyStepId { index: usize },
    #[error("step id `{id}` is not unique (duplicated at indices {first} and {second})")]
    DuplicateStepId {
        id: String,
        first: usize,
        second: usize,
    },
    #[error("step `{step}` depends on unknown id `{missing}`")]
    UnknownDependency { step: String, missing: String },
    #[error("step `{step}` depends on itself")]
    SelfDependency { step: String },
    #[error("plan has a dependency cycle through: {cycle}")]
    DependencyCycle { cycle: String },
    #[error("step `{step}` path `{path}` is absolute; the planner must emit workspace-relative paths")]
    AbsolutePath { step: String, path: String },
    #[error("step `{step}` path `{path}` escapes the workspace root via `..`")]
    EscapingPath { step: String, path: String },
    #[error("step `{step}` description is empty")]
    EmptyDescription { step: String },
}

pub fn validate_planner_response(raw: &str) -> Result<PlannerResponse, PlanParseError> {
    let trimmed = strip_code_fences(raw.trim());
    let resp: PlannerResponse = serde_json::from_str(trimmed)?;

    if resp.steps.is_empty() {
        return Err(PlanParseError::EmptySteps);
    }

    let mut id_at: HashMap<&str, usize> = HashMap::with_capacity(resp.steps.len());
    for (idx, step) in resp.steps.iter().enumerate() {
        let id = step.id.trim();
        if id.is_empty() {
            return Err(PlanParseError::EmptyStepId { index: idx });
        }
        if let Some(prev) = id_at.insert(id, idx) {
            return Err(PlanParseError::DuplicateStepId {
                id: id.to_string(),
                first: prev,
                second: idx,
            });
        }
    }

    for step in &resp.steps {
        if step.description.trim().is_empty() {
            return Err(PlanParseError::EmptyDescription {
                step: step.id.clone(),
            });
        }
        let path = Path::new(&step.path);
        if path.is_absolute() {
            return Err(PlanParseError::AbsolutePath {
                step: step.id.clone(),
                path: step.path.clone(),
            });
        }
        if step
            .path
            .split('/')
            .any(|seg| seg == "..")
        {
            return Err(PlanParseError::EscapingPath {
                step: step.id.clone(),
                path: step.path.clone(),
            });
        }
        for dep in &step.depends_on {
            if dep == &step.id {
                return Err(PlanParseError::SelfDependency {
                    step: step.id.clone(),
                });
            }
            if !id_at.contains_key(dep.as_str()) {
                return Err(PlanParseError::UnknownDependency {
                    step: step.id.clone(),
                    missing: dep.clone(),
                });
            }
        }
    }

    Ok(resp)
}

fn strip_code_fences(s: &str) -> &str {

    let s = s.trim();
    let candidates = ["```json", "```JSON", "```"];
    for prefix in candidates {
        if let Some(rest) = s.strip_prefix(prefix) {
            if let Some(inner) = rest.trim_start().strip_suffix("```") {
                return inner.trim();
            }
        }
    }
    s
}

#[derive(Debug, Clone)]
pub struct PlanDependencyGraph {

    pub steps: Vec<PlanStepJson>,

    pub edges_out: HashMap<String, HashSet<String>>,

    pub in_degree: HashMap<String, usize>,
}

impl PlanDependencyGraph {

    pub fn build(steps: Vec<PlanStepJson>) -> Result<Self, PlanParseError> {
        let id_set: HashSet<String> = steps.iter().map(|s| s.id.clone()).collect();
        let mut edges_out: HashMap<String, HashSet<String>> = HashMap::new();
        let mut in_degree: HashMap<String, usize> = HashMap::new();

        for step in &steps {
            edges_out.entry(step.id.clone()).or_default();
            in_degree.entry(step.id.clone()).or_insert(0);
        }

        for step in &steps {
            for dep in &step.depends_on {
                if !id_set.contains(dep) {
                    return Err(PlanParseError::UnknownDependency {
                        step: step.id.clone(),
                        missing: dep.clone(),
                    });
                }
                let inserted = edges_out
                    .entry(dep.clone())
                    .or_default()
                    .insert(step.id.clone());
                if inserted {
                    *in_degree.entry(step.id.clone()).or_insert(0) += 1;
                }
            }
        }

        let mut by_path: HashMap<String, Vec<String>> = HashMap::new();
        for step in &steps {

            let key = step.path.replace('\\', "/");
            by_path.entry(key).or_default().push(step.id.clone());
        }
        for ids in by_path.values() {
            if ids.len() < 2 {
                continue;
            }
            for w in ids.windows(2) {
                let (prev, curr) = (&w[0], &w[1]);
                let inserted = edges_out
                    .entry(prev.clone())
                    .or_default()
                    .insert(curr.clone());
                if inserted {
                    *in_degree.entry(curr.clone()).or_insert(0) += 1;
                }
            }
        }

        Ok(Self {
            steps,
            edges_out,
            in_degree,
        })
    }

    pub fn topo_layers(&self) -> Result<Vec<Vec<String>>, PlanParseError> {
        let mut in_degree: HashMap<String, usize> = self.in_degree.clone();
        let mut layers: Vec<Vec<String>> = Vec::new();
        let mut visited: usize = 0;
        let total = self.steps.len();

        loop {

            let mut layer: Vec<String> = Vec::new();
            for step in &self.steps {
                if in_degree.get(&step.id).copied().unwrap_or(0) == 0 && !already_layered(&layers, &step.id) {
                    layer.push(step.id.clone());
                }
            }
            if layer.is_empty() {
                break;
            }
            for id in &layer {
                if let Some(succs) = self.edges_out.get(id) {
                    for succ in succs {
                        if let Some(deg) = in_degree.get_mut(succ) {
                            *deg = deg.saturating_sub(1);
                        }
                    }
                }

                in_degree.insert(id.clone(), usize::MAX);
            }
            visited += layer.len();
            layers.push(layer);
            if visited >= total {
                break;
            }
        }

        if visited < total {

            let stuck: BTreeSet<&String> = self
                .steps
                .iter()
                .map(|s| &s.id)
                .filter(|id| !already_layered(&layers, id))
                .collect();
            return Err(PlanParseError::DependencyCycle {
                cycle: stuck
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(" → "),
            });
        }

        Ok(layers)
    }

    pub fn topo_order(&self) -> Result<Vec<String>, PlanParseError> {
        Ok(self.topo_layers()?.into_iter().flatten().collect())
    }

    pub fn step(&self, id: &str) -> Option<&PlanStepJson> {
        self.steps.iter().find(|s| s.id == id)
    }
}

fn already_layered(layers: &[Vec<String>], id: &str) -> bool {
    layers.iter().any(|layer| layer.iter().any(|x| x == id))
}

pub fn auto_expand_with_symbol_graph(steps: &mut Vec<PlanStepJson>, workspace_root: &Path) -> usize {
    const MAX_EXPANSIONS_PER_STEP: usize = 8;
    const TOTAL_HARD_CAP: usize = 32;

    let graph = match SymbolGraph::load(workspace_root) {
        Ok(Some(g)) => g,
        Ok(None) => {
            tracing::info!(
                target: "agent.flows.code_edit.planner",
                stage = "planner",
                auto_expand_candidates = 0u32,
                "symbol_graph_not_persisted; skipping auto_expand_deps",
            );
            return 0;
        }
        Err(e) => {
            tracing::warn!(
                target: "agent.flows.code_edit.planner",
                stage = "planner",
                error = %e,
                "symbol_graph load failed; skipping auto_expand_deps",
            );
            return 0;
        }
    };

    let primary_paths: HashSet<String> = steps
        .iter()
        .filter(|s| !matches!(s.kind, PlanStepKind::Review))
        .map(|s| s.path.replace('\\', "/"))
        .collect();

    let mut appended: usize = 0;
    let mut new_steps: Vec<PlanStepJson> = Vec::new();
    let primary_steps: Vec<PlanStepJson> = steps
        .iter()
        .filter(|s| matches!(s.kind, PlanStepKind::Modify | PlanStepKind::Delete | PlanStepKind::Rename))
        .cloned()
        .collect();

    'outer: for parent in &primary_steps {
        if appended >= TOTAL_HARD_CAP {
            break;
        }
        let mut emitted_for_this_parent: usize = 0;
        let symbol_candidates = extract_symbol_candidates(&parent.description);
        let mut seen_paths: HashSet<String> = HashSet::new();
        for symbol in symbol_candidates {
            if emitted_for_this_parent >= MAX_EXPANSIONS_PER_STEP {
                break;
            }

            let mut dependent_paths: Vec<PathBuf> = Vec::new();
            for caller in graph.callers_of(&symbol) {
                dependent_paths.push(caller.file.clone());
            }
            for implementor in graph.implementors_of(&symbol) {
                dependent_paths.push(implementor.file.clone());
            }

            for dep_path in dependent_paths {
                let key = dep_path.to_string_lossy().replace('\\', "/");
                if key == parent.path.replace('\\', "/") {
                    continue;
                }
                if primary_paths.contains(&key) {
                    continue;
                }
                if !seen_paths.insert(key.clone()) {
                    continue;
                }
                let id = format!(
                    "edit-expand-{parent_id}-{n}",
                    parent_id = parent.id,
                    n = emitted_for_this_parent
                );
                let step = PlanStepJson {
                    id,
                    path: key,
                    kind: PlanStepKind::Review,
                    description: format!(
                        "Review callers/implementors of `{symbol}` (touched by step `{}`); apply minimal corrections only if compilation/typing depends on the upstream change.",
                        parent.id
                    ),
                    depends_on: vec![parent.id.clone()],
                    est_tokens: Some(800),
                    risk: RiskLevel::Low,
                    affected_scope: None,
                    to_path: None,
                };
                new_steps.push(step);
                appended += 1;
                emitted_for_this_parent += 1;
                if appended >= TOTAL_HARD_CAP {
                    break 'outer;
                }
            }
        }
    }

    if !new_steps.is_empty() {
        steps.extend(new_steps);
    }

    tracing::info!(
        target: "agent.flows.code_edit.planner",
        stage = "planner",
        auto_expand_candidates = appended as u32,
        "auto_expand_deps_done",
    );
    appended
}

fn extract_symbol_candidates(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for word in tokenise(text) {
        if let Some(first) = word.chars().next() {
            if first.is_ascii_uppercase()
                && word.len() >= 3
                && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && seen.insert(word.to_string())
            {
                out.push(word.to_string());
            }
        }
    }

    for kw in ["fn ", "def ", "function "] {
        let mut rest = text;
        while let Some(pos) = rest.find(kw) {
            let after = &rest[pos + kw.len()..];
            let ident: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let ident_len = ident.len();
            if ident_len >= 3 && seen.insert(ident.clone()) {
                out.push(ident);
            }
            rest = &after[ident_len.min(after.len())..];
            if rest.is_empty() {
                break;
            }
        }
    }

    out
}

fn tokenise(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
}

pub const PLANNER_JSON_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["steps"],
  "properties": {
    "steps": {
      "type": "array",
      "minItems": 1,
      "items": {
        "type": "object",
        "required": ["id", "path", "kind", "description"],
        "properties": {
          "id": { "type": "string", "minLength": 1 },
          "path": { "type": "string", "minLength": 1, "description": "Workspace-relative path; absolute paths are rejected." },
          "kind": { "type": "string", "enum": ["create", "modify", "delete", "rename", "review"] },
          "description": { "type": "string", "minLength": 1 },
          "depends_on": { "type": "array", "items": { "type": "string" }, "default": [] },
          "est_tokens": { "type": "integer", "minimum": 0 },
          "risk": { "type": "string", "enum": ["low", "medium", "high"], "default": "medium" },
          "to_path": { "type": "string", "description": "Destination path for rename steps (workspace-relative). Required when kind = 'rename'." }
        }
      }
    },
    "reasoning": { "type": "string" }
  }
}"#;

pub const CODE_EDIT_PLANNER_PROMPT_V2: &str = r#"You are a precise code-editing planner. Respond with STRICTLY VALID JSON matching this schema (and nothing else — no markdown, no commentary):

{schema}

Goal:
{goal}

Workspace root: {workspace_root}
Focused files: {focus_files}
Relevant symbols: {symbol_summaries}

Rules:
- One step per self-contained edit on one file.
- `id` must be unique within this response.
- `depends_on` MUST reference earlier ids declared in this same response.
- Keep `description` imperative and specific (e.g. "Add `Foo::bar` returning `Result<X, Error>`", not "update Foo").
- Estimate `est_tokens` as the rough total tokens for prompt + response of that step (model output will be a unified diff, not a full file).
- Use `kind = "review"` when an edit on another file *might* affect this file's compilation but you are not sure it does — the executor is allowed to return an empty diff for review steps.
- Return valid JSON. No prose, no markdown fences."#;

pub const CODE_EDIT_PLANNER_RETRY_PROMPT: &str = r#"Your previous JSON response failed validation:
error: {error}
raw: {raw}

Produce a corrected JSON that matches the schema below. Return JSON only — no prose, no markdown fences.
{schema}"#;

pub fn render_planner_prompt(
    goal: &str,
    workspace_root: &str,
    focus_files: &str,
    symbol_summaries: &str,
) -> String {
    CODE_EDIT_PLANNER_PROMPT_V2
        .replace("{schema}", PLANNER_JSON_SCHEMA)
        .replace("{goal}", goal)
        .replace("{workspace_root}", workspace_root)
        .replace("{focus_files}", focus_files)
        .replace("{symbol_summaries}", symbol_summaries)
}

pub fn render_planner_retry_prompt(raw: &str, error: &str) -> String {
    CODE_EDIT_PLANNER_RETRY_PROMPT
        .replace("{schema}", PLANNER_JSON_SCHEMA)
        .replace("{error}", error)
        .replace("{raw}", raw)
}

pub fn degraded_catch_all_step(goal: &str, default_path: &str) -> PlanStepJson {
    PlanStepJson {
        id: "edit-0".into(),
        path: if default_path.is_empty() {
            "unknown-0".into()
        } else {
            default_path.to_string()
        },
        kind: PlanStepKind::Modify,
        description: goal.trim().to_string(),
        depends_on: Vec::new(),
        est_tokens: None,
        risk: RiskLevel::Medium,
        affected_scope: None,
        to_path: None,
    }
}

pub fn step_from_plan(plan: &PlanStepJson, planner_degraded: bool) -> super::traits::Step {
    let kind = match plan.kind {
        PlanStepKind::Create => "create",
        PlanStepKind::Modify => "modify",
        PlanStepKind::Delete => "delete",
        PlanStepKind::Rename => "rename",
        PlanStepKind::Review => "review",
    };
    let risk = match plan.risk {
        RiskLevel::Low => "low",
        RiskLevel::Medium => "medium",
        RiskLevel::High => "high",
    };
    let mut step = super::traits::Step::new(plan.id.clone(), plan.description.clone())
        .with_input("path", plan.path.clone())
        .with_input("kind", kind)
        .with_input("risk", risk)
        .with_input("depends_on", plan.depends_on.join(","));
    if let Some(t) = plan.est_tokens {
        step.inputs.insert("est_tokens".into(), t.to_string());
    }
    if planner_degraded {
        step.inputs
            .insert("planner_degraded".into(), "true".into());
    }
    if let Some(scopes) = plan.affected_scope.as_ref() {
        if !scopes.is_empty() {

            step.inputs
                .insert("affected_scope".into(), scopes.join(","));
        }
    }
    if let Some(tp) = plan.to_path.as_ref() {
        if !tp.is_empty() {
            step.inputs.insert("to_path".into(), tp.clone());
        }
    }
    step
}
