// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use include_dir::{include_dir, Dir};
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::OnceLock;

static DESIGN_SYSTEMS_DIR: Dir<'static> =
    include_dir!("$CARGO_MANIFEST_DIR/assets/design-systems");

#[derive(Debug, Clone, Serialize)]
pub struct DesignSystemMeta {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
}

fn read_member(id: &str, file: &str) -> Option<&'static str> {
    DESIGN_SYSTEMS_DIR
        .get_file(format!("{id}/{file}"))
        .and_then(|f| f.contents_utf8())
}

fn build_catalog() -> Vec<DesignSystemMeta> {
    let mut out: Vec<DesignSystemMeta> = Vec::new();
    for dir in DESIGN_SYSTEMS_DIR.dirs() {
        let Some(id) = dir
            .path()
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let Some(raw) = read_member(&id, "manifest.json") else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(raw) else {
            continue;
        };
        let name = manifest
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&id)
            .to_string();
        let category = manifest
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("Other")
            .to_string();
        let description = manifest
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        out.push(DesignSystemMeta {
            id,
            name,
            category,
            description,
        });
    }
    out.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });
    out
}

pub fn catalog() -> &'static [DesignSystemMeta] {
    static CACHE: OnceLock<Vec<DesignSystemMeta>> = OnceLock::new();
    CACHE.get_or_init(build_catalog).as_slice()
}

pub fn is_known(id: &str) -> bool {
    catalog().iter().any(|m| m.id == id)
}

pub fn name_for(id: &str) -> Option<&'static str> {
    catalog()
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.name.as_str())
}

const PUSHED_FILES: &[&str] = &["DESIGN.md", "tokens.css", "components.manifest.json"];

pub fn list_files(id: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(dir) = DESIGN_SYSTEMS_DIR.get_dir(id) else {
        return out;
    };
    collect_files(dir, id, &mut out);
    out.sort();
    out
}

fn collect_files(dir: &Dir<'static>, root: &str, out: &mut Vec<String>) {
    for f in dir.files() {
        if let Some(rel) = f
            .path()
            .strip_prefix(root)
            .ok()
            .and_then(|p| p.to_str())
        {
            out.push(rel.replace('\\', "/"));
        }
    }
    for sub in dir.dirs() {
        collect_files(sub, root, out);
    }
}

pub fn read_file(id: &str, rel_path: &str) -> Option<&'static str> {
    let rel = rel_path.trim().trim_start_matches('/').replace('\\', "/");
    if rel.is_empty() || rel.contains("..") {
        return None;
    }
    if !is_known(id) {
        return None;
    }
    read_member(id, &rel)
}

pub fn pull_index(id: &str) -> Option<String> {
    let trimmed = id.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") || !resolved_is_known(trimmed) {
        return None;
    }
    let title = resolved_name_for(trimmed).unwrap_or_else(|| trimmed.to_string());
    let extra: Vec<String> = resolved_list_files(trimmed)
        .into_iter()
        .filter(|p| !PUSHED_FILES.contains(&p.as_str()) && p.as_str() != "manifest.json")
        .collect();
    if extra.is_empty() {
        return None;
    }
    let listing = extra
        .iter()
        .map(|p| format!("- {p}"))
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "\n\n## Pull-layer files available on demand — {title}\n\n\
         This design-system package declares richer files for deeper inspection, source evidence, or \
         human preview. Keep the prompt light: use the index below to decide what to read later. When \
         you need a richer file, call the `design_system_read` tool with `id` = `{trimmed}` and `path` \
         set to one of the listed relative paths. Useful entries: `design-tokens.json` (machine-readable \
         tokens), `tailwind-v4.css` (Tailwind v4 theme), `components.html` (full worked fixture), \
         `source/evidence.md` (provenance).\n\n{listing}"
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenBinding {
    Html,
    Deck,
    Diagram,
    Media,
}

fn binding_for(sub: super::submode::DesignerSubMode) -> TokenBinding {
    use super::submode::DesignerSubMode as S;
    match sub {
        S::Deck => TokenBinding::Deck,
        S::Diagram => TokenBinding::Diagram,
        S::Image | S::Video | S::Audio => TokenBinding::Media,
        S::Prototype | S::LiveArtifact | S::HyperFrames | S::FromFigma | S::FromTemplate => {
            TokenBinding::Html
        }
    }
}

