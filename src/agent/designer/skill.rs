// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

static SKILLS_DIR: Dir<'static> =
    include_dir!("$CARGO_MANIFEST_DIR/assets/designer-skills");
static CRAFT_DIR: Dir<'static> =
    include_dir!("$CARGO_MANIFEST_DIR/assets/designer-craft");

#[derive(Debug, Clone, Serialize)]
pub struct SkillMeta {
    pub id: String,
    pub name: String,
    pub description: String,
    pub mode: String,
    pub craft_requires: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
    od: Option<Od>,
}

#[derive(Debug, Deserialize)]
struct Od {
    mode: Option<String>,
    craft: Option<Craft>,
}

#[derive(Debug, Deserialize)]
struct Craft {
    #[serde(default)]
    requires: Vec<String>,
}

fn read_skill_md(id: &str) -> Option<&'static str> {
    SKILLS_DIR
        .get_file(format!("{id}/SKILL.md"))
        .and_then(|f| f.contents_utf8())
}

fn split_frontmatter(md: &str) -> (Option<&str>, &str) {
    let trimmed = md.trim_start_matches('\u{feff}');
    let Some(rest) = trimmed.strip_prefix("---") else {
        return (None, md);
    };
    let rest = rest.trim_start_matches(['\r', '\n']);
    let Some(end) = rest.find("\n---") else {
        return (None, md);
    };
    let fm = &rest[..end];
    let after = &rest[end + 4..];
    let body = match after.split_once('\n') {
        Some((_, b)) => b,
        None => "",
    };
    (Some(fm), body.trim_start_matches(['\r', '\n']))
}

fn parse_frontmatter(fm: &str) -> Option<Frontmatter> {
    serde_yaml::from_str::<Frontmatter>(fm).ok()
}

fn build_catalog() -> Vec<SkillMeta> {
    let mut out: Vec<SkillMeta> = Vec::new();
    for dir in SKILLS_DIR.dirs() {
        let Some(id) = dir
            .path()
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let Some(raw) = read_skill_md(&id) else {
            continue;
        };
        let (fm, _) = split_frontmatter(raw);
        let parsed = fm.and_then(parse_frontmatter);
        let name = parsed
            .as_ref()
            .and_then(|f| f.name.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| id.clone());
        let description = parsed
            .as_ref()
            .and_then(|f| f.description.clone())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let od = parsed.as_ref().and_then(|f| f.od.as_ref());
        let mode = od
            .and_then(|o| o.mode.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "other".to_string());
        let craft_requires = od
            .and_then(|o| o.craft.as_ref())
            .map(|c| c.requires.clone())
            .unwrap_or_default();
        out.push(SkillMeta {
            id,
            name,
            description,
            mode,
            craft_requires,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn catalog() -> &'static [SkillMeta] {
    static CACHE: OnceLock<Vec<SkillMeta>> = OnceLock::new();
    CACHE.get_or_init(build_catalog).as_slice()
}

pub fn is_known(id: &str) -> bool {
    catalog().iter().any(|m| m.id == id)
}

pub fn meta_for(id: &str) -> Option<&'static SkillMeta> {
    catalog().iter().find(|m| m.id == id)
}

pub fn body(id: &str) -> Option<&'static str> {
    let raw = read_skill_md(id)?;
    let (_, body) = split_frontmatter(raw);
    if body.trim().is_empty() {
        None
    } else {
        Some(body)
    }
}

fn craft_read(name: &str) -> Option<&'static str> {
    let clean = name.trim().trim_end_matches(".md");
    CRAFT_DIR
        .get_file(format!("{clean}.md"))
        .and_then(|f| f.contents_utf8())
}

pub fn list_files(id: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(dir) = SKILLS_DIR.get_dir(id) else {
        return out;
    };
    collect_files(dir, id, &mut out);
    out.sort();
    out
}

