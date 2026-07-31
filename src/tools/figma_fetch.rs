// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde_json::{json, Map, Value};

use super::traits::{Tool, ToolResult};

const FIGMA_API_BASE: &str = "https://api.figma.com/v1";
const MAX_OUTPUT_BYTES: usize = 60_000;
const MAX_NODE_DEPTH: usize = 12;
const CREDENTIAL_NAMES: &[&str] = &[
    "FIGMA_TOKEN",
    "FIGMA_PERSONAL_ACCESS_TOKEN",
    "FIGMA_API_TOKEN",
    "figma_token",
    "figma",
];
const ENV_NAMES: &[&str] = &[
    "FIGMA_TOKEN",
    "FIGMA_PERSONAL_ACCESS_TOKEN",
    "FIGMA_API_TOKEN",
];

pub struct FigmaFetchTool;

impl FigmaFetchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FigmaFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_token() -> Option<String> {
    if let Some(vault) = crate::services::governance::credential_vault::try_get_credential_vault()
    {
        for name in CREDENTIAL_NAMES {
            if let Some(v) = vault.get(name) {
                let v = v.trim().to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    for name in ENV_NAMES {
        if let Ok(v) = std::env::var(name) {
            let v = v.trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

fn missing_token_error() -> String {
    "No Figma access token configured. Save a personal access token in the credential vault under \
     the name `FIGMA_TOKEN` (Settings → Credentials), or export the `FIGMA_TOKEN` environment \
     variable, then retry. Tokens are created at figma.com → Settings → Security → Personal \
     access tokens."
        .to_string()
}

pub fn parse_figma_url(raw: &str) -> Option<(String, Option<String>)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let mut host_and_rest = without_scheme.splitn(2, '/');
    let host = host_and_rest.next().unwrap_or_default().to_ascii_lowercase();
    let rest = host_and_rest.next().unwrap_or_default();
    if !(host == "figma.com" || host.ends_with(".figma.com")) {
        return None;
    }
    let (path, query) = match rest.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (rest, None),
    };
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let key = match segments.as_slice() {
        [kind, key, ..]
            if matches!(*kind, "file" | "design" | "proto" | "board" | "slides") =>
        {
            (*key).to_string()
        }
        _ => return None,
    };
    if key.is_empty() {
        return None;
    }
    let node_id = query.and_then(|q| {
        q.split('&').find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            if k == "node-id" || k == "node_id" {
                let decoded = v.replace("%3A", ":").replace("%2D", "-");
                let normalized = if decoded.contains(':') {
                    decoded
                } else {
                    decoded.replace('-', ":")
                };
                let normalized = normalized.trim().to_string();
                if normalized.is_empty() {
                    None
                } else {
                    Some(normalized)
                }
            } else {
                None
            }
        })
    });
    Some((key, node_id))
}

fn normalize_node_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.contains(':') {
        trimmed.to_string()
    } else {
        trimmed.replace('-', ":")
    }
}

fn resolve_file_and_node(args: &Value) -> Result<(String, Option<String>), String> {
    let url = args.get("url").and_then(|v| v.as_str()).map(str::trim).unwrap_or("");
    let file_key = args
        .get("file_key")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    let explicit_node = args
        .get("node_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(normalize_node_id);
    if !file_key.is_empty() {
        return Ok((file_key.to_string(), explicit_node));
    }
    if !url.is_empty() {
        match parse_figma_url(url) {
            Some((key, url_node)) => return Ok((key, explicit_node.or(url_node))),
            None => {
                return Err(format!(
                    "Could not parse `{url}` as a Figma URL. Expected a link like \
                     https://www.figma.com/design/<file-key>/<title>?node-id=<id> (file/design/proto \
                     links are supported)."
                ))
            }
        }
    }
    Err("Provide either `url` (a Figma share link) or `file_key`.".to_string())
}

fn http_client(timeout_secs: u64) -> reqwest::Client {
    crate::services::require_services()
        .proxy_runtime()
        .build_client_with_timeouts("tool.figma_fetch", timeout_secs, 10)
}

async fn figma_get(token: &str, path_and_query: &str, timeout_secs: u64) -> Result<Value, String> {
    let url = format!("{FIGMA_API_BASE}{path_and_query}");
    let resp = http_client(timeout_secs)
        .get(&url)
        .header("X-Figma-Token", token)
        .send()
        .await
        .map_err(|e| format!("Figma API request failed: {e}"))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read Figma API response: {e}"))?;
    if !status.is_success() {
        let detail = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("err")
                    .or_else(|| v.get("message"))
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| body.chars().take(300).collect());
        let hint = match status.as_u16() {
            403 => " (invalid token, or the token lacks access to this file)",
            404 => " (file not found — check the file key and that your account can open it)",
            429 => " (rate limited — wait a moment and retry)",
            _ => "",
        };
        return Err(format!("Figma API returned {status}{hint}: {detail}"));
    }
    serde_json::from_str::<Value>(&body)
        .map_err(|e| format!("Figma API returned non-JSON payload: {e}"))
}

fn fmt_size(node: &Value) -> String {
    let bb = node.get("absoluteBoundingBox");
    let w = bb.and_then(|b| b.get("width")).and_then(|v| v.as_f64());
    let h = bb.and_then(|b| b.get("height")).and_then(|v| v.as_f64());
    match (w, h) {
        (Some(w), Some(h)) => format!(" {}x{}", w.round() as i64, h.round() as i64),
        _ => String::new(),
    }
}

fn outline_node(node: &Value, depth: usize, max_depth: usize, out: &mut String) {
    if depth > max_depth || out.len() > MAX_OUTPUT_BYTES {
        return;
    }
    let name = node.get("name").and_then(|v| v.as_str()).unwrap_or("(unnamed)");
    let ty = node.get("type").and_then(|v| v.as_str()).unwrap_or("NODE");
    let id = node.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    let indent = "  ".repeat(depth);
    out.push_str(&format!("{indent}- [{ty}] {name} (id `{id}`{})\n", fmt_size(node)));
    if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
        for child in children {
            outline_node(child, depth + 1, max_depth, out);
        }
    }
}

