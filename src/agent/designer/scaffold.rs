// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use include_dir::{include_dir, Dir};
use serde::Serialize;
use std::sync::OnceLock;

static SCAFFOLDS_DIR: Dir<'static> =
    include_dir!("$CARGO_MANIFEST_DIR/assets/designer-scaffolds");

#[derive(Debug, Clone, Serialize)]
pub struct ScaffoldMeta {
    pub id: String,
    pub category: String,
    pub format: String,
    pub path: String,
    pub description: String,
}

fn str_field(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn build_catalog() -> Vec<ScaffoldMeta> {
    let Some(raw) = SCAFFOLDS_DIR
        .get_file("manifest.json")
        .and_then(|f| f.contents_utf8())
    else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let Some(items) = v.get("scaffolds").and_then(|s| s.as_array()) else {
        return Vec::new();
    };
    let mut out: Vec<ScaffoldMeta> = Vec::new();
    for item in items {
        let (Some(id), Some(path)) = (str_field(item, "id"), str_field(item, "path")) else {
            continue;
        };
        if SCAFFOLDS_DIR.get_file(&path).is_none() {
            continue;
        }
        out.push(ScaffoldMeta {
            id,
            category: str_field(item, "category").unwrap_or_else(|| "general".to_string()),
            format: str_field(item, "format").unwrap_or_else(|| "html".to_string()),
            path,
            description: str_field(item, "description").unwrap_or_default(),
        });
    }
    out.sort_by(|a, b| a.category.cmp(&b.category).then_with(|| a.id.cmp(&b.id)));
    out
}

pub fn catalog() -> &'static [ScaffoldMeta] {
    static CACHE: OnceLock<Vec<ScaffoldMeta>> = OnceLock::new();
    CACHE.get_or_init(build_catalog).as_slice()
}

pub fn meta_for(id: &str) -> Option<&'static ScaffoldMeta> {
    catalog().iter().find(|m| m.id == id)
}

pub fn read(id: &str) -> Option<&'static str> {
    let meta = meta_for(id.trim())?;
    SCAFFOLDS_DIR
        .get_file(&meta.path)
        .and_then(|f| f.contents_utf8())
}

pub fn listing() -> String {
    let mut out = String::new();
    let mut current = "";
    for m in catalog() {
        if m.category != current {
            current = &m.category;
            out.push_str(&format!("\n[{current}]\n"));
        }
        out.push_str(&format!("- {} ({}) — {}\n", m.id, m.format, m.description));
    }
    out.trim_start().to_string()
}
