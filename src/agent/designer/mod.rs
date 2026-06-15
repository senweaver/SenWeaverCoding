// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod assets;
pub mod bi_styles;
pub mod chart_palettes;
pub mod deck;
pub mod design_md;
pub mod design_system;
pub mod handoff;
pub mod html_template;
pub mod lint;
pub mod params;
pub mod pipeline;
pub mod prompt_template;
pub mod scaffold;
pub mod skill;
pub mod submode;

pub use submode::{DesignerSubMode, DesignerSurface};

use serde_json::Value;

pub fn designer_system_prompt_injection() -> String {
    let mut prompt = String::new();
    prompt.push_str(assets::DESIGNER_BASE_CONTRACT);
    prompt.push_str(assets::DESIGNER_PIPELINE_CONTRACT);

    let selection = active_designer_selection();
    let sub = selection
        .as_ref()
        .and_then(|(id, _)| DesignerSubMode::from_id(id));

    if let Some(sub) = sub {
        if sub.media_surface().is_some() {
            prompt.push_str(assets::DESIGNER_MEDIA_CONTRACT);
        } else if !matches!(sub, DesignerSubMode::Deck | DesignerSubMode::Diagram) {
            prompt.push_str(assets::DESIGNER_ANNOTATION_CONTRACT);
            prompt.push_str(assets::DESIGNER_TWEAKS_CONTRACT);
            prompt.push_str(assets::DESIGNER_SCAFFOLD_CONTRACT);
            if let Some(baton_block) = design_md::injection() {
                prompt.push_str(&baton_block);
            }
        }
        if matches!(sub, DesignerSubMode::HyperFrames) {
            prompt.push_str(assets::DESIGNER_HYPERFRAMES_CONTRACT);
        }
        prompt.push_str(assets::submode_skill(sub));
        let params_opt = selection.as_ref().map(|(_, params)| params);
        if let Some(params) = params_opt {
            if matches!(
                sub,
                DesignerSubMode::Prototype | DesignerSubMode::FromTemplate
            ) {
                if let Some(frame_block) = params
                    .get("platform")
                    .and_then(|v| v.as_str())
                    .and_then(assets::device_frame_contract)
                {
                    prompt.push_str(frame_block);
                }
            }
            let block = params::render_params_prompt(sub, params);
            if !block.is_empty() {
                prompt.push_str("\n");
                prompt.push_str(&block);
                prompt.push('\n');
            }
            if let Some(ds_id) = params.get("designSystem").and_then(|v| v.as_str()) {
                if let Some(ds_block) = design_system::injection(ds_id, sub) {
                    prompt.push_str(&ds_block);
                }
                if let Some(pull_block) = design_system::pull_index(ds_id) {
                    prompt.push_str(&pull_block);
                }
            }
            if let Some(tpl_block) = params::selected_prompt_template_block(sub, params) {
                prompt.push_str(&tpl_block);
            }
            if let Some(html_tpl_block) = params::selected_html_template_block(sub, params) {
                prompt.push_str(&html_tpl_block);
            }
        }
        if let Some(skill_id) = skill::optimal_skill_for_submode(sub.id()) {
            if let Some(skill_block) = skill::injection(&skill_id) {
                prompt.push_str(&skill_block);
            }
        }
    } else {
        prompt.push_str(assets::DESIGNER_MEDIA_CONTRACT);
        prompt.push_str(
            "\n### No sub-mode selected yet\n\
             Ask the user which of the design surfaces they want (prototype, BI dashboard, slide \
             deck, diagram, image, video, HyperFrames, audio, from Figma, from template), or \
             infer it from their brief, then proceed through the pipeline.\n",
        );
    }

    prompt
}

fn gateway_session_key(session_id: &str) -> String {
    format!("gw_{session_id}")
}

fn active_designer_selection() -> Option<(String, Value)> {
    let svc = crate::services::try_get_services()?;
    let session = crate::session::current_session_context()?;
    let selection = svc.session_designer(&gateway_session_key(&session.session_id))?;
    Some((selection.submode_id, selection.params))
}

fn strip_verbatim_prefix(path: &std::path::Path) -> std::path::PathBuf {
    let raw = path.to_string_lossy();
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        return std::path::PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = raw.strip_prefix(r"\\?\") {
        return std::path::PathBuf::from(rest);
    }
    path.to_path_buf()
}

fn workspace_relative_path(
    abs_path: &std::path::Path,
    workspace_dir: &std::path::Path,
) -> Option<String> {
    let abs = if abs_path.is_absolute() {
        abs_path.to_path_buf()
    } else {
        workspace_dir.join(abs_path)
    };
    let abs_canon = std::fs::canonicalize(&abs).unwrap_or(abs);
    let ws_canon =
        std::fs::canonicalize(workspace_dir).unwrap_or_else(|_| workspace_dir.to_path_buf());
    let abs_norm = strip_verbatim_prefix(&abs_canon);
    let ws_norm = strip_verbatim_prefix(&ws_canon);
    if let Ok(rel) = abs_norm.strip_prefix(&ws_norm) {
        return Some(rel.to_string_lossy().replace('\\', "/"));
    }
    if let Ok(rel) = abs_path.strip_prefix(workspace_dir) {
        return Some(rel.to_string_lossy().replace('\\', "/"));
    }
    None
}

fn classify_artifact_surface(path: &std::path::Path) -> Option<&'static str> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".mmd") || name.ends_with(".echarts.json") || name.ends_with(".mindmap.md") {
        return Some("diagram");
    }
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" | "svg" => Some("html"),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" => Some("image"),
        "mp4" | "webm" | "mov" | "m4v" => Some("video"),
        "mp3" | "wav" | "ogg" | "m4a" | "aac" | "flac" => Some("audio"),
        _ => None,
    }
}

pub fn record_artifact_if_designer(abs_path: &std::path::Path) {
    if !matches!(
        crate::agent::coding_mode::active_coding_mode(),
        crate::agent::coding_mode::CodingMode::Designer
    ) {
        return;
    }
    let Some(session) = crate::session::current_session_context() else {
        return;
    };
    let workspace_dir = std::path::Path::new(&session.workspace_dir);
    if let Some(deck_dir) = deck::compile::deck_dir_for_spec_path(abs_path) {
        let manifest = deck_dir.join(deck::compile::MANIFEST_FILE);
        let Some(rel_path) = workspace_relative_path(&manifest, workspace_dir) else {
            return;
        };
        if rel_path.is_empty() || !rel_path.contains(".senweavercoding/designer/") {
            return;
        }
        deck::compile::compile_deck_quiet(&deck_dir, workspace_dir);
        let session_key = gateway_session_key(&session.session_id);
        let submode = crate::services::try_get_services()
            .and_then(|svc| svc.session_designer(&session_key))
            .map(|s| s.submode_id);
        if let Some(backend) = crate::channels::session::global_session_backend() {
            let _ = backend.record_design_artifact(
                &session_key,
                &rel_path,
                submode.as_deref(),
                "deck",
            );
        }
        return;
    }
    let Some(surface) = classify_artifact_surface(abs_path) else {
        return;
    };
    let Some(rel_path) = workspace_relative_path(abs_path, workspace_dir) else {
        return;
    };
    if rel_path.is_empty() {
        return;
    }
    let session_key = gateway_session_key(&session.session_id);
    let submode = crate::services::try_get_services()
        .and_then(|svc| svc.session_designer(&session_key))
        .map(|s| s.submode_id);
    if let Some(backend) = crate::channels::session::global_session_backend() {
        let _ = backend.record_design_artifact(
            &session_key,
            &rel_path,
            submode.as_deref(),
            surface,
        );
    }
}