pub fn injection(id: &str, sub: super::submode::DesignerSubMode) -> Option<String> {
    let trimmed = id.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        return None;
    }
    if !resolved_is_known(trimmed) {
        return None;
    }
    let title = resolved_name_for(trimmed).unwrap_or_else(|| trimmed.to_string());
    let binding = binding_for(sub);

    let mut out = String::new();

    if let Some(design_md) = resolved_read_file(trimmed, "DESIGN.md") {
        let design_md = design_md.trim();
        if !design_md.is_empty() {
            out.push_str(&format!(
                "\n\n## Active design system — {title} (id: `{trimmed}`)\n\n\
                 Treat the following DESIGN.md as authoritative for color, typography, spacing, and \
                 component rules. Do not invent tokens outside this palette. When calling the \
                 `design_system_read` tool, pass `id` = `{trimmed}` exactly — never derive an id from \
                 the display name.\n\n{design_md}"
            ));
        }
    }

    let tokens_owned = resolved_read_file(trimmed, "tokens.css");
    let tokens = tokens_owned
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty());

    match binding {
        TokenBinding::Html => {
            if let Some(tokens) = tokens {
                out.push_str(&format!(
                    "\n\n## Active design system tokens — {title}\n\n\
                     The block below is this brand's tokens.css contract — every `:root` custom property \
                     and any scoped override the brand defines. Paste the unscoped `:root {{ ... }}` block \
                     verbatim into the artifact's first `<style>` so every `var(--*)` reference resolves at \
                     runtime. Do not invent new tokens. Do not redefine these values. Do not write raw hex \
                     outside this :root block. The DESIGN.md above is prose; this is the binding contract.\n\n\
                     ```css\n{tokens}\n```"
                ));
            }
            if let Some(components) = resolved_read_file(trimmed, "components.manifest.json") {
                let components = components.trim();
                if !components.is_empty() {
                    out.push_str(&format!(
                        "\n\n## Reference component manifest — {title}\n\n\
                         A compact structured summary derived from this brand's components.html fixture. Use it \
                         as the component inventory for generated artifacts: match the listed selectors, \
                         component groups, class names, token references, focus behavior, and spacing cadence. \
                         Prefer these manifest entries over inventing new component shapes.\n\n\
                         ```json\n{components}\n```"
                    ));
                }
            }
        }
        TokenBinding::Deck => {
            if let Some(tokens) = tokens {
                out.push_str(&format!(
                    "\n\n## Design system binding for slide decks — {title}\n\n\
                     Slide decks are compiled from JSON spec files, so CSS is never pasted anywhere. \
                     Bind this design system through the deck manifest instead (this is a HARD mapping, \
                     not a suggestion):\n\
                     - Extract the brand's primary accent hex from the tokens below (the `--accent` \
                     custom property or the DESIGN.md palette) and set it as `palette.accent` in \
                     `deck.json`. When the brief demands a full brand takeover, also map background, \
                     surface, text, muted and hairline to the matching `palette` keys.\n\
                     - Map the brand's heading and body font families onto `fonts.heading` and \
                     `fonts.body` in `deck.json`.\n\
                     - Keep using palette TOKEN names (accent, surface, muted, ...) inside slide files — \
                     the overrides in `deck.json` re-color every token reference deck-wide.\n\
                     - When generating imagery for the deck, build every image prompt from this brand's \
                     palette and mood so the visuals read on-brand.\n\n\
                     ```css\n{tokens}\n```"
                ));
            }
        }
        TokenBinding::Diagram => {
            if let Some(tokens) = tokens {
                out.push_str(&format!(
                    "\n\n## Design system binding for diagrams — {title}\n\n\
                     Diagram sources are Mermaid text, ECharts JSON, or mind-map markdown — there is no \
                     CSS channel. Bind this design system through each engine's own styling surface \
                     (HARD mapping):\n\
                     - ECharts (`.echarts.json`): derive the `color` series array (5-7 hues anchored on \
                     the brand accent from the tokens below), set `backgroundColor` from the brand \
                     background, and color axis labels/splitLines from the brand's fg/muted/border \
                     values. Pure JSON only — no functions.\n\
                     - Mermaid (`.mmd`): when the theme parameter is `default`, emit a first line \
                     `%%{{init: {{'theme':'base','themeVariables':{{'primaryColor':'<accent>',\
                     'primaryTextColor':'<fg>','lineColor':'<border>','fontFamily':'<body font>'}}}}}}%%` \
                     with values taken from the tokens below; an explicitly selected non-default theme \
                     parameter wins over this mapping.\n\
                     - Mind maps (`.mindmap.md`): plain markdown carries no styling — the design system \
                     does not restyle it; do not inject color syntax into the list.\n\n\
                     ```css\n{tokens}\n```"
                ));
            }
        }
        TokenBinding::Media => {
            out.push_str(&format!(
                "\n\n## Design system binding for generated media — {title}\n\n\
                 Media files are produced from text prompts, so CSS tokens cannot be pasted anywhere. \
                 Translate the design system into prompt language instead (HARD mapping):\n\
                 - Extract the brand's 3-5 dominant colors (background, accent, supporting hues) from \
                 the DESIGN.md palette above and state them explicitly in every `media_generate` prompt \
                 as the color grading / palette of the output.\n\
                 - Carry the brand's mood, materials, lighting and texture language (as described in \
                 DESIGN.md) into the scene description.\n\
                 - When the output contains typography (posters, UI mockups, title cards), describe the \
                 brand's type personality (serif/sans, weight, tracking) so rendered text matches.\n\
                 - Keep the mapping consistent across every asset generated in this session so the set \
                 reads as one brand family."
            ));
        }
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn library_store() -> Option<&'static crate::services::TemplateLibraryStore> {
    crate::services::try_get_services().map(|s| &s.template_library)
}

