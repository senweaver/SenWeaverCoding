// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::token_saver::pipeline::Rule;
use crate::token_saver::CompactContext;
use once_cell::sync::OnceCell;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub match_command: Option<String>,
    #[serde(default)]
    pub max_lines: Option<usize>,
    #[serde(default)]
    pub head: Option<usize>,
    #[serde(default)]
    pub tail: Option<usize>,
    #[serde(default)]
    pub dedup: bool,
    #[serde(default)]
    pub on_empty: Option<String>,
    #[serde(default)]
    pub strip_lines: Vec<String>,
    #[serde(default, rename = "replace")]
    pub replace: Vec<RawReplace>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawReplace {
    pub pattern: String,
    pub to: String,
}

impl RawRule {
    pub fn into_compiled(self, fallback_name: &str) -> Rule {
        let name = self.name.unwrap_or_else(|| fallback_name.to_string());
        Rule::compile(
            name,
            self.strip_lines,
            self.replace.into_iter().map(|r| (r.pattern, r.to)).collect(),
            self.max_lines,
            self.head,
            self.tail,
            self.dedup,
            self.on_empty,
            self.match_command,
        )
    }
}

static REGISTRY: OnceCell<Mutex<HashMap<String, Rule>>> = OnceCell::new();

fn registry() -> &'static Mutex<HashMap<String, Rule>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn lookup(name: &str, ctx: &CompactContext) -> Option<Rule> {
    ensure_loaded(ctx);
    registry()
        .lock()
        .ok()
        .and_then(|m| m.get(name).cloned())
}

pub fn list_names(ctx: &CompactContext) -> Vec<String> {
    ensure_loaded(ctx);
    registry()
        .lock()
        .ok()
        .map(|m| {
            let mut v: Vec<String> = m.keys().cloned().collect();
            v.sort();
            v
        })
        .unwrap_or_default()
}

pub fn loaded_count(ctx: &CompactContext) -> usize {
    ensure_loaded(ctx);
    registry().lock().ok().map(|m| m.len()).unwrap_or(0)
}

fn ensure_loaded(ctx: &CompactContext) {
    let already = registry().lock().map(|m| !m.is_empty()).unwrap_or(true);
    if already {
        return;
    }
    let mut map: HashMap<String, Rule> = HashMap::new();

    for (name, src) in super::embedded_filters::ALL {
        match toml::from_str::<RawRule>(src) {
            Ok(raw) => {
                let rule = raw.into_compiled(name);
                map.insert(rule.name.clone(), rule);
            }
            Err(e) => {
                tracing::warn!(rule = %name, error = %e, "embedded toml filter failed to parse");
            }
        }
    }

    if let Some(dir) = &ctx.custom_filters_dir {
        load_dir(dir, &mut map);
    }

    if let Ok(mut guard) = registry().lock() {
        *guard = map;
    }
}

fn load_dir(dir: &std::path::Path, map: &mut HashMap<String, Rule>) {
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("custom")
            .to_string();
        match std::fs::read_to_string(&path) {
            Ok(src) => match toml::from_str::<RawRule>(&src) {
                Ok(raw) => {
                    let rule = raw.into_compiled(&stem);
                    map.insert(rule.name.clone(), rule);
                }
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "user toml filter parse error");
                }
            },
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "could not read user toml filter");
            }
        }
    }
}