const NODE_KEEP_KEYS: &[&str] = &[
    "id",
    "name",
    "type",
    "absoluteBoundingBox",
    "layoutMode",
    "layoutWrap",
    "primaryAxisAlignItems",
    "counterAxisAlignItems",
    "primaryAxisSizingMode",
    "counterAxisSizingMode",
    "itemSpacing",
    "paddingLeft",
    "paddingRight",
    "paddingTop",
    "paddingBottom",
    "fills",
    "strokes",
    "strokeWeight",
    "strokeAlign",
    "cornerRadius",
    "rectangleCornerRadii",
    "effects",
    "opacity",
    "blendMode",
    "characters",
    "style",
    "characterStyleOverrides",
    "styleOverrideTable",
    "componentId",
    "clipsContent",
    "background",
    "backgroundColor",
    "constraints",
];

fn prune_node(node: &Value, depth: usize) -> Value {
    let Some(obj) = node.as_object() else {
        return node.clone();
    };
    let mut out = Map::new();
    for key in NODE_KEEP_KEYS {
        if let Some(v) = obj.get(*key) {
            if !v.is_null() {
                out.insert((*key).to_string(), v.clone());
            }
        }
    }
    if depth < MAX_NODE_DEPTH {
        if let Some(children) = obj.get("children").and_then(|v| v.as_array()) {
            let pruned: Vec<Value> =
                children.iter().map(|c| prune_node(c, depth + 1)).collect();
            if !pruned.is_empty() {
                out.insert("children".to_string(), Value::Array(pruned));
            }
        }
    } else if obj.get("children").is_some() {
        out.insert("childrenTruncated".to_string(), Value::Bool(true));
    }
    Value::Object(out)
}

