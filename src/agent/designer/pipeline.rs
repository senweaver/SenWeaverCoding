// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::params::{render_params_prompt, selected_prompt_template_block};
use super::submode::DesignerSubMode;
use serde_json::Value;

pub fn designer_session_dir(session_id: &str) -> String {
    let sanitized: String = session_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let sanitized = sanitized.trim_matches('-');
    let id = if sanitized.is_empty() { "default" } else { sanitized };
    format!(".senweavercoding/designer/{id}")
}

fn extract_figma_url(text: &str) -> Option<String> {
    let idx = text.find("figma.com/")?;
    let start = text[..idx]
        .rfind(|c: char| {
            c.is_whitespace() || matches!(c, '(' | '<' | '"' | '\'' | '`' | '[')
        })
        .map(|p| p + 1)
        .unwrap_or(0);
    let tail = &text[start..];
    let end = tail
        .find(|c: char| {
            c.is_whitespace() || matches!(c, ')' | '>' | '"' | '\'' | '`' | ']')
        })
        .unwrap_or(tail.len());
    let candidate = tail[..end].trim_end_matches(['.', ',', ';', '!', '?']);
    if candidate.contains("figma.com/") && candidate.len() > "figma.com/".len() {
        Some(candidate.to_string())
    } else {
        None
    }
}

pub const DESIGN_TASK_PREFIX: &str = "[Design task — EXCLUSIVE TASK FOR THIS TURN]";

fn unique_deck_dir_name() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("deck-{ms:x}")
}

pub fn list_existing_decks(
    workspace: &std::path::Path,
    session_id: &str,
) -> Vec<(String, String)> {
    let rel_base = designer_session_dir(session_id);
    let base = workspace.join(&rel_base);
    let Ok(entries) = std::fs::read_dir(&base) else {
        return Vec::new();
    };
    let mut decks: Vec<(std::time::SystemTime, String, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = path.join("deck.json");
        let Ok(meta) = std::fs::metadata(&manifest) else {
            continue;
        };
        let title = std::fs::read_to_string(&manifest)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| {
                v.get("title")
                    .and_then(|t| t.as_str())
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_default();
        let name = entry.file_name().to_string_lossy().to_string();
        let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        decks.push((mtime, format!("{rel_base}/{name}"), title));
    }
    decks.sort_by(|a, b| b.0.cmp(&a.0));
    decks
        .into_iter()
        .take(8)
        .map(|(_, dir, title)| (dir, title))
        .collect()
}

fn image_gen_tool_available() -> bool {
    #[cfg(feature = "tool-image")]
    {
        if let Some(svc) = crate::services::try_get_services() {
            let config = svc.config();
            if config.image_gen.enabled {
                return std::env::var(&config.image_gen.api_key_env)
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false);
            }
        }
    }
    false
}

fn media_image_available() -> bool {
    let Some(svc) = crate::services::try_get_services() else {
        return false;
    };
    let config = svc.config();
    let models = crate::tools::media::registry::default_models(
        crate::tools::media::MediaSurface::Image,
    );
    let Some(entries) = models.as_array() else {
        return false;
    };
    entries
        .iter()
        .filter_map(|m| m.get("provider").and_then(|v| v.as_str()))
        .any(|provider| crate::tools::media::credentials::provider_has_key(&config, provider))
}