fn collect_files(dir: &Dir<'static>, root: &str, out: &mut Vec<String>) {
    for f in dir.files() {
        if let Some(rel) = f.path().strip_prefix(root).ok().and_then(|p| p.to_str()) {
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
    SKILLS_DIR
        .get_file(format!("{id}/{rel}"))
        .and_then(|f| f.contents_utf8())
}

fn mode_for_submode(submode_id: &str) -> &'static str {
    match submode_id {
        "deck" => "deck",
        "image" => "image",
        "video" | "hyperframes" => "video",
        "audio" => "audio",
        "template" => "template",
        // prototype, live-artifact, figma all draw from prototype skills
        _ => "prototype",
    }
}

fn modes_for_submode(submode_id: &str) -> Vec<&'static str> {
    let primary = mode_for_submode(submode_id);
    let media = matches!(submode_id, "image" | "video" | "hyperframes" | "audio");
    let mut modes = vec![primary];
    if !media {
        modes.push("design-system");
    }
    modes.push("utility");
    modes
}

pub fn skills_for_submode(submode_id: &str) -> Vec<&'static SkillMeta> {
    let modes = modes_for_submode(submode_id);
    catalog()
        .iter()
        .filter(|m| modes.iter().any(|mode| *mode == m.mode))
        .collect()
}

pub fn optimal_skill_for_submode(submode_id: &str) -> Option<String> {
    let curated = match submode_id {
        "prototype" | "figma" => "frontend-design",
        "live-artifact" => "frontend-design",
        "deck" => "deck-swiss-international",
        "image" => "imagegen-frontend-web",
        "video" => "video-hyperframes",
        "hyperframes" => "video-hyperframes",
        "audio" => "speech",
        "template" => "frontend-design",
        _ => "",
    };
    if !curated.is_empty() && is_known(curated) {
        return Some(curated.to_string());
    }
    skills_for_submode(submode_id)
        .first()
        .map(|m| m.id.clone())
}

const SIDE_FILE_HINTS: &[&str] = &[
    "assets/template.html",
    "references/checklist.md",
    "template.html",
    "checklist.md",
];

fn derive_preflight(id: &str, skill_body: &str) -> String {
    let extra: Vec<String> = list_files(id)
        .into_iter()
        .filter(|p| p != "SKILL.md")
        .collect();
    if extra.is_empty() {
        return String::new();
    }
    let mentions_side = SIDE_FILE_HINTS.iter().any(|h| skill_body.contains(h))
        || skill_body.contains("template.html")
        || skill_body.contains("references/")
        || skill_body.contains("checklist");
    if !mentions_side {
        return String::new();
    }
    let listing = extra
        .iter()
        .map(|p| format!("- {p}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        " Before generating, read this skill's bundled seed files with the `designer_skill_read` \
         tool (`id` = `{id}`, `path` = one of the entries below) — start with any \
         `template.html` and `references/*.md` it references.\n\n\
         Skill files available on demand:\n{listing}"
    )
}

pub fn injection(id: &str) -> Option<String> {
    let trimmed = id.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") || !is_known(trimmed) {
        return None;
    }
    let meta = meta_for(trimmed)?;
    let mut out = String::new();

    if !meta.craft_requires.is_empty() {
        let mut craft_body = String::new();
        for name in &meta.craft_requires {
            if let Some(content) = craft_read(name) {
                let content = content.trim();
                if !content.is_empty() {
                    craft_body.push_str(&format!("\n\n### Craft — {name}\n\n{content}"));
                }
            }
        }
        if !craft_body.trim().is_empty() {
            out.push_str(&format!(
                "\n\n## Active craft references\n\n\
                 These are brand-agnostic craft rules the active skill requires. They sit ABOVE the \
                 design system: bind the brand tokens first, then apply this craft discipline to \
                 typography, color, spacing, motion, and anti-generic polish.{craft_body}"
            ));
        }
    }

    if let Some(skill_body) = body(trimmed) {
        let skill_body = skill_body.trim();
        if !skill_body.is_empty() {
            let preflight = derive_preflight(trimmed, skill_body);
            out.push_str(&format!(
                "\n\n## Active design skill — {name}\n\n\
                 Follow this skill's workflow exactly; it defines how to produce and self-critique \
                 this artifact at a professional bar.{preflight}\n\n{skill_body}",
                name = meta.name,
            ));
        }
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}