fn collect_colors(node: &Value, colors: &mut std::collections::BTreeMap<String, usize>) {
    if let Some(obj) = node.as_object() {
        for key in ["fills", "strokes", "background"] {
            if let Some(paints) = obj.get(key).and_then(|v| v.as_array()) {
                for paint in paints {
                    let visible = paint
                        .get("visible")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    if !visible {
                        continue;
                    }
                    if paint.get("type").and_then(|v| v.as_str()) == Some("SOLID") {
                        if let Some(c) = paint.get("color") {
                            let r = (c.get("r").and_then(|v| v.as_f64()).unwrap_or(0.0) * 255.0)
                                .round() as u8;
                            let g = (c.get("g").and_then(|v| v.as_f64()).unwrap_or(0.0) * 255.0)
                                .round() as u8;
                            let b = (c.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0) * 255.0)
                                .round() as u8;
                            let a = paint
                                .get("opacity")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(1.0);
                            let hex = if a < 0.999 {
                                format!("#{r:02x}{g:02x}{b:02x} @ {:.0}%", a * 100.0)
                            } else {
                                format!("#{r:02x}{g:02x}{b:02x}")
                            };
                            *colors.entry(hex).or_insert(0) += 1;
                        }
                    }
                }
            }
        }
        if let Some(children) = obj.get("children").and_then(|v| v.as_array()) {
            for child in children {
                collect_colors(child, colors);
            }
        }
    }
}

fn collect_text_styles(node: &Value, styles: &mut std::collections::BTreeMap<String, usize>) {
    if let Some(obj) = node.as_object() {
        if obj.get("type").and_then(|v| v.as_str()) == Some("TEXT") {
            if let Some(style) = obj.get("style") {
                let family = style
                    .get("fontFamily")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let size = style
                    .get("fontSize")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let weight = style
                    .get("fontWeight")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(400.0);
                let line = style
                    .get("lineHeightPx")
                    .and_then(|v| v.as_f64())
                    .map(|v| format!("/{:.0}px", v))
                    .unwrap_or_default();
                let key = format!("{family} {:.0}px w{:.0}{line}", size, weight);
                *styles.entry(key).or_insert(0) += 1;
            }
        }
        if let Some(children) = obj.get("children").and_then(|v| v.as_array()) {
            for child in children {
                collect_text_styles(child, styles);
            }
        }
    }
}

fn truncate_output(mut s: String) -> String {
    if s.len() > MAX_OUTPUT_BYTES {
        let mut cut = MAX_OUTPUT_BYTES;
        while cut > 0 && !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("\n… (output truncated — narrow the request with `node_id` for full detail)");
    }
    s
}

fn workspace_dir() -> Result<std::path::PathBuf, String> {
    let session = crate::session::current_session_context()
        .ok_or_else(|| "No active session workspace.".to_string())?;
    Ok(std::path::PathBuf::from(session.workspace_dir))
}

fn sanitize_ref_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('-').to_string();
    if cleaned.is_empty() {
        "frame".to_string()
    } else {
        cleaned
    }
}

async fn run_structure(token: &str, key: &str, node_id: Option<&str>, depth: u64) -> Result<String, String> {
    let depth = depth.clamp(1, 6);
    let payload = if let Some(node) = node_id {
        figma_get(
            token,
            &format!("/files/{key}/nodes?ids={}&depth={depth}", urlencode(node)),
            45,
        )
        .await?
    } else {
        figma_get(token, &format!("/files/{key}?depth={depth}"), 45).await?
    };
    let file_name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("(unknown)");
    let mut out = format!("Figma file: {file_name} (key `{key}`)\n\n");
    if let Some(node) = node_id {
        let doc = payload
            .pointer(&format!("/nodes/{}/document", escape_pointer(node)))
            .ok_or_else(|| {
                format!("Node `{node}` not found in file `{key}` — check the node-id.")
            })?;
        out.push_str("Structure under the requested node:\n");
        outline_node(doc, 0, depth as usize + 2, &mut out);
    } else {
        let document = payload
            .get("document")
            .ok_or_else(|| "Figma response missing `document`.".to_string())?;
        out.push_str("Pages and top-level frames:\n");
        if let Some(pages) = document.get("children").and_then(|v| v.as_array()) {
            for page in pages {
                outline_node(page, 0, 2, &mut out);
            }
        }
        out.push_str(
            "\nNext: call `figma_fetch` with `action=node` and the target frame's `node_id` for \
             full layout/style detail, and `action=image` to export a reference render.",
        );
    }
    Ok(truncate_output(out))
}