fn param_str<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn media_tool_args_hint(sub: DesignerSubMode, params: &Value) -> Option<String> {
    let mut pairs: Vec<String> = Vec::new();
    match sub {
        DesignerSubMode::Image => {
            if let Some(aspect) = param_str(params, "aspect") {
                pairs.push(format!("`aspect={aspect}`"));
            }
            if let Some(res) = param_str(params, "resolution") {
                if res.eq_ignore_ascii_case("2k") || res.eq_ignore_ascii_case("4k") {
                    pairs.push(format!("`resolution={}`", res.to_ascii_lowercase()));
                }
            }
            if let Some(count) = param_str(params, "count") {
                if count != "1" {
                    pairs.push(format!("`count={count}`"));
                }
            }
        }
        DesignerSubMode::Video => {
            if let Some(aspect) = param_str(params, "aspect") {
                pairs.push(format!("`aspect={aspect}`"));
            }
            if let Some(len) = param_str(params, "length") {
                pairs.push(format!("`length={len}`"));
            }
        }
        DesignerSubMode::HyperFrames => {
            if let Some(aspect) = param_str(params, "aspect") {
                pairs.push(format!("`aspect={aspect}`"));
            }
            if let Some(len) = param_str(params, "length") {
                pairs.push(format!("`duration={len}`"));
            }
        }
        DesignerSubMode::Audio => {
            if let Some(kind) = param_str(params, "audioKind") {
                pairs.push(format!("`audio_kind={kind}`"));
            }
            if let Some(dur) = param_str(params, "duration") {
                pairs.push(format!("`duration={dur}`"));
            }
        }
        _ => return None,
    }
    if pairs.is_empty() {
        return None;
    }
    Some(format!(
        "Tool argument mapping (HARD): when calling `media_generate`, pass exactly {} — these come \
         from the user's selected parameters and must not be changed or omitted.",
        pairs.join(", ")
    ))
}

fn push_chart_palette_injection(out: &mut String, params: &Value, editing: bool) {
    let palette_id = params
        .get("chartPalette")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto");
    if editing {
        out.push_str(
            "Chart palette (edit mode): keep the existing chart's colors stable — only recolor when \
             the request explicitly asks for it",
        );
        match super::chart_palettes::palette_spec(palette_id) {
            Some(spec) => {
                out.push_str("; in that case apply the selected palette below:\n");
                out.push_str(&spec);
                out.push_str("\n\n");
            }
            None => out.push_str(
                "; in that case pick the best-fitting palette from the chart palette menu and \
                 follow its usage rules.\n\n",
            ),
        }
        return;
    }
    match super::chart_palettes::palette_spec(palette_id) {
        Some(spec) => {
            out.push_str(
                "Chart palette (HARD): the user selected the palette below. Apply it exactly — for \
                 ECharts set the top-level `color` array to the listed hex values verbatim; for \
                 Mermaid map the leading colors into `themeVariables`. An explicitly selected \
                 palette OVERRIDES any design-system-derived chart colors.\n",
            );
            out.push_str(&spec);
            out.push_str("\n\n");
        }
        None => {
            out.push_str(&format!(
                "Chart palette (auto): pick the single best-fitting palette for the data's nature \
                 from this menu: {}. Categorical palettes for discrete series, sequential for \
                 magnitude scales (heatmap/gauge fills), diverging for signed data centered on \
                 zero. Declare your choice in one sentence before writing the file, then follow \
                 that palette's usage rules; when an active design system is injected, its binding \
                 may win instead — prefer the design system for brand-locked work, the palette \
                 menu for general analytics.\n\n",
                super::chart_palettes::palette_menu(),
            ));
        }
    }
}

fn push_bi_style_injection(out: &mut String, params: &Value, editing: bool) {
    let style_id = params
        .get("biStyle")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto");
    if editing {
        out.push_str(
            "Visual style (edit mode): preserve the dashboard's existing visual language — match \
             the established palette tokens, panel chrome, chart treatment and composition already \
             in the artifact. Only restyle when the request explicitly asks for it",
        );
        match super::bi_styles::bi_style_spec(style_id) {
            Some(spec) => {
                out.push_str("; in that case apply the selected style below:\n");
                out.push_str(spec);
                out.push_str("\n\n");
            }
            None => out.push_str(
                "; in that case pick the best-fitting style from the BI style menu and follow \
                 its spec.\n\n",
            ),
        }
    } else {
        match super::bi_styles::bi_style_spec(style_id) {
            Some(spec) => {
                out.push_str(
                    "Visual style (HARD): the user selected the BI style below. Follow its palette, \
                     stage/layout system, panel chrome, chart treatment and motion rules on every \
                     panel of the artifact.\n",
                );
                out.push_str(spec);
                out.push_str("\n\n");
            }
            None => {
                out.push_str(&format!(
                    "Visual style (auto): pick the single best-fitting BI style for the brief's \
                     subject and audience from this menu: {}. Declare your choice in the plan, then \
                     follow that style's spec as if the user had selected it.\n\n",
                    super::bi_styles::style_menu(),
                ));
            }
        }
    }
}

