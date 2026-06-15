// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::pipeline::designer_session_dir;

pub struct HandoffArtifact {
    pub rel_path: String,
    pub surface: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HandoffResult {
    #[serde(rename = "zipPath")]
    pub zip_path: String,
    #[serde(rename = "manifestPath")]
    pub manifest_path: String,
    #[serde(rename = "handoffPath")]
    pub handoff_path: String,
    #[serde(rename = "reactPaths")]
    pub react_paths: Vec<String>,
    #[serde(rename = "fileCount")]
    pub file_count: usize,
}

const RESPONSIVE_VIEWPORTS: &[u32] = &[360, 390, 414, 768, 834, 1024, 1280, 1440, 1920];
const INTERACTION_STATES: &[&str] = &[
    "default", "hover", "focus-visible", "active", "disabled", "loading", "empty", "error",
    "success",
];

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn file_stem(rel: &str) -> String {
    let base = rel.rsplit(['/', '\\']).next().unwrap_or(rel);
    base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base).to_string()
}

fn pascal_case(stem: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() {
            if upper {
                out.extend(ch.to_uppercase());
                upper = false;
            } else {
                out.push(ch);
            }
        } else {
            upper = true;
        }
    }
    if out.is_empty() || out.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        format!("Design{out}")
    } else {
        out
    }
}

fn build_manifest(session_id: &str, artifacts: &[HandoffArtifact]) -> String {
    #[derive(Serialize)]
    struct Entry<'a> {
        #[serde(rename = "relPath")]
        rel_path: &'a str,
        surface: &'a str,
    }
    let entries: Vec<Entry> = artifacts
        .iter()
        .map(|a| Entry {
            rel_path: &a.rel_path,
            surface: &a.surface,
        })
        .collect();
    let manifest = serde_json::json!({
        "name": "SenWeaverCoding design handoff",
        "session": session_id,
        "generatedAt": now_millis(),
        "artifacts": entries,
        "responsiveViewports": RESPONSIVE_VIEWPORTS,
        "interactionStates": INTERACTION_STATES,
    });
    serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".to_string())
}

fn build_handoff_md(session_id: &str, artifacts: &[HandoffArtifact]) -> String {
    let mut md = String::new();
    md.push_str("# Design handoff\n\n");
    md.push_str(&format!("Session: `{session_id}`\n\n"));
    md.push_str("This package was produced by SenWeaverCoding Designer mode. It bundles the \
        generated artifacts, a machine-readable `DESIGN-MANIFEST.json`, and React wrappers for the \
        HTML surfaces so an implementer can rebuild the design verbatim.\n\n");

    md.push_str("## Artifacts\n\n");
    if artifacts.is_empty() {
        md.push_str("_No artifacts recorded for this session yet._\n\n");
    } else {
        md.push_str("| File | Surface |\n|---|---|\n");
        for a in artifacts {
            md.push_str(&format!("| `{}` | {} |\n", a.rel_path, a.surface));
        }
        md.push('\n');
    }

    md.push_str("## Responsive viewport matrix\n\n");
    md.push_str("Verify the layout reflows correctly at each width:\n\n");
    for w in RESPONSIVE_VIEWPORTS {
        md.push_str(&format!("- {w}px\n"));
    }
    md.push('\n');

    md.push_str("## Interaction state checklist\n\n");
    for s in INTERACTION_STATES {
        md.push_str(&format!("- [ ] {s}\n"));
    }
    md.push('\n');

    md.push_str("## Implementation checklist\n\n");
    for item in [
        "Bind the design-system tokens into `:root` before composing layout.",
        "Reference `var(--*)` tokens — no raw hex outside `:root`.",
        "Cover every interaction state listed above.",
        "Confirm WCAG AA contrast, visible focus rings, and `prefers-reduced-motion`.",
        "Keep `data-od-id` annotations so future targeted edits stay precise.",
        "Run `designer_lint` and resolve every P0 finding.",
    ] {
        md.push_str(&format!("- [ ] {item}\n"));
    }
    md.push('\n');

    md.push_str("## React / PDF / PPTX\n\n");
    md.push_str("- React: see the `react/` folder — each HTML surface ships a self-contained \
        component you can drop into a React app.\n");
    md.push_str("- PDF: open the HTML artifact in the in-app browser and print to PDF, or run a \
        headless Chrome print on the file.\n");
    md.push_str("- PPTX: deck surfaces ship the final `deck.pptx` next to their spec files — it is \
        recompiled automatically on every spec change, so the bundled copy is always current.\n");
    md
}

fn build_react_component(rel_path: &str, html: &str) -> String {
    let component = pascal_case(&file_stem(rel_path));
    let escaped = html.replace('\\', "\\\\").replace('`', "\\`").replace("${", "\\${");
    format!(
        "// SPDX-License-Identifier: MIT\n\
         // Copyright (c) 2025-2026 SenWeaverCoding\n\
         // Auto-generated React wrapper for `{rel_path}`.\n\n\
         const __HTML__ = `{escaped}`;\n\n\
         export default function {component}() {{\n\
         \u{20}\u{20}return (\n\
         \u{20}\u{20}\u{20}\u{20}<iframe\n\
         \u{20}\u{20}\u{20}\u{20}\u{20}\u{20}title=\"{component}\"\n\
         \u{20}\u{20}\u{20}\u{20}\u{20}\u{20}srcDoc={{__HTML__}}\n\
         \u{20}\u{20}\u{20}\u{20}\u{20}\u{20}style={{{{ width: '100%', height: '100%', border: 0 }}}}\n\
         \u{20}\u{20}\u{20}\u{20}/>\n\
         \u{20}\u{20});\n\
         }}\n"
    )
}