async fn run_node(token: &str, key: &str, node_id: &str) -> Result<String, String> {
    let payload = figma_get(
        token,
        &format!("/files/{key}/nodes?ids={}", urlencode(node_id)),
        60,
    )
    .await?;
    let doc = payload
        .pointer(&format!("/nodes/{}/document", escape_pointer(node_id)))
        .ok_or_else(|| format!("Node `{node_id}` not found in file `{key}`."))?;
    let pruned = prune_node(doc, 0);

    let mut colors = std::collections::BTreeMap::new();
    collect_colors(doc, &mut colors);
    let mut text_styles = std::collections::BTreeMap::new();
    collect_text_styles(doc, &mut text_styles);

    let mut out = String::new();
    let name = doc.get("name").and_then(|v| v.as_str()).unwrap_or("(unnamed)");
    out.push_str(&format!("Node `{node_id}` — {name}{}\n\n", fmt_size(doc)));
    if !colors.is_empty() {
        let mut entries: Vec<(String, usize)> = colors.into_iter().collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1));
        out.push_str("Solid colors in use (count):\n");
        for (hex, count) in entries.iter().take(24) {
            out.push_str(&format!("- {hex} ×{count}\n"));
        }
        out.push('\n');
    }
    if !text_styles.is_empty() {
        let mut entries: Vec<(String, usize)> = text_styles.into_iter().collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1));
        out.push_str("Text styles in use (count):\n");
        for (style, count) in entries.iter().take(16) {
            out.push_str(&format!("- {style} ×{count}\n"));
        }
        out.push('\n');
    }
    out.push_str("Pruned node tree (layout, fills, typography):\n");
    out.push_str(
        &serde_json::to_string_pretty(&pruned).unwrap_or_else(|_| pruned.to_string()),
    );
    Ok(truncate_output(out))
}

async fn run_image(
    token: &str,
    key: &str,
    node_id: &str,
    scale: f64,
    dest: Option<&str>,
) -> Result<String, String> {
    let scale = if scale.is_finite() { scale.clamp(0.5, 4.0) } else { 2.0 };
    let payload = figma_get(
        token,
        &format!(
            "/images/{key}?ids={}&format=png&scale={scale}",
            urlencode(node_id)
        ),
        90,
    )
    .await?;
    if let Some(err) = payload.get("err").and_then(|v| v.as_str()) {
        if !err.is_empty() {
            return Err(format!("Figma image export failed: {err}"));
        }
    }
    let image_url = payload
        .pointer(&format!("/images/{}", escape_pointer(node_id)))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            format!("Figma did not return an image URL for node `{node_id}` (the node may be empty or unexported).")
        })?;

    let bytes = http_client(120)
        .get(image_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download exported image: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("Failed to read exported image body: {e}"))?;

    let workspace = workspace_dir()?;
    let rel_dir = crate::session::current_session_context()
        .map(|session| {
            format!(
                "{}/figma-refs",
                crate::agent::designer::pipeline::designer_session_dir(&session.session_id)
            )
        })
        .unwrap_or_else(|| ".senweavercoding/designer/figma-refs".to_string());
    let rel_path = match dest.map(str::trim).filter(|s| !s.is_empty()) {
        Some(d) => {
            let normalized = d.replace('\\', "/");
            if normalized.contains("..") || normalized.starts_with('/') {
                return Err("`dest` must be a workspace-relative path without `..`.".to_string());
            }
            normalized
        }
        None => format!("{rel_dir}/{}-{}.png", sanitize_ref_name(node_id), scale as u32),
    };
    let abs = workspace.join(&rel_path);
    if let Some(parent) = abs.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Could not create directory for `{rel_path}`: {e}"))?;
    }
    tokio::fs::write(&abs, &bytes)
        .await
        .map_err(|e| format!("Could not write `{rel_path}`: {e}"))?;
    crate::session::record_write_for_current_session(&abs);
    crate::agent::designer::record_artifact_if_designer(&abs);
    Ok(format!(
        "Exported node `{node_id}` at {scale}x to `{rel_path}` ({} KB). Use `view_image` on that \
         path to inspect it, and keep it as the visual reference while rebuilding the design.",
        bytes.len() / 1024,
    ))
}