fn push_deck_injections(out: &mut String, params: &Value, deck_dir: &str, editing: bool) {
    let style_id = params
        .get("deckStyle")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto");
    if editing {
        out.push_str(
            "Visual style (edit mode): preserve the deck's existing `theme` and visual language — \
             match the established palette tokens, typography roles and composition of the slides \
             already in the deck. Only restyle when the request explicitly asks for it",
        );
        match super::deck::styles::deck_style_spec(style_id) {
            Some(spec) => {
                out.push_str("; in that case apply the selected style below and set its theme id \
                     in deck.json:\n");
                out.push_str(spec);
                out.push_str("\n\n");
            }
            None => out.push_str(
                "; in that case pick the best-fitting style from the deck skill's theme menu and \
                 set its id in deck.json.\n\n",
            ),
        }
    } else {
        match super::deck::styles::deck_style_spec(style_id) {
            Some(spec) => {
                out.push_str(
                    "Visual style (HARD): the user selected the style below. Set the matching `theme` \
                     id in deck.json and follow the style's composition, token-usage and imagery rules \
                     on every slide.\n",
                );
                out.push_str(spec);
                out.push_str("\n\n");
            }
            None => {
                out.push_str(&format!(
                    "Visual style (auto): pick the single best-fitting style for the brief's subject \
                     and audience from this menu: {}. Declare your choice in the Stage 1 outline plan, \
                     set the matching `theme` id in deck.json, then follow that style's spec on every \
                     slide as if the user had selected it.\n\n",
                    super::deck::styles::style_menu(),
                ));
            }
        }
    }

    let imagery_pref = params
        .get("aiImagery")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto");
    let media_ok = media_image_available();
    let image_gen_ok = image_gen_tool_available();
    if imagery_pref.eq_ignore_ascii_case("none") || (!media_ok && !image_gen_ok) {
        out.push_str(
            "Imagery capability: image generation tools are NOT available for this task. Build \
             every visual moment with shape compositions, large typography and the theme palette — \
             the deck must look complete and intentional with zero raster images. Do NOT call \
             media_generate or image_gen, and do NOT reference image files that don't exist.\n\n",
        );
    } else {
        let tool_hint = if media_ok {
            "`media_generate surface=image`"
        } else {
            "`image_gen`"
        };
        let coverage = if imagery_pref.eq_ignore_ascii_case("rich") {
            "Coverage (rich): give the deck a full visual layer — atmospheric backgrounds for the \
             cover and every section-divider slide, PLUS a supporting `image` block on most \
             content slides where a visual genuinely strengthens the message (concept \
             illustration, product scene, metaphor, environment). Pair each image with the text \
             via a two-column arrangement or an inset panel anchored to the layout slots; the \
             image must never crowd out the slide's key points. Plan the imagery in Stage 1: mark \
             in the outline which slides get an image and what each image depicts."
        } else {
            "Coverage (auto): generate atmospheric backgrounds for the cover and section-divider \
             slides. Additionally, when a specific content slide clearly benefits from a visual \
             (a product shot, a scene, a concept that words undersell), you MAY add ONE supporting \
             `image` block to that slide — use this selectively (a few strongest moments, not \
             every slide), and never let an image displace the planned key points."
        };
        out.push_str(&format!(
            "Imagery capability: image generation IS available via {tool_hint} (imagery setting: \
             {imagery_pref}).\n{coverage}\n\
             Visual family (HARD): build every image prompt from the active style spec (palette, \
             mood, materials, lighting) plus the slide's subject so all images read as ONE family; \
             if the selected visual style forbids raster imagery, the style spec wins — use shape \
             and typography treatments instead.\n\
             Asset contract: save files under `{deck_dir}/assets/` and reference them from `image` \
             blocks (`src` relative to the deck directory, e.g. `assets/market-scene.png`, with \
             `fit` of `cover` or `contain`) or slide `background.image`. Generate each image \
             BEFORE writing the slide file that references it; never reference a file that does \
             not exist yet. One image per call; if any call fails or stalls, stop generating \
             images and fall back to shape/typography treatments immediately — imagery must never \
             block deck completion.\n\n"
        ));
    }

    if editing {
        out.push_str(&format!(
            "Execution order (HARD, targeted edit): Stage 1 read `{deck_dir}/deck.json` (slide \
             list, theme) and ONLY the slide files relevant to the request; Stage 2 post a short \
             edit plan (which files change and why); Stage 3 apply the minimal edits with \
             `file_write`, keeping every untouched slide file byte-for-byte unchanged and every \
             existing block `id` stable; Stage 4 run the `deck_compile` tool, fix EVERY P0 \
             finding, mark ALL todos completed via a final todo_write, then summarize the changed \
             slides. Do NOT rewrite unchanged slides, do NOT re-plan the whole deck, and do NOT \
             create a new deck directory. The canvas re-renders and `deck.pptx` regenerates \
             automatically after every write.\n\n",
        ));
    } else {
        out.push_str(&format!(
            "Execution order (HARD): follow the staged production protocol — Stage 1 post the full \
             per-slide outline plan as a chat message BEFORE writing any file; Stage 2 `file_write` \
             `{deck_dir}/deck.json` (title, theme, slide id list) plus the cover slide file; \
             Stage 3 write the remaining slides in batches of 2-4 `file_write` calls to \
             `{deck_dir}/slides/<id>.json`, honoring the plan slide by slide; Stage 4 run the \
             `deck_compile` tool, fix EVERY P0 finding (and any pending slide files) until the compile \
             is clean, mark ALL todos completed via a final todo_write (the task is not finished while \
             any todo stays open), then summarize naming `deck.pptx`. The canvas renders the deck live \
             from the spec files after every write, and `deck.pptx` is regenerated automatically — \
             there is no separate export step.\n\n",
        ));
    }
}