fn add_zip_file<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    name: &str,
    bytes: &[u8],
) -> std::io::Result<()> {
    let options: zip::write::SimpleFileOptions =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(name, options)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    zip.write_all(bytes)?;
    Ok(())
}

fn add_zip_dir<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    dir: &Path,
    prefix: &str,
) -> std::io::Result<usize> {
    let mut count = 0usize;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(0);
    };
    let mut items: Vec<std::fs::DirEntry> = entries.flatten().collect();
    items.sort_by_key(|e| e.file_name());
    for entry in items {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if path.is_dir() {
            count += add_zip_dir(zip, &path, &format!("{prefix}/{name}"))?;
        } else if path.is_file() {
            if let Ok(bytes) = std::fs::read(&path) {
                add_zip_file(zip, &format!("{prefix}/{name}"), &bytes)?;
                count += 1;
            }
        }
    }
    Ok(count)
}

pub fn build_handoff(
    session_id: &str,
    work_dir: &Path,
    artifacts: &[HandoffArtifact],
) -> std::io::Result<HandoffResult> {
    let session_dir_rel = designer_session_dir(session_id);
    let session_dir = work_dir.join(&session_dir_rel);
    let handoff_dir = session_dir.join("handoff");
    let react_dir = session_dir.join("react");
    std::fs::create_dir_all(&handoff_dir)?;

    let manifest = build_manifest(session_id, artifacts);
    let handoff_md = build_handoff_md(session_id, artifacts);
    let manifest_rel = format!("{session_dir_rel}/DESIGN-MANIFEST.json");
    let handoff_rel = format!("{session_dir_rel}/DESIGN-HANDOFF.md");
    std::fs::write(work_dir.join(&manifest_rel), manifest.as_bytes())?;
    std::fs::write(work_dir.join(&handoff_rel), handoff_md.as_bytes())?;

    let mut react_paths: Vec<String> = Vec::new();
    let mut react_files: Vec<(String, String)> = Vec::new();
    for a in artifacts {
        if a.surface != "html" {
            continue;
        }
        let abs = work_dir.join(a.rel_path.trim_start_matches(['/', '\\']));
        let Ok(html) = std::fs::read_to_string(&abs) else {
            continue;
        };
        let component = build_react_component(&a.rel_path, &html);
        let name = format!("{}.jsx", pascal_case(&file_stem(&a.rel_path)));
        let out_rel = format!("{session_dir_rel}/react/{name}");
        std::fs::create_dir_all(&react_dir)?;
        std::fs::write(work_dir.join(&out_rel), component.as_bytes())?;
        react_paths.push(out_rel.clone());
        react_files.push((format!("react/{name}"), component));
    }

    let ts = now_millis();
    let zip_rel = format!("{session_dir_rel}/handoff/handoff-{ts}.zip");
    let zip_abs = work_dir.join(&zip_rel);
    let file = std::fs::File::create(&zip_abs)?;
    let mut zip = zip::ZipWriter::new(file);
    let mut file_count = 0usize;

    add_zip_file(&mut zip, "DESIGN-MANIFEST.json", manifest.as_bytes())?;
    add_zip_file(&mut zip, "DESIGN-HANDOFF.md", handoff_md.as_bytes())?;
    file_count += 2;
    for (name, body) in &react_files {
        add_zip_file(&mut zip, name, body.as_bytes())?;
        file_count += 1;
    }
    for a in artifacts {
        let normalized = a.rel_path.trim_start_matches(['/', '\\']).replace('\\', "/");
        let abs = work_dir.join(&normalized);
        if !abs.is_file() {
            continue;
        }
        let is_deck_manifest = normalized
            .rsplit('/')
            .next()
            .map(|n| n.eq_ignore_ascii_case("deck.json"))
            .unwrap_or(false);
        if is_deck_manifest {
            if let Some(deck_dir) = abs.parent() {
                let deck_dir_rel = normalized
                    .rsplit_once('/')
                    .map(|(dir, _)| dir.to_string())
                    .unwrap_or_default();
                file_count +=
                    add_zip_dir(&mut zip, deck_dir, &format!("artifacts/{deck_dir_rel}"))?;
            }
            continue;
        }
        if let Ok(bytes) = std::fs::read(&abs) {
            let zip_name = format!("artifacts/{normalized}");
            add_zip_file(&mut zip, &zip_name, &bytes)?;
            file_count += 1;
        }
    }
    zip.finish()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let _ = PathBuf::from(&session_dir);
    Ok(HandoffResult {
        zip_path: zip_rel,
        manifest_path: manifest_rel,
        handoff_path: handoff_rel,
        react_paths,
        file_count,
    })
}