fn urlencode(s: &str) -> String {
    s.replace(':', "%3A")
}

fn escape_pointer(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

#[async_trait]
impl Tool for FigmaFetchTool {
    fn name(&self) -> &str {
        "figma_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a design from Figma through the official REST API (Designer mode). Actions: \
         `structure` — list the file's pages and frames (or the subtree under `node_id`) with ids \
         and sizes; `node` — full layout/fill/typography detail for one frame plus a palette and \
         text-style digest; `image` — export a node as PNG into the workspace as a visual \
         reference. Accepts a `url` (any figma.com file/design/proto share link; `node-id` query \
         param is honored) or an explicit `file_key` + `node_id`. Requires a Figma personal access \
         token stored as the `FIGMA_TOKEN` credential or environment variable."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["structure", "node", "image"],
                    "description": "What to fetch: structure (page/frame outline), node (full design detail of one node), image (export PNG reference into the workspace)."
                },
                "url": {
                    "type": "string",
                    "description": "Figma share link, e.g. https://www.figma.com/design/<key>/<title>?node-id=1-2"
                },
                "file_key": {
                    "type": "string",
                    "description": "Figma file key (alternative to url)."
                },
                "node_id": {
                    "type": "string",
                    "description": "Node id like `1:2` (or `1-2`). Required for action=node and action=image when the url has no node-id."
                },
                "depth": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 6,
                    "description": "Tree depth for action=structure (default 2)."
                },
                "scale": {
                    "type": "number",
                    "minimum": 0.5,
                    "maximum": 4,
                    "description": "Export scale for action=image (default 2)."
                },
                "dest": {
                    "type": "string",
                    "description": "Optional workspace-relative output path for action=image (default .senweavercoding/designer/figma-refs/<node>.png)."
                }
            },
            "required": ["action"]
        })
    }

    fn cache_ttl_secs(&self) -> u64 {
        60
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("structure")
            .trim()
            .to_ascii_lowercase();

        let Some(token) = resolve_token() else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(missing_token_error()),
            });
        };

        let (file_key, node_id) = match resolve_file_and_node(&args) {
            Ok(v) => v,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e),
                });
            }
        };

        let result = match action.as_str() {
            "structure" => {
                let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(2);
                run_structure(&token, &file_key, node_id.as_deref(), depth).await
            }
            "node" => match node_id.as_deref() {
                Some(node) => run_node(&token, &file_key, node).await,
                None => Err(
                    "action=node requires a `node_id` (or a url containing `node-id=`). Run \
                     action=structure first to discover frame ids."
                        .to_string(),
                ),
            },
            "image" => match node_id.as_deref() {
                Some(node) => {
                    let scale = args.get("scale").and_then(|v| v.as_f64()).unwrap_or(2.0);
                    let dest = args.get("dest").and_then(|v| v.as_str());
                    run_image(&token, &file_key, node, scale, dest).await
                }
                None => Err(
                    "action=image requires a `node_id` (or a url containing `node-id=`). Run \
                     action=structure first to discover frame ids."
                        .to_string(),
                ),
            },
            other => Err(format!(
                "Unknown action `{other}`. Use one of: structure, node, image."
            )),
        };

        match result {
            Ok(output) => Ok(ToolResult {
                success: true,
                output,
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e),
            }),
        }
    }
}