pub fn build_design_task_message(
    sub: DesignerSubMode,
    params: &Value,
    brief: &str,
    ref_artifact: Option<&str>,
    ref_element: Option<&str>,
    ref_element_label: Option<&str>,
    session_id: &str,
    existing_decks: &[(String, String)],
) -> String {
    let params_block = render_params_prompt(sub, params);
    let out_dir = designer_session_dir(session_id);
    let mut out = String::new();
    out.push_str(&format!(
        "{DESIGN_TASK_PREFIX} Produce a `{}` design ({}) following the \
         Designer pipeline (discovery → plan → generate → critique).\n\n\
         This task SUPERSEDES and VOIDS every earlier design task, brief, or stopped/unfinished \
         design work in this conversation and in any recalled memory. Do NOT resume, continue, or \
         restate an earlier design task; derive the subject EXCLUSIVELY from the content of THIS \
         message.\n\n",
        sub.label_en(),
        sub.id(),
    ));
    let editing = ref_artifact.map(str::trim).filter(|s| !s.is_empty());
    let element = ref_element.map(str::trim).filter(|s| !s.is_empty());
    let element_label = ref_element_label.map(str::trim).filter(|s| !s.is_empty());
    let deck_editing = editing
        .map(|t| t.replace('\\', "/"))
        .filter(|t| t.ends_with("/deck.json") || t == "deck.json");
    let image_editing = editing
        .map(|t| t.replace('\\', "/"))
        .filter(|t| {
            let lower = t.to_ascii_lowercase();
            [".png", ".jpg", ".jpeg", ".webp", ".gif", ".avif", ".bmp"]
                .iter()
                .any(|ext| lower.ends_with(ext))
        });
    let av_editing = editing
        .map(|t| t.replace('\\', "/"))
        .filter(|t| {
            let lower = t.to_ascii_lowercase();
            [
                ".mp4", ".webm", ".mov", ".m4v", ".mp3", ".wav", ".ogg", ".m4a", ".aac", ".flac",
            ]
            .iter()
            .any(|ext| lower.ends_with(ext))
        });
    let deck_dir_rel: Option<String> = if matches!(sub, DesignerSubMode::Deck) {
        Some(match deck_editing.as_deref() {
            Some(target) => target
                .trim_end_matches("deck.json")
                .trim_end_matches('/')
                .to_string(),
            None => format!("{out_dir}/{}", unique_deck_dir_name()),
        })
    } else {
        None
    };
    if let Some(target) = deck_editing.as_deref() {
        let deck_dir = target.trim_end_matches("deck.json").trim_end_matches('/');
        out.push_str(&format!(
            "Edit the existing slide deck whose manifest is `{target}` (referenced by the user \
             from the canvas). Read `{target}` FIRST to see the slide list and theme. Apply the \
             requested changes by editing ONLY the relevant files: a specific slide lives at \
             `{deck_dir}/slides/<id>.json`; deck-wide settings (title, theme, footer, slide order, \
             adding/removing slides) live in `{target}`. Preserve every untouched slide file \
             byte-for-byte. To add a slide, write the new `slides/<id>.json` file AND insert its id \
             into the `slides` array of `{target}` at the right position. After your edits, run \
             `deck_compile` and fix every P0 finding — the canvas and `deck.pptx` refresh \
             automatically from the spec files.\n\n"
        ));
        if let Some(el) = element {
            let label_hint = element_label
                .map(|l| format!(" The user knows this element as \"{l}\"."))
                .unwrap_or_default();
            if let Some(rest) = el.strip_prefix("deck:") {
                let mut parts = rest.splitn(2, ':');
                let slide_id = parts.next().unwrap_or("").trim();
                let block_id = parts.next().unwrap_or("").trim();
                if block_id.is_empty() {
                    out.push_str(&format!(
                        "Scope: the user selected slide `{slide_id}`.{label_hint} Focus the edit on \
                         `{deck_dir}/slides/{slide_id}.json` and keep every other slide file \
                         unchanged unless strictly required.\n\n"
                    ));
                } else {
                    out.push_str(&format!(
                        "Scope: the user selected the block `{block_id}` on slide `{slide_id}`.{label_hint} \
                         Open `{deck_dir}/slides/{slide_id}.json`, focus the edit on the block whose \
                         `id` is `{block_id}` (keep its `id` stable), and keep the rest of the deck \
                         unchanged unless strictly required.\n\n"
                    ));
                }
            }
        }
    } else if let Some(target) = image_editing.as_deref() {
        out.push_str(&format!(
            "Edit the existing IMAGE at `{target}` (referenced by the user from the canvas). This \
             is a raster image — NEVER try to rewrite its bytes with file tools. The editing \
             workflow is:\n\
             1. `view_image` `{target}` FIRST so you can see exactly what is in the source.\n\
             2. Apply the requested change with `media_generate surface=image` passing \
             `source_image={target}`:\n\
             - If a region mask is provided in the Scope below, ALSO pass the given `mask` path — \
             the white area of the mask is the ONLY region that gets repainted; describe in the \
             `prompt` the desired content of that region in the context of the full image (not a \
             standalone picture).\n\
             - Without a mask, perform a whole-image instruction edit: the `prompt` states the \
             requested change while everything not mentioned must stay faithful to the source \
             (pass `fidelity=high` unless the user asks for a loose reinterpretation).\n\
             3. The tool writes the result as a NEW file next to the source (the original is \
             preserved and both stay on the canvas). `view_image` the result to verify the edit \
             landed correctly — if it visibly failed the request, refine the prompt and retry \
             ONCE.\n\
             4. Summarize naming the new file path.\n\n"
        ));
        if let Some(el) = element {
            if let Some(rest) = el.strip_prefix("image-region:") {
                let mut parts = rest.splitn(2, ':');
                let coords = parts.next().unwrap_or("").trim();
                let mask_rel = parts.next().unwrap_or("").trim();
                let label_hint = element_label
                    .map(|l| format!(" The user describes this region as \"{l}\"."))
                    .unwrap_or_default();
                let coords_hint = {
                    let vals: Vec<f64> = coords
                        .split(',')
                        .filter_map(|v| v.trim().parse::<f64>().ok())
                        .collect();
                    if vals.len() == 4 {
                        format!(
                            "It covers approximately x={:.0}%, y={:.0}%, w={:.0}%, h={:.0}% of the image. ",
                            vals[0] * 100.0,
                            vals[1] * 100.0,
                            vals[2] * 100.0,
                            vals[3] * 100.0
                        )
                    } else {
                        String::new()
                    }
                };
                if mask_rel.is_empty() {
                    out.push_str(&format!(
                        "Scope: the user circled a region of the image.{label_hint} {coords_hint}\
                         No mask file is available — confine the whole-image instruction edit to \
                         that region by describing its location explicitly in the prompt, keeping \
                         the rest of the image unchanged.\n\n"
                    ));
                } else {
                    out.push_str(&format!(
                        "Scope (HARD): the user circled a region of the image and a repaint mask \
                         was saved at `{mask_rel}`.{label_hint} {coords_hint}You MUST pass \
                         `mask={mask_rel}` together with `source_image` to `media_generate` — the \
                         mask's white area marks the region to repaint and everything else must \
                         remain pixel-faithful. The `prompt` describes ONLY what the circled \
                         region should become, phrased in the context of the surrounding image.\n\n"
                    ));
                }
            }
        }
    } else if let Some(target) = av_editing.as_deref() {
        let kind = if [".mp4", ".webm", ".mov", ".m4v"]
            .iter()
            .any(|ext| target.to_ascii_lowercase().ends_with(ext))
        {
            "video"
        } else {
            "audio"
        };
        out.push_str(&format!(
            "Revise the existing {kind} at `{target}` (referenced by the user from the canvas). \
             This is a rendered media file — NEVER try to rewrite its bytes with file tools. \
             Produce the revision by calling `media_generate surface={kind}` with a prompt that \
             restates the original piece's intent PLUS the requested change (for HyperFrames \
             compositions, edit the composition HTML under `.hyperframes/` and re-render \
             instead). The tool writes the result as a NEW file with a descriptive name — the \
             original stays on the canvas for comparison. Summarize naming the new file path.\n\n"
        ));
    } else if let Some(target) = editing {
        out.push_str(&format!(
            "Edit the existing design unit at `{target}` (referenced by the user from the canvas). \
             Read that file FIRST, preserve its overall structure, layout and visual style, and \
             apply the requested changes IN PLACE by writing the modified content back to the SAME \
             file path `{target}`. Do NOT create a new file or rename it — editing in place lets \
             the canvas re-render this exact unit live. If you must add auxiliary assets, place \
             them inside `{out_dir}/`.\n\n"
        ));
        if let Some(el) = element {
            let label_hint = element_label
                .map(|l| format!(" The user knows this element as \"{l}\"."))
                .unwrap_or_default();
            if let Some(css) = el.strip_prefix("css:") {
                out.push_str(&format!(
                    "Scope: the user selected the element matching the CSS selector `{css}` (it has \
                     no `data-od-id` annotation yet).{label_hint} Locate that exact element in the file, focus \
                     the edit on THAT element (and its descendants) and keep the rest of the file \
                     byte-for-byte unchanged unless a change is strictly required to satisfy the \
                     request. While editing it, ADD a stable kebab-case `data-od-id` (plus a short \
                     `data-od-label`) to that element so future canvas point-selects can target it \
                     directly.\n\n"
                ));
            } else {
                out.push_str(&format!(
                    "Scope: the user selected the element annotated `data-od-id=\"{el}\"`.{label_hint} Focus the edit \
                     on THAT element (and its descendants) and keep the rest of the file byte-for-byte \
                     unchanged unless a change is strictly required to satisfy the request. Preserve the \
                     element's `data-od-id` so the canvas keeps tracking it.\n\n"
                ));
            }
        }
    } else if matches!(sub, DesignerSubMode::Deck) {
        let deck_dir = deck_dir_rel.as_deref().unwrap_or(out_dir.as_str());
        if !existing_decks.is_empty() {
            let mut listing = String::new();
            for (dir, title) in existing_decks {
                let shown = if title.is_empty() { "untitled" } else { title.as_str() };
                listing.push_str(&format!("- `{dir}/deck.json` — \"{shown}\"\n"));
            }
            out.push_str(&format!(
                "Edit-vs-new decision (FIRST, before any planning): this session already contains \
                 the following deck(s), most recently modified first:\n{listing}\
                 If the brief is a modification or iteration request aimed at one of these decks \
                 (changing specific slides or copy, adding/removing/reordering slides, switching \
                 theme or style, adjusting imagery — anything that refers to an existing deck, \
                 e.g. \"刚才的/之前的/这个\" deck or one of the titles above), treat this task as a \
                 TARGETED EDIT of that deck: read its `deck.json` FIRST, edit ONLY the relevant \
                 files inside that deck's directory following the 'Targeted edits' protocol and \
                 the targeted-edit execution order, run `deck_compile`, and IGNORE the new-deck \
                 output location and staged production protocol below. The supersede clause above \
                 voids earlier TASKS, not earlier ARTIFACTS — editing the deck the user refers to \
                 IS this task. Only create a new deck when the brief asks for a new presentation \
                 rather than changes to an existing one; when unsure between exactly two \
                 candidates, ask ONCE via `ask_question`.\n\n"
            ));
        }
        out.push_str(&format!(
            "Output location for a NEW deck (MANDATORY): create EVERY file produced for this deck \
             inside `{deck_dir}/` (path relative to the workspace root) — the manifest at \
             `{deck_dir}/deck.json`, slide files under `{deck_dir}/slides/`, and generated \
             imagery under `{deck_dir}/assets/`. This directory is reserved for THIS deck \
             generation run; earlier decks from this session live in sibling `deck-*` directories \
             and must NOT be touched (unless the edit-vs-new decision above resolved to editing \
             one of them). Do NOT write design files anywhere else in the workspace — this keeps \
             each deck unit isolated and rendered correctly in the canvas.\n\n"
        ));
    } else {
        out.push_str(&format!(
            "Output location (MANDATORY): create EVERY file produced for this design inside the \
             directory `{out_dir}/` (path relative to the workspace root). Create the directory if \
             it does not exist, and use descriptive file names within it (for example \
             `{out_dir}/<name>.html`). If a file with the same name already exists from an earlier \
             design in this session, pick a NEW distinct name instead of overwriting it — each \
             generation must remain its own preview unit. Do NOT write design files anywhere else \
             in the workspace — this keeps each session's design units isolated and rendered \
             correctly in the canvas.\n\n"
        ));
    }
    if matches!(sub, DesignerSubMode::FromFigma) {
        let figma_url = params
            .get("figmaUrl")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| extract_figma_url(brief));
        let frame_name = params
            .get("frameName")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        match figma_url {
            Some(url) => {
                out.push_str(&format!(
                    "Figma source (MANDATORY): fetch the real design from `{url}` with the \
                     `figma_fetch` tool before writing any markup — first `action=structure` to \
                     locate the target frame, then `action=node` for exact layout/fills/typography, \
                     then `action=image` to export a PNG reference and `view_image` it. Reproduce \
                     the fetched design faithfully; do NOT invent a design from the brief alone.\n"
                ));
                if let Some(frame) = frame_name {
                    out.push_str(&format!(
                        "Target frame: the user asked for the frame named \"{frame}\" — match it \
                         (case-insensitively, partial match allowed) in the structure outline.\n"
                    ));
                }
                out.push('\n');
            }
            None => {
                out.push_str(
                    "Figma source: the user did not provide a Figma URL. Ask for the share link \
                     with `ask_question` BEFORE generating anything — this sub-mode must reproduce \
                     a real Figma design, not invent one.\n\n",
                );
            }
        }
    }
    if !brief.trim().is_empty() {
        out.push_str("Brief:\n");
        out.push_str(brief.trim());
        out.push_str("\n\n");
        out.push_str(
            "Subject fidelity (HARD): the brief above is the authoritative subject of this design. \
             Produce the artifact for exactly this topic — never substitute an invented example \
             company, product, or dataset, and never ask the user to re-pick a topic the brief \
             already states. Selected parameters (narrative type, density, style, ...) shape the \
             STRUCTURE and STYLE only; when a parameter conflicts with the brief, the brief wins.\n\n",
        );
    }
    if sub.media_surface().is_none()
        && !matches!(sub, DesignerSubMode::Deck | DesignerSubMode::Diagram)
    {
        out.push_str(
            "Output size guard (HARD): build large HTML artifacts incrementally — first write the \
             skeleton (head, styles, scripts, shell with an end marker comment), then add content in \
             batches via `file_edit` (`mode=insert_before` the marker or `append`), keeping every \
             single tool call under ~250 lines. Never emit the full artifact as one monolithic \
             write.\n\n",
        );
    }
    if matches!(sub, DesignerSubMode::Deck) {
        let deck_dir = deck_dir_rel.as_deref().unwrap_or(out_dir.as_str());
        push_deck_injections(&mut out, params, deck_dir, deck_editing.is_some());
    }
    if matches!(sub, DesignerSubMode::LiveArtifact) {
        push_bi_style_injection(&mut out, params, editing.is_some());
    }
    if matches!(sub, DesignerSubMode::Diagram) {
        push_chart_palette_injection(&mut out, params, editing.is_some());
    }
    if !params_block.is_empty() {
        out.push_str(&params_block);
        out.push_str("\n\n");
    }
    if sub.media_surface().is_some() {
        if let Some(model) = params
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("auto"))
        {
            out.push_str(&format!(
                "Media model (selected in the composer model picker): {model}\n\
                 Pass this exact `model` to the `media_generate` tool. Do not substitute another model.\n\n"
            ));
        } else if matches!(sub, DesignerSubMode::HyperFrames) {
            out.push_str(
                "Media model (MANDATORY for HyperFrames): pass `model=hyperframes-html` to \
                 `media_generate` together with `composition_dir` — HyperFrames compositions render \
                 locally, never through a text-to-video provider.\n\n",
            );
        }
        if let Some(mapping) = media_tool_args_hint(sub, params) {
            out.push_str(&mapping);
            out.push_str("\n\n");
        }
    }
    if let Some(tpl_block) = selected_prompt_template_block(sub, params) {
        out.push_str(tpl_block.trim_start());
        out.push_str("\n\n");
    }
    if let Some(html_tpl_block) = super::params::selected_html_template_block(sub, params) {
        out.push_str(html_tpl_block.trim_start());
        out.push_str("\n\n");
    }
    out.push_str(
        "Write every produced asset to the project workspace so the Designer preview panel can render \
         it. For media surfaces, generate real files via the `media_generate` tool (reusing the \
         configured model providers). End with a short summary naming the primary artifact file.",
    );
    out
}