pub fn resolved_is_known(id: &str) -> bool {
    if is_known(id) {
        return true;
    }
    library_store()
        .map(|s| s.exists(&format!("design-systems/{id}/manifest.json")))
        .unwrap_or(false)
}

pub fn resolved_read_file(id: &str, rel_path: &str) -> Option<String> {
    let rel = rel_path.trim().trim_start_matches('/').replace('\\', "/");
    if rel.is_empty() || rel.contains("..") {
        return None;
    }
    if !resolved_is_known(id) {
        return None;
    }
    if let Some(store) = library_store() {
        if let Some(content) = store.read(&format!("design-systems/{id}/{rel}")) {
            return Some(content);
        }
    }
    read_member(id, &rel).map(str::to_string)
}

pub fn resolved_list_files(id: &str) -> Vec<String> {
    let mut set: BTreeSet<String> = list_files(id).into_iter().collect();
    if let Some(store) = library_store() {
        let prefix = format!("design-systems/{id}");
        let entry_prefix = format!("{prefix}/");
        for f in store.list_files(&prefix) {
            if let Some(rel) = f.strip_prefix(&entry_prefix) {
                set.insert(rel.to_string());
            }
        }
    }
    set.into_iter().collect()
}

fn resolved_meta(id: &str) -> Option<DesignSystemMeta> {
    if let Some(store) = library_store() {
        if let Some(raw) = store.read(&format!("design-systems/{id}/manifest.json")) {
            if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) {
                let name = manifest
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(id)
                    .to_string();
                let category = manifest
                    .get("category")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Other")
                    .to_string();
                let description = manifest
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                return Some(DesignSystemMeta {
                    id: id.to_string(),
                    name,
                    category,
                    description,
                });
            }
        }
    }
    catalog().iter().find(|m| m.id == id).cloned()
}

fn resolved_name_for(id: &str) -> Option<String> {
    resolved_meta(id).map(|m| m.name)
}

pub fn resolved_catalog() -> Vec<DesignSystemMeta> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<DesignSystemMeta> = Vec::new();
    for m in catalog() {
        seen.insert(m.id.clone());
        out.push(resolved_meta(&m.id).unwrap_or_else(|| m.clone()));
    }
    if let Some(store) = library_store() {
        let mut extra: Vec<String> = store
            .child_dirs("design-systems")
            .into_iter()
            .filter(|id| {
                !seen.contains(id)
                    && store.exists(&format!("design-systems/{id}/manifest.json"))
            })
            .collect();
        extra.sort();
        for id in extra {
            if let Some(meta) = resolved_meta(&id) {
                out.push(meta);
            }
        }
    }
    out
}
