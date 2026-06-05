// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::AppState;
use super::api::require_auth;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use base64::Engine;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const DEFAULT_TREE_DEPTH: u32 = 1;
const MAX_TREE_DEPTH: u32 = 6;
const MAX_TREE_NODES: usize = 5_000;
const MAX_FILE_READ_BYTES: u64 = 4 * 1024 * 1024;
const MAX_UPLOAD_BYTES: usize = 10 * 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 500;

const HIDDEN_DEFAULT_DIRS: &[&str] = &[];

#[derive(Debug)]
enum FsError {
    NotFound,
    OutsideRoot,
    InvalidName,
    InvalidRoot,
    Io(std::io::Error),
    TooLarge(u64),
}

impl FsError {
    fn into_response(self) -> axum::response::Response {
        match self {
            FsError::NotFound => (StatusCode::NOT_FOUND, Json(json!({"error": "Not found"}))).into_response(),
            FsError::OutsideRoot => (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Path is outside the workspace root"})),
            )
                .into_response(),
            FsError::InvalidName => (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid path or name"})),
            )
                .into_response(),
            FsError::InvalidRoot => (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Workspace root is not in the allowed list"
                })),
            )
                .into_response(),
            FsError::TooLarge(size) => (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({
                    "error": "File too large",
                    "size": size,
                })),
            )
                .into_response(),
            FsError::Io(e) => {
                tracing::warn!(err = %e, "workspace fs error");
                let kind = e.kind();
                let status = match kind {
                    std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
                    std::io::ErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
                    std::io::ErrorKind::AlreadyExists => StatusCode::CONFLICT,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, Json(json!({"error": e.to_string()}))).into_response()
            }
        }
    }
}

impl From<std::io::Error> for FsError {
    fn from(value: std::io::Error) -> Self {
        FsError::Io(value)
    }
}

fn session_allowed_workspace_canonicals(state: &AppState) -> Vec<PathBuf> {
    let Some(ref backend) = state.session_backend else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = backend
        .list_sessions_with_metadata()
        .into_iter()
        .filter_map(|meta| {
            let wd = meta.work_dir.as_deref()?.trim();
            if wd.is_empty() {
                return None;
            }
            let p = PathBuf::from(wd);
            p.canonicalize().ok()
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

async fn allowed_workspace_root(state: &AppState, requested: &str) -> Result<PathBuf, FsError> {
    let state = state.clone();
    let requested = requested.to_string();
    tokio::task::spawn_blocking(move || {
        let workspace = state.config.lock().workspace_dir.clone();
        let workspace_canonical = workspace.canonicalize().map_err(|_| FsError::InvalidRoot)?;
        let requested = PathBuf::from(&requested);
        let requested_canonical = requested.canonicalize().map_err(|_| FsError::InvalidRoot)?;
        if requested_canonical == workspace_canonical {
            return Ok(workspace_canonical);
        }
        for root in session_allowed_workspace_canonicals(&state) {
            if root == requested_canonical {
                return Ok(requested_canonical);
            }
        }
        Err(FsError::InvalidRoot)
    })
    .await
    .map_err(|_| FsError::InvalidRoot)?
}

fn resolve_within(root: &Path, rel: &str, must_exist: bool) -> Result<PathBuf, FsError> {
    let rel = rel.trim();
    if rel.is_empty() || rel == "." {
        if !must_exist {
            return Err(FsError::InvalidName);
        }
        return Ok(root.to_path_buf());
    }
    let rel_path = PathBuf::from(rel.trim_start_matches(['/', '\\']));
    if rel_path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(FsError::OutsideRoot);
    }
    let joined = root.join(&rel_path);

    let resolved = if joined.exists() {
        joined.canonicalize().map_err(FsError::Io)?
    } else {
        if must_exist {
            return Err(FsError::NotFound);
        }
        let parent = joined.parent().ok_or(FsError::InvalidName)?;
        let parent_canonical = parent.canonicalize().map_err(|_| FsError::NotFound)?;
        let file_name = joined.file_name().ok_or(FsError::InvalidName)?;
        parent_canonical.join(file_name)
    };

    if !resolved.starts_with(root) {
        return Err(FsError::OutsideRoot);
    }
    Ok(resolved)
}

fn relative_path(root: &Path, target: &Path) -> String {
    target
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn modified_at(metadata: &std::fs::Metadata) -> Option<String> {
    let modified = metadata.modified().ok()?;
    let datetime: chrono::DateTime<chrono::Utc> = modified.into();
    Some(datetime.to_rfc3339())
}

fn entry_to_json(root: &Path, path: &Path, name: &str, is_dir: bool) -> serde_json::Value {
    let mut payload = json!({
        "name": name,
        "relPath": relative_path(root, path),
        "isDir": is_dir,
    });
    if let Ok(metadata) = std::fs::metadata(path) {
        if let Some(map) = payload.as_object_mut() {
            if !is_dir {
                map.insert("sizeBytes".into(), json!(metadata.len()));
            }
            if let Some(ts) = modified_at(&metadata) {
                map.insert("modifiedAt".into(), json!(ts));
            }
        }
    }
    payload
}

fn is_hidden_default(name: &str) -> bool {
    HIDDEN_DEFAULT_DIRS.contains(&name)
}

#[derive(Debug, Deserialize)]
pub struct TreeQuery {
    pub root: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub depth: Option<u32>,
    #[serde(default, rename = "showHidden")]
    pub show_hidden: Option<bool>,
}

pub async fn handle_workspace_tree(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<TreeQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &q.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let rel = q.path.as_deref().unwrap_or("");
    let target = match resolve_within(&root, rel, true) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };
    let depth = q
        .depth
        .unwrap_or(DEFAULT_TREE_DEPTH)
        .clamp(0, MAX_TREE_DEPTH);
    let show_hidden = q.show_hidden.unwrap_or(false);

    fn collect(
        root: &Path,
        dir: &Path,
        depth: u32,
        show_hidden: bool,
        budget: &mut usize,
    ) -> std::io::Result<Vec<serde_json::Value>> {
        let mut entries = Vec::new();
        let mut read = std::fs::read_dir(dir)?
            .filter_map(|res| res.ok())
            .collect::<Vec<_>>();
        read.sort_by(|a, b| {
            let a_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let b_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
            b_dir.cmp(&a_dir).then(
                a.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .cmp(&b.file_name().to_string_lossy().to_lowercase()),
            )
        });
        for entry in read {
            if *budget == 0 {
                break;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !show_hidden && is_hidden_default(&name) {
                continue;
            }
            let path = entry.path();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let mut node = entry_to_json(root, &path, &name, is_dir);
            *budget = budget.saturating_sub(1);
            if is_dir && depth > 0 {
                if let Ok(children) = collect(root, &path, depth - 1, show_hidden, budget) {
                    if let Some(map) = node.as_object_mut() {
                        map.insert("children".into(), json!(children));
                        map.insert("loaded".into(), json!(true));
                    }
                }
            } else if is_dir {
                if let Some(map) = node.as_object_mut() {
                    map.insert("loaded".into(), json!(false));
                }
            }
            entries.push(node);
        }
        Ok(entries)
    }

    let initial_depth = depth.saturating_sub(1);
    let root_for_io = root.clone();
    let target_for_io = target.clone();
    let result: Result<(Vec<serde_json::Value>, bool), FsError> =
        tokio::task::spawn_blocking(move || {
            if !target_for_io.is_dir() {
                return Err(FsError::InvalidName);
            }
            let mut budget = MAX_TREE_NODES;
            let children = collect(
                &root_for_io,
                &target_for_io,
                initial_depth,
                show_hidden,
                &mut budget,
            )
            .map_err(FsError::Io)?;
            Ok((children, budget == 0))
        })
        .await
        .unwrap_or_else(|join_err| {
            Err(FsError::Io(std::io::Error::other(join_err.to_string())))
        });

    match result {
        Ok((children, truncated)) => Json(json!({
            "root": root.to_string_lossy(),
            "relPath": relative_path(&root, &target),
            "entries": children,
            "truncated": truncated,
        }))
        .into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct FileGetQuery {
    pub root: String,
    pub path: String,
}

pub async fn handle_workspace_file_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<FileGetQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &q.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let target = match resolve_within(&root, &q.path, true) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };
    if !target.is_file() {
        return FsError::InvalidName.into_response();
    }
    let target_for_io = target.clone();
    let read_result: Result<(std::fs::Metadata, Vec<u8>), FsError> =
        tokio::task::spawn_blocking(move || {
            let metadata =
                std::fs::metadata(&target_for_io).map_err(FsError::Io)?;
            if metadata.len() > MAX_FILE_READ_BYTES {
                return Err(FsError::TooLarge(metadata.len()));
            }
            let bytes = std::fs::read(&target_for_io).map_err(FsError::Io)?;
            Ok((metadata, bytes))
        })
        .await
        .unwrap_or_else(|e| {
            Err(FsError::Io(std::io::Error::other(format!(
                "blocking task join failed: {e}"
            ))))
        });
    let (metadata, bytes) = match read_result {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let modified = modified_at(&metadata);
    let mime = mime_from_extension(&target);
    let payload = match classify_file_content(&target, &bytes) {
        Some(text) => json!({
            "content": text,
            "encoding": "utf8",
            "isBinary": false,
            "sizeBytes": metadata.len(),
            "modifiedAt": modified,
            "mimeType": mime,
        }),
        None => json!({
            "content": base64::engine::general_purpose::STANDARD.encode(&bytes),
            "encoding": "base64",
            "isBinary": true,
            "sizeBytes": metadata.len(),
            "modifiedAt": modified,
            "mimeType": mime,
        }),
    };
    Json(payload).into_response()
}

fn classify_file_content(path: &Path, bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return Some(String::new());
    }
    if let Some(text) = decode_with_bom(bytes) {
        return Some(strip_bom_prefix(text));
    }
    if looks_text_by_content(bytes) {
        return Some(decode_text_best_effort(bytes));
    }
    if is_known_text_path(path) {
        if let Some(text) = try_decode_utf16_no_bom(bytes) {
            return Some(strip_bom_prefix(text));
        }
        return Some(decode_text_best_effort(bytes));
    }
    None
}

fn decode_with_bom(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(b"\xEF\xBB\xBF") {
        return Some(String::from_utf8_lossy(&bytes[3..]).into_owned());
    }
    if bytes.starts_with(b"\xFF\xFE\x00\x00") || bytes.starts_with(b"\x00\x00\xFE\xFF") {
        return None;
    }
    if bytes.starts_with(b"\xFF\xFE") {
        let (decoded, _, had_errors) = encoding_rs::UTF_16LE.decode(&bytes[2..]);
        if had_errors {
            return None;
        }
        return Some(decoded.into_owned());
    }
    if bytes.starts_with(b"\xFE\xFF") {
        let (decoded, _, had_errors) = encoding_rs::UTF_16BE.decode(&bytes[2..]);
        if had_errors {
            return None;
        }
        return Some(decoded.into_owned());
    }
    None
}

fn strip_bom_prefix(s: String) -> String {
    if let Some(rest) = s.strip_prefix('\u{FEFF}') {
        rest.to_string()
    } else {
        s
    }
}

fn looks_text_by_content(bytes: &[u8]) -> bool {
    let sniff_len = bytes.len().min(8_192);
    let sample = &bytes[..sniff_len];
    if sample.contains(&0) {
        return false;
    }
    if sample.is_empty() {
        return true;
    }
    let non_text = sample.iter().filter(|b| !is_text_byte(**b)).count();
    non_text * 100 / sample.len() <= 5
}

fn is_text_byte(b: u8) -> bool {
    matches!(b, 0x07 | 0x08 | b'\t' | b'\n' | 0x0B | b'\x0C' | b'\r' | 0x1B)
        || (0x20..=0x7E).contains(&b)
        || b >= 0x80
}

fn decode_text_best_effort(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(_) => String::from_utf8(bytes.to_vec()).unwrap_or_default(),
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn try_decode_utf16_no_bom(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let sample_len = bytes.len().min(512);
    let sample = &bytes[..sample_len];
    let mut even_nul = 0usize;
    let mut odd_nul = 0usize;
    for (i, b) in sample.iter().enumerate() {
        if *b == 0 {
            if i % 2 == 0 {
                even_nul += 1;
            } else {
                odd_nul += 1;
            }
        }
    }
    let pairs = sample_len / 2;
    if pairs == 0 {
        return None;
    }
    let high_threshold = (pairs * 4) / 10;
    let low_threshold = pairs / 8;
    if odd_nul >= high_threshold && even_nul <= low_threshold {
        let (decoded, _, had_errors) = encoding_rs::UTF_16LE.decode(bytes);
        if !had_errors {
            return Some(decoded.into_owned());
        }
    }
    if even_nul >= high_threshold && odd_nul <= low_threshold {
        let (decoded, _, had_errors) = encoding_rs::UTF_16BE.decode(bytes);
        if !had_errors {
            return Some(decoded.into_owned());
        }
    }
    None
}

fn is_known_text_path(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if !ext.is_empty() && TEXT_EXTENSIONS.iter().any(|e| *e == ext) {
        return true;
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if !name.is_empty() && TEXT_FILENAMES.iter().any(|e| *e == name) {
        return true;
    }
    false
}

const TEXT_EXTENSIONS: &[&str] = &[
    "applescript",
    "asm",
    "astro",
    "bash",
    "bat",
    "bazel",
    "bib",
    "build",
    "c",
    "cc",
    "cfg",
    "clj",
    "cljs",
    "cmake",
    "cmd",
    "cnf",
    "code-workspace",
    "coffee",
    "conf",
    "config",
    "cpp",
    "cs",
    "csh",
    "css",
    "csv",
    "cts",
    "cxx",
    "d",
    "dart",
    "diff",
    "dockerfile",
    "dprint",
    "edn",
    "ejs",
    "elm",
    "env",
    "erl",
    "ex",
    "exs",
    "fish",
    "fs",
    "fsi",
    "fsx",
    "gemspec",
    "gitattributes",
    "gitconfig",
    "gitignore",
    "gleam",
    "go",
    "gradle",
    "graphql",
    "groovy",
    "gql",
    "h",
    "haml",
    "hbs",
    "hcl",
    "hh",
    "hpp",
    "hs",
    "htm",
    "html",
    "hxx",
    "ics",
    "ini",
    "iml",
    "j2",
    "java",
    "jinja",
    "jl",
    "js",
    "json",
    "json5",
    "jsonc",
    "jsonl",
    "jsx",
    "kt",
    "kts",
    "latex",
    "less",
    "lisp",
    "lock",
    "log",
    "lua",
    "m",
    "manifest",
    "markdown",
    "md",
    "mdx",
    "mjs",
    "mk",
    "ml",
    "mli",
    "mm",
    "mod",
    "mts",
    "nim",
    "nix",
    "patch",
    "pas",
    "php",
    "pl",
    "pp",
    "pri",
    "pro",
    "properties",
    "props",
    "proto",
    "ps1",
    "psd1",
    "psm1",
    "pug",
    "purs",
    "py",
    "pyi",
    "pyx",
    "qml",
    "r",
    "rb",
    "re",
    "resx",
    "rmd",
    "rs",
    "rst",
    "ru",
    "s",
    "sass",
    "sbt",
    "sc",
    "scala",
    "scss",
    "service",
    "sh",
    "sln",
    "slt",
    "sql",
    "stylus",
    "sty",
    "sum",
    "svelte",
    "svg",
    "swift",
    "sx",
    "tcl",
    "tex",
    "textile",
    "tf",
    "tfvars",
    "tml",
    "toml",
    "ts",
    "tsv",
    "tsx",
    "twig",
    "txt",
    "v",
    "vb",
    "vbs",
    "vim",
    "vh",
    "vhd",
    "vhdl",
    "vue",
    "wat",
    "wxs",
    "xaml",
    "xml",
    "xsd",
    "xsl",
    "xslt",
    "yaml",
    "yml",
    "zig",
    "zsh",
];

const TEXT_FILENAMES: &[&str] = &[
    ".babelrc",
    ".bash_aliases",
    ".bash_profile",
    ".bashrc",
    ".dockerignore",
    ".editorconfig",
    ".env",
    ".eslintignore",
    ".eslintrc",
    ".gitattributes",
    ".gitconfig",
    ".gitignore",
    ".gitmodules",
    ".npmignore",
    ".npmrc",
    ".nvmrc",
    ".prettierignore",
    ".prettierrc",
    ".profile",
    ".python-version",
    ".rspec",
    ".rubocop.yml",
    ".tool-versions",
    ".yarnrc",
    ".zshrc",
    "authors",
    "berksfile",
    "brewfile",
    "build",
    "capfile",
    "cargo.lock",
    "cargo.toml",
    "changelog",
    "cmakelists.txt",
    "code_of_conduct",
    "contributing",
    "contributors",
    "copying",
    "dockerfile",
    "doxyfile",
    "gemfile",
    "guardfile",
    "license",
    "license.md",
    "license.txt",
    "makefile",
    "notice",
    "owners",
    "package-lock.json",
    "package.json",
    "pipfile",
    "podfile",
    "procfile",
    "rakefile",
    "readme",
    "readme.md",
    "readme.txt",
    "thanks",
    "todo",
    "vagrantfile",
    "yarn.lock",
];

fn mime_from_extension(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "json" | "json5" | "jsonc" | "jsonl" => "application/json",
        "md" | "markdown" | "mdx" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "scss" => "text/x-scss",
        "less" => "text/x-less",
        "js" | "mjs" | "cjs" | "jsx" => "text/javascript",
        "ts" | "tsx" | "mts" | "cts" => "text/typescript",
        "rs" => "text/x-rust",
        "py" | "pyi" => "text/x-python",
        "go" => "text/x-go",
        "java" => "text/x-java",
        "kt" | "kts" => "text/x-kotlin",
        "c" | "h" => "text/x-c",
        "cc" | "cpp" | "cxx" | "hpp" | "hxx" => "text/x-c++",
        "cs" => "text/x-csharp",
        "rb" => "text/x-ruby",
        "php" => "application/x-php",
        "swift" => "text/x-swift",
        "dart" => "text/x-dart",
        "lua" => "text/x-lua",
        "sh" | "bash" | "zsh" => "application/x-shellscript",
        "ps1" | "psm1" => "application/x-powershell",
        "bat" | "cmd" => "application/x-bat",
        "yaml" | "yml" => "text/yaml",
        "toml" => "text/toml",
        "ini" | "cfg" | "conf" | "properties" => "text/plain",
        "xml" => "text/xml",
        "vue" => "text/x-vue",
        "svelte" => "text/x-svelte",
        "sql" => "application/sql",
        "proto" => "text/x-proto",
        "graphql" | "gql" => "application/graphql",
        "diff" | "patch" => "text/x-diff",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "tif" | "tiff" => "image/tiff",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "ogv" => "video/ogg",
        "avi" => "video/x-msvideo",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "m4a" => "audio/mp4",
        "opus" => "audio/opus",
        "pdf" => "application/pdf",
        "txt" | "log" | "env" | "lock" => "text/plain",
        "rst" => "text/x-rst",
        "tex" | "latex" => "text/x-tex",
        _ => "application/octet-stream",
    }
}

#[derive(Debug, Deserialize)]
pub struct FilePutBody {
    pub root: String,
    pub path: String,
    pub content: String,
    #[serde(default, rename = "ifMatchMtime")]
    pub if_match_mtime: Option<String>,
    #[serde(default)]
    pub encoding: Option<String>,
}

pub async fn handle_workspace_file_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<FilePutBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    write_file_inner(&state, body,  false).await
}

pub async fn handle_workspace_file_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<FilePutBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    write_file_inner(&state, body,  true).await
}

async fn write_file_inner(
    state: &AppState,
    body: FilePutBody,
    create_only: bool,
) -> axum::response::Response {
    let root = match allowed_workspace_root(state, &body.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let target = match resolve_within(&root, &body.path, false) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };

    if create_only && target.exists() {
        return FsError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "File already exists",
        ))
        .into_response();
    }

    let bytes = match body.encoding.as_deref() {
        Some("base64") => match base64::engine::general_purpose::STANDARD.decode(body.content.as_bytes()) {
            Ok(b) => b,
            Err(_) => return FsError::InvalidName.into_response(),
        },
        _ => body.content.as_bytes().to_vec(),
    };

    let target_for_io = target.clone();
    let if_match = body.if_match_mtime.clone();
    let bytes_for_io = bytes;
    enum WriteOutcome {
        Conflict(String),
        Ok(Option<std::fs::Metadata>),
        Err(std::io::Error),
    }
    let outcome = tokio::task::spawn_blocking(move || -> WriteOutcome {
        if !create_only {
            if let Some(expected) = if_match.as_deref() {
                if target_for_io.exists() {
                    if let Ok(meta) = std::fs::metadata(&target_for_io) {
                        if let Some(actual) = modified_at(&meta) {
                            if actual != expected {
                                return WriteOutcome::Conflict(actual);
                            }
                        }
                    }
                }
            }
        }

        if let Some(parent) = target_for_io.parent() {
            if !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return WriteOutcome::Err(e);
                }
            }
        }

        if let Err(e) = atomic_write(&target_for_io, &bytes_for_io) {
            return WriteOutcome::Err(e);
        }

        WriteOutcome::Ok(std::fs::metadata(&target_for_io).ok())
    })
    .await
    .unwrap_or_else(|e| {
        WriteOutcome::Err(std::io::Error::other(format!(
            "blocking task join failed: {e}"
        )))
    });

    match outcome {
        WriteOutcome::Conflict(actual) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "File was modified by another process",
                "actualMtime": actual,
            })),
        )
            .into_response(),
        WriteOutcome::Err(e) => FsError::Io(e).into_response(),
        WriteOutcome::Ok(metadata) => {
            let payload = json!({
                "ok": true,
                "relPath": relative_path(&root, &target),
                "sizeBytes": metadata.as_ref().map(|m| m.len()),
                "modifiedAt": metadata.as_ref().and_then(modified_at),
            });
            (StatusCode::OK, Json(payload)).into_response()
        }
    }
}

fn atomic_write(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent")
    })?;
    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("tmp");
    let tmp = parent.join(format!(".{file_name}.tmp.{}", std::process::id()));
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all().ok();
    }
    if let Err(e) = std::fs::rename(&tmp, target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct DirPostBody {
    pub root: String,
    pub path: String,
}

pub async fn handle_workspace_dir_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DirPostBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &body.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let target = match resolve_within(&root, &body.path, false) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };
    let target_io = target.clone();
    let io = tokio::task::spawn_blocking(move || std::fs::create_dir_all(&target_io))
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())));
    if let Err(e) = io {
        return FsError::Io(e).into_response();
    }
    Json(json!({
        "ok": true,
        "relPath": relative_path(&root, &target),
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct MoveBody {
    pub root: String,
    #[serde(rename = "fromPath")]
    pub from_path: String,
    #[serde(rename = "toPath")]
    pub to_path: String,
}

pub async fn handle_workspace_move(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MoveBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &body.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let from = match resolve_within(&root, &body.from_path, true) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    let to = match resolve_within(&root, &body.to_path, false) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    let from_io = from.clone();
    let to_io = to.clone();
    let io: Result<(), FsError> = tokio::task::spawn_blocking(move || {
        if to_io.exists() {
            return Err(FsError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "Destination already exists",
            )));
        }
        if let Some(parent) = to_io.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(FsError::Io)?;
            }
        }
        std::fs::rename(&from_io, &to_io).map_err(FsError::Io)?;
        Ok(())
    })
    .await
    .unwrap_or_else(|e| Err(FsError::Io(std::io::Error::other(e.to_string()))));
    if let Err(e) = io {
        return e.into_response();
    }
    Json(json!({
        "ok": true,
        "fromPath": relative_path(&root, &from),
        "toPath": relative_path(&root, &to),
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct CopyBody {
    pub root: String,
    #[serde(rename = "fromPath")]
    pub from_path: String,
    #[serde(rename = "toDir")]
    pub to_dir: String,
}

fn make_copy_name(name: &str, is_dir: bool, suffix: &str) -> String {
    if is_dir {
        return format!("{name}{suffix}");
    }
    match name.rfind('.') {
        Some(idx) if idx > 0 => {
            let (stem, ext) = name.split_at(idx);
            format!("{stem}{suffix}{ext}")
        }
        _ => format!("{name}{suffix}"),
    }
}

fn unique_dest(to_dir: &Path, name: &str, is_dir: bool) -> PathBuf {
    let first = to_dir.join(name);
    if !first.exists() {
        return first;
    }
    for i in 1..=9999 {
        let suffix = if i == 1 {
            " copy".to_string()
        } else {
            format!(" copy {i}")
        };
        let candidate = to_dir.join(make_copy_name(name, is_dir, &suffix));
        if !candidate.exists() {
            return candidate;
        }
    }
    to_dir.join(make_copy_name(name, is_dir, " copy"))
}

fn scan_totals(src: &Path, cancel: &AtomicBool) -> std::io::Result<(u64, u64)> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        return Ok((0, 0));
    }
    if meta.is_dir() {
        let mut bytes = 0u64;
        let mut files = 0u64;
        for entry in std::fs::read_dir(src)? {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let entry = entry?;
            let (b, f) = scan_totals(&entry.path(), cancel)?;
            bytes = bytes.saturating_add(b);
            files = files.saturating_add(f);
        }
        Ok((bytes, files))
    } else {
        Ok((meta.len(), 1))
    }
}

struct CopyCtx<'a> {
    bytes_done: u64,
    files_done: u64,
    total_bytes: u64,
    total_files: u64,
    last_emit: Instant,
    cancel: &'a AtomicBool,
    root: &'a Path,
    emit: &'a mut dyn FnMut(serde_json::Value) -> bool,
}

impl CopyCtx<'_> {
    fn maybe_emit(&mut self, current: &Path, force: bool) -> bool {
        if !force && self.last_emit.elapsed() < Duration::from_millis(80) {
            return true;
        }
        self.last_emit = Instant::now();
        let payload = json!({
            "type": "progress",
            "bytesDone": self.bytes_done,
            "bytesTotal": self.total_bytes,
            "filesDone": self.files_done,
            "filesTotal": self.total_files,
            "currentRelPath": relative_path(self.root, current),
        });
        (self.emit)(payload)
    }
}

fn copy_entry(src: &Path, dest: &Path, ctx: &mut CopyCtx) -> std::io::Result<()> {
    if ctx.cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    let meta = std::fs::symlink_metadata(src)?;
    let file_type = meta.file_type();
    if file_type.is_symlink() {
        return Ok(());
    }
    if file_type.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(src)? {
            if ctx.cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let entry = entry?;
            let child_src = entry.path();
            let child_dest = dest.join(entry.file_name());
            copy_entry(&child_src, &child_dest, ctx)?;
        }
    } else {
        std::fs::copy(src, dest)?;
        ctx.bytes_done = ctx.bytes_done.saturating_add(meta.len());
        ctx.files_done = ctx.files_done.saturating_add(1);
        if !ctx.maybe_emit(src, false) {
            ctx.cancel.store(true, Ordering::Relaxed);
            return Ok(());
        }
    }
    Ok(())
}

struct CancelOnDrop<S> {
    inner: S,
    cancel: std::sync::Arc<AtomicBool>,
}

impl<S: tokio_stream::Stream + Unpin> tokio_stream::Stream for CancelOnDrop<S> {
    type Item = S::Item;
    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.inner).poll_next(cx)
    }
}

impl<S> Drop for CancelOnDrop<S> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub async fn handle_workspace_copy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CopyBody>,
) -> axum::response::Response {
    use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
    use std::convert::Infallible;
    use std::sync::Arc;
    use tokio_stream::wrappers::ReceiverStream;

    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &body.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let from = match resolve_within(&root, &body.from_path, true) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    let to_dir = match resolve_within(&root, &body.to_dir, true) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    if !to_dir.is_dir() {
        return FsError::InvalidName.into_response();
    }
    if to_dir == from || to_dir.starts_with(&from) {
        return FsError::InvalidName.into_response();
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<SseEvent, Infallible>>(64);
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_task = cancel.clone();
    let root_for_rel = root.clone();

    tokio::task::spawn_blocking(move || {
        let mut emit = |payload: serde_json::Value| -> bool {
            tx.blocking_send(Ok(SseEvent::default().data(payload.to_string())))
                .is_ok()
        };
        let is_dir = match std::fs::symlink_metadata(&from) {
            Ok(m) => m.is_dir(),
            Err(e) => {
                emit(json!({"type": "error", "message": e.to_string()}));
                return;
            }
        };
        let name = from
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            emit(json!({"type": "error", "message": "Invalid source name"}));
            return;
        }
        let dest = unique_dest(&to_dir, &name, is_dir);

        let (total_bytes, total_files) = match scan_totals(&from, &cancel_task) {
            Ok(v) => v,
            Err(e) => {
                emit(json!({"type": "error", "message": e.to_string()}));
                return;
            }
        };

        let mut ctx = CopyCtx {
            bytes_done: 0,
            files_done: 0,
            total_bytes,
            total_files,
            last_emit: Instant::now()
                .checked_sub(Duration::from_millis(200))
                .unwrap_or_else(Instant::now),
            cancel: &cancel_task,
            root: &root_for_rel,
            emit: &mut emit,
        };
        ctx.maybe_emit(&from, true);

        match copy_entry(&from, &dest, &mut ctx) {
            Ok(()) => {
                if cancel_task.load(Ordering::Relaxed) {
                    (ctx.emit)(json!({"type": "error", "message": "cancelled"}));
                } else {
                    ctx.maybe_emit(&dest, true);
                    (ctx.emit)(json!({
                        "type": "done",
                        "toPath": relative_path(&root_for_rel, &dest),
                    }));
                }
            }
            Err(e) => {
                (ctx.emit)(json!({"type": "error", "message": e.to_string()}));
            }
        }
    });

    let stream = CancelOnDrop {
        inner: ReceiverStream::new(rx),
        cancel,
    };

    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        )
        .into_response()
}

#[derive(Debug, Deserialize)]
pub struct DeleteQuery {
    pub root: String,
    pub path: String,
    #[serde(default)]
    pub recursive: Option<bool>,
}

pub async fn handle_workspace_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<DeleteQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &q.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let target = match resolve_within(&root, &q.path, true) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    if target == root {
        return FsError::OutsideRoot.into_response();
    }
    let target_io = target.clone();
    let recursive = q.recursive.unwrap_or(false);
    let io: Result<(), FsError> = tokio::task::spawn_blocking(move || {
        let metadata = std::fs::metadata(&target_io).map_err(FsError::Io)?;
        let result = if metadata.is_dir() {
            if recursive {
                std::fs::remove_dir_all(&target_io)
            } else {
                std::fs::remove_dir(&target_io)
            }
        } else {
            std::fs::remove_file(&target_io)
        };
        result.map_err(FsError::Io)
    })
    .await
    .unwrap_or_else(|e| Err(FsError::Io(std::io::Error::other(e.to_string()))));
    if let Err(e) = io {
        return e.into_response();
    }
    Json(json!({"ok": true})).into_response()
}

#[derive(Debug, Deserialize)]
pub struct UploadBody {
    pub root: String,
    pub path: String,
    #[serde(rename = "contentBase64")]
    pub content_base64: String,
    #[serde(default)]
    pub overwrite: Option<bool>,
}

pub async fn handle_workspace_upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UploadBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &body.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let target = match resolve_within(&root, &body.path, false) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };
    if target.exists() && !body.overwrite.unwrap_or(false) {
        return FsError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "File already exists",
        ))
        .into_response();
    }
    let bytes = match base64::engine::general_purpose::STANDARD.decode(body.content_base64.as_bytes()) {
        Ok(b) => b,
        Err(_) => return FsError::InvalidName.into_response(),
    };
    if bytes.len() > MAX_UPLOAD_BYTES {
        return FsError::TooLarge(bytes.len() as u64).into_response();
    }
    let target_io = target.clone();
    let io: Result<Option<std::fs::Metadata>, FsError> = tokio::task::spawn_blocking(move || {
        if let Some(parent) = target_io.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(FsError::Io)?;
            }
        }
        atomic_write(&target_io, &bytes).map_err(FsError::Io)?;
        Ok(std::fs::metadata(&target_io).ok())
    })
    .await
    .unwrap_or_else(|e| Err(FsError::Io(std::io::Error::other(e.to_string()))));
    let metadata = match io {
        Ok(m) => m,
        Err(e) => return e.into_response(),
    };
    Json(json!({
        "ok": true,
        "relPath": relative_path(&root, &target),
        "sizeBytes": metadata.as_ref().map(|m| m.len()),
        "modifiedAt": metadata.as_ref().and_then(modified_at),
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub root: String,
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default, rename = "showHidden")]
    pub show_hidden: Option<bool>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default, rename = "caseSensitive")]
    pub case_sensitive: Option<bool>,
    #[serde(default, rename = "wholeWord")]
    pub whole_word: Option<bool>,
    #[serde(default)]
    pub regex: Option<bool>,
    #[serde(default, rename = "maxFileSizeBytes")]
    pub max_file_size_bytes: Option<u64>,
}

pub async fn handle_workspace_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SearchQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &q.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let needle_raw = q.query.trim().to_string();
    if needle_raw.is_empty() {
        return Json(json!({"results": [], "total": 0})).into_response();
    }
    let limit = q.limit.unwrap_or(MAX_SEARCH_RESULTS).min(MAX_SEARCH_RESULTS);
    let show_hidden = q.show_hidden.unwrap_or(false);
    let kind = q
        .kind
        .as_deref()
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "name".to_string());

    let case_sensitive = q.case_sensitive.unwrap_or(false);
    let whole_word = q.whole_word.unwrap_or(false);
    let regex = q.regex.unwrap_or(false);
    let max_size = q.max_file_size_bytes.unwrap_or(2 * 1024 * 1024);
    let root_io = root.clone();

    let body = tokio::task::spawn_blocking(move || {
        if kind == "content" {
            let results = run_content_search(
                &root_io,
                &needle_raw,
                limit,
                ContentSearchOptions {
                    show_hidden,
                    case_sensitive,
                    whole_word,
                    regex,
                },
                max_size,
            );
            let total = results.len();
            return json!({
                "results": results,
                "total": total,
                "limit": limit,
                "kind": "content",
            });
        }

        let needle_lower = needle_raw.to_ascii_lowercase();
        let mut scored: Vec<FuzzyHit> = Vec::new();
        walk_filenames_fuzzy(&root_io, &root_io, &needle_lower, show_hidden, &mut scored);
        scored.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.depth.cmp(&b.depth))
                .then_with(|| a.rel_path.cmp(&b.rel_path))
        });
        scored.truncate(limit);
        let results: Vec<serde_json::Value> = scored
            .iter()
            .map(|hit| {
                let mut payload =
                    entry_to_json(&root_io, &hit.absolute_path, &hit.name, hit.is_dir);
                if let Some(map) = payload.as_object_mut() {
                    map.insert("score".into(), json!(hit.score));
                }
                payload
            })
            .collect();
        let total = results.len();
        json!({
            "results": results,
            "total": total,
            "limit": limit,
            "kind": "name",
        })
    })
    .await
    .unwrap_or_else(|_| json!({"results": [], "total": 0}));

    Json(body).into_response()
}

#[derive(Debug)]
struct FuzzyHit {
    name: String,
    rel_path: String,
    absolute_path: PathBuf,
    is_dir: bool,
    depth: u32,
    score: i64,
}

fn walk_filenames_fuzzy(
    root: &Path,
    dir: &Path,
    needle_lower: &str,
    show_hidden: bool,
    out: &mut Vec<FuzzyHit>,
) {
    if out.len() >= MAX_SEARCH_RESULTS * 4 {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        if out.len() >= MAX_SEARCH_RESULTS * 4 {
            return;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !show_hidden && is_hidden_default(&name) {
            continue;
        }
        let path = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let rel_path = relative_path(root, &path);
        let depth = rel_path.matches('/').count() as u32;
        let name_lower = name.to_ascii_lowercase();
        let rel_lower = rel_path.to_ascii_lowercase();
        if let Some(score) = fuzzy_score(&name_lower, &rel_lower, needle_lower, depth) {
            out.push(FuzzyHit {
                name,
                rel_path,
                absolute_path: path.clone(),
                is_dir,
                depth,
                score,
            });
        }
        if is_dir {
            walk_filenames_fuzzy(root, &path, needle_lower, show_hidden, out);
        }
    }
}

fn fuzzy_score(
    name_lower: &str,
    rel_lower: &str,
    needle_lower: &str,
    depth: u32,
) -> Option<i64> {
    if needle_lower.is_empty() {
        return None;
    }
    if needle_lower.contains('/') || needle_lower.contains('\\') {
        let needle_norm = needle_lower.replace('\\', "/");
        if !rel_lower.contains(&needle_norm) {
            return None;
        }
        let mut score: i64 = 250;
        if rel_lower.starts_with(&needle_norm) {
            score += 200;
        }
        score -= depth as i64 * 5;
        return Some(score);
    }
    let mut score: i64 = 0;
    if name_lower == needle_lower {
        score += 1000;
    }
    if name_lower.starts_with(needle_lower) {
        score += 500;
    }
    if name_lower.contains(needle_lower) {
        score += 200;
    }
    if let Some(sub) = fuzzy_subsequence_score(name_lower, needle_lower) {
        score += sub;
    } else if let Some(sub) = fuzzy_subsequence_score(rel_lower, needle_lower) {
        score += sub / 2;
    } else {
        return None;
    }
    let camel_bonus = camel_hump_bonus(name_lower, needle_lower);
    score += camel_bonus;
    score -= depth as i64 * 3;
    Some(score)
}

fn fuzzy_subsequence_score(haystack: &str, needle: &str) -> Option<i64> {
    let mut hi = haystack.chars();
    let mut prev_idx: Option<usize> = None;
    let mut idx = 0usize;
    let mut score: i64 = 0;
    let mut consecutive = 0i64;
    for ch in needle.chars() {
        let mut found = false;
        for h in hi.by_ref() {
            idx += 1;
            if h == ch {
                if let Some(prev) = prev_idx {
                    let gap = idx.saturating_sub(prev) as i64;
                    if gap <= 1 {
                        consecutive += 1;
                        score += 8 + consecutive * 4;
                    } else {
                        consecutive = 0;
                        score += (10 - gap.min(8)).max(1);
                    }
                } else {
                    score += 12 - (idx as i64).min(8);
                    consecutive = 1;
                }
                prev_idx = Some(idx);
                found = true;
                break;
            }
        }
        if !found {
            return None;
        }
    }
    Some(score)
}

fn camel_hump_bonus(name: &str, needle: &str) -> i64 {
    let mut bonus: i64 = 0;
    let mut needle_chars = needle.chars().peekable();
    let mut prev_was_separator = true;
    let mut prev_was_lower = false;
    for ch in name.chars() {
        let is_upper = ch.is_ascii_uppercase();
        let is_digit = ch.is_ascii_digit();
        let is_alpha = ch.is_ascii_alphabetic();
        let is_boundary = prev_was_separator
            || (prev_was_lower && (is_upper || is_digit))
            || !is_alpha;
        if is_boundary {
            if let Some(next) = needle_chars.peek() {
                if ch.to_ascii_lowercase() == *next {
                    bonus += 18;
                    needle_chars.next();
                }
            }
        }
        prev_was_separator = !is_alpha && !is_digit;
        prev_was_lower = ch.is_ascii_lowercase();
    }
    bonus
}

#[derive(Clone, Copy)]
struct ContentSearchOptions {
    show_hidden: bool,
    case_sensitive: bool,
    whole_word: bool,
    regex: bool,
}

fn run_content_search(
    root: &Path,
    pattern: &str,
    limit: usize,
    opts: ContentSearchOptions,
    max_file_size_bytes: u64,
) -> Vec<serde_json::Value> {
    if let Some(rg_results) = run_ripgrep_search(
        root,
        pattern,
        limit,
        opts,
        max_file_size_bytes,
    ) {
        return rg_results;
    }
    fallback_content_search(
        root,
        pattern,
        limit,
        opts.show_hidden,
        opts.case_sensitive,
        opts.whole_word,
        max_file_size_bytes,
    )
}

fn run_ripgrep_search(
    root: &Path,
    pattern: &str,
    limit: usize,
    opts: ContentSearchOptions,
    max_file_size_bytes: u64,
) -> Option<Vec<serde_json::Value>> {
    let mut cmd = crate::util::hidden_sync_command("rg");
    cmd.arg("--json")
        .arg("--max-count=20")
        .arg("--max-filesize")
        .arg(format!("{}b", max_file_size_bytes));
    if !opts.regex {
        cmd.arg("--fixed-strings");
    }
    if opts.whole_word {
        cmd.arg("--word-regexp");
    }
    if !opts.case_sensitive {
        cmd.arg("--ignore-case");
    }
    if !opts.show_hidden {
        cmd.arg("--hidden");
        for excl in HIDDEN_DEFAULT_DIRS {
            cmd.arg("--glob").arg(format!("!**/{}/**", excl));
            cmd.arg("--glob").arg(format!("!**/{}", excl));
        }
    } else {
        cmd.arg("--hidden");
    }
    cmd.arg("--").arg(pattern).arg(root);
    let output = cmd.output().ok()?;
    let stdout = String::from_utf8(output.stdout).ok()?;
    let mut results: Vec<serde_json::Value> = Vec::new();
    for line in stdout.lines() {
        if results.len() >= limit {
            break;
        }
        let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(line) else {
            continue;
        };
        let kind = value
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if kind != "match" {
            continue;
        }
        let data = match value.get("data") {
            Some(d) => d,
            None => continue,
        };
        let path_text = data
            .get("path")
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or_default();
        if path_text.is_empty() {
            continue;
        }
        let abs = PathBuf::from(path_text);
        let rel = relative_path(root, &abs);
        let line_number = data.get("line_number").and_then(|v| v.as_u64()).unwrap_or(0);
        let line_text = data
            .get("lines")
            .and_then(|l| l.get("text"))
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .trim_end_matches(['\n', '\r'])
            .to_string();
        let mut submatches_json: Vec<serde_json::Value> = Vec::new();
        if let Some(arr) = data.get("submatches").and_then(|v| v.as_array()) {
            for sm in arr {
                let start = sm.get("start").and_then(|v| v.as_u64()).unwrap_or(0);
                let end = sm.get("end").and_then(|v| v.as_u64()).unwrap_or(0);
                submatches_json.push(json!({
                    "start": start,
                    "end": end,
                }));
            }
        }
        results.push(json!({
            "name": abs.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            "relPath": rel,
            "isDir": false,
            "line": line_number.saturating_sub(1),
            "preview": line_text,
            "submatches": submatches_json,
        }));
    }
    Some(results)
}

fn fallback_content_search(
    root: &Path,
    pattern: &str,
    limit: usize,
    show_hidden: bool,
    case_sensitive: bool,
    whole_word: bool,
    max_file_size_bytes: u64,
) -> Vec<serde_json::Value> {
    let needle = if case_sensitive {
        pattern.to_string()
    } else {
        pattern.to_ascii_lowercase()
    };
    let mut out: Vec<serde_json::Value> = Vec::new();
    walk_content(
        root,
        root,
        &needle,
        show_hidden,
        case_sensitive,
        whole_word,
        max_file_size_bytes,
        limit,
        &mut out,
    );
    out
}

fn walk_content(
    root: &Path,
    dir: &Path,
    needle: &str,
    show_hidden: bool,
    case_sensitive: bool,
    whole_word: bool,
    max_size: u64,
    limit: usize,
    out: &mut Vec<serde_json::Value>,
) {
    if out.len() >= limit {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        if out.len() >= limit {
            return;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !show_hidden && is_hidden_default(&name) {
            continue;
        }
        let path = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            walk_content(
                root,
                &path,
                needle,
                show_hidden,
                case_sensitive,
                whole_word,
                max_size,
                limit,
                out,
            );
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.len() > max_size {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (idx, line) in text.lines().enumerate() {
            if out.len() >= limit {
                return;
            }
            let haystack = if case_sensitive {
                line.to_string()
            } else {
                line.to_ascii_lowercase()
            };
            let mut found = false;
            if whole_word {
                let needle_chars: Vec<char> = needle.chars().collect();
                let line_chars: Vec<char> = haystack.chars().collect();
                if needle_chars.is_empty() {
                    continue;
                }
                let n = needle_chars.len();
                'outer: for start in 0..line_chars.len().saturating_sub(n - 1) {
                    if &line_chars[start..start + n] != needle_chars.as_slice() {
                        continue;
                    }
                    let before = if start == 0 {
                        None
                    } else {
                        Some(line_chars[start - 1])
                    };
                    let after = line_chars.get(start + n).copied();
                    let is_word = |c: Option<char>| match c {
                        Some(ch) => ch.is_alphanumeric() || ch == '_',
                        None => false,
                    };
                    if !is_word(before) && !is_word(after) {
                        found = true;
                        break 'outer;
                    }
                }
            } else if haystack.contains(needle) {
                found = true;
            }
            if found {
                let preview = line.trim_end_matches('\r').to_string();
                out.push(json!({
                    "name": path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                    "relPath": relative_path(root, &path),
                    "isDir": false,
                    "line": idx as u64,
                    "preview": preview,
                }));
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct WatchQuery {
    pub root: String,
}

#[cfg(feature = "fs-watch")]
pub async fn handle_workspace_watch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WatchQuery>,
) -> axum::response::Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &q.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let cache = state.git_status_cache.clone();
    match watch_impl::start_watch_stream(root, cache).await {
        Ok(sse) => sse.into_response(),
        Err(e) => {
            tracing::warn!(err = %e, "workspace watch start failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

#[cfg(not(feature = "fs-watch"))]
pub async fn handle_workspace_watch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(_q): Query<WatchQuery>,
) -> axum::response::Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "fs-watch feature is disabled in this build",
        })),
    )
        .into_response()
}

#[cfg(feature = "fs-watch")]
mod watch_impl {
    use super::relative_path;
    use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
    use futures_util::future::OptionFuture;
    use notify::event::{EventKind, ModifyKind, RenameMode};
    use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
    use serde_json::json;
    use std::collections::{HashMap, VecDeque};
    use std::convert::Infallible;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_stream::Stream;

    const DEBOUNCE_MS: u64 = 100;

    const RENAME_WINDOW_MS: u64 = 800;

    const RECENT_REMOVED_TTL_MS: u64 = 1500;

    const RECENT_REMOVED_CAP: usize = 32;

    const CHANNEL_SIZE: usize = 64;
    const RAW_WATCH_CHANNEL_SIZE: usize = 4096;

    const KEEP_ALIVE_SECS: u64 = 15;

    #[derive(Clone)]
    struct PendingEvent {
        kind: &'static str,
        from_rel: Option<String>,
    }

    struct RecentRemoved {
        rel: String,
        basename: String,
        parent: String,
        expires_at: Instant,
    }

    fn split_rel(rel: &str) -> (String, String) {
        if let Some(idx) = rel.rfind('/') {
            (rel[..idx].to_string(), rel[idx + 1..].to_string())
        } else {
            (String::new(), rel.to_string())
        }
    }

    fn push_recent_removed(buf: &mut VecDeque<RecentRemoved>, rel: &str) {
        let (parent, basename) = split_rel(rel);
        if basename.is_empty() {
            return;
        }
        while buf.len() >= RECENT_REMOVED_CAP {
            buf.pop_front();
        }
        buf.push_back(RecentRemoved {
            rel: rel.to_string(),
            basename,
            parent,
            expires_at: Instant::now() + Duration::from_millis(RECENT_REMOVED_TTL_MS),
        });
    }

    fn match_recent_removed(
        buf: &mut VecDeque<RecentRemoved>,
        to_rel: &str,
    ) -> Option<String> {
        let (to_parent, to_base) = split_rel(to_rel);
        if to_base.is_empty() {
            return None;
        }
        if let Some(idx) = buf
            .iter()
            .position(|r| r.basename == to_base && r.parent == to_parent)
        {
            return buf.remove(idx).map(|r| r.rel);
        }
        if let Some(idx) = buf.iter().position(|r| r.basename == to_base) {
            return buf.remove(idx).map(|r| r.rel);
        }
        None
    }

    fn compute_next_deadline(
        pending_renames: &HashMap<usize, (String, Instant)>,
        recent_removed: &VecDeque<RecentRemoved>,
    ) -> Option<tokio::time::Instant> {
        let mut next: Option<Instant> = None;
        for (_, ts) in pending_renames.values() {
            let exp = *ts + Duration::from_millis(RENAME_WINDOW_MS);
            next = Some(next.map_or(exp, |n: Instant| n.min(exp)));
        }
        for r in recent_removed {
            next = Some(next.map_or(r.expires_at, |n: Instant| n.min(r.expires_at)));
        }
        next.map(|i| {
            let remaining = i.saturating_duration_since(Instant::now());
            tokio::time::Instant::now() + remaining
        })
    }

    pub(super) async fn start_watch_stream(
        root: PathBuf,
        git_status_cache: crate::gateway::git_routes::GitStatusCache,
    ) -> std::io::Result<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>> {
        let (raw_tx, mut raw_rx) =
            tokio::sync::mpsc::channel::<notify::Result<notify::Event>>(RAW_WATCH_CHANNEL_SIZE);
        let config = Config::default().with_poll_interval(Duration::from_millis(500));
        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| {
                if let Err(tokio::sync::mpsc::error::TrySendError::Full(_)) = raw_tx.try_send(res) {
                    tracing::warn!(
                        target: "workspace.watch",
                        "file watch event buffer full; dropping event (client may need manual refresh)"
                    );
                }
            },
            config,
        )
        .map_err(|e| std::io::Error::other(format!("notify init: {e}")))?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| std::io::Error::other(format!("notify watch: {e}")))?;

        let (out_tx, out_rx) =
            tokio::sync::mpsc::channel::<Result<SseEvent, Infallible>>(CHANNEL_SIZE);

        crate::runtime::spawn_supervised("workspace.file_watch", async move {

            let _watcher_guard = watcher;
            let mut pending: HashMap<String, PendingEvent> = HashMap::new();
            let mut pending_renames: HashMap<usize, (String, Instant)> = HashMap::new();
            let mut recent_removed: VecDeque<RecentRemoved> =
                VecDeque::with_capacity(RECENT_REMOVED_CAP);
            let mut deadline: Option<tokio::time::Instant> = None;

            loop {
                let sleep_fut: OptionFuture<_> =
                    deadline.map(tokio::time::sleep_until).into();
                tokio::pin!(sleep_fut);

                tokio::select! {
                    biased;
                    Some(_) = &mut sleep_fut => {
                        let now = Instant::now();
                        let stale_keys: Vec<usize> = pending_renames
                            .iter()
                            .filter_map(|(k, (_, ts))| {
                                if now.duration_since(*ts)
                                    >= Duration::from_millis(RENAME_WINDOW_MS)
                                {
                                    Some(*k)
                                } else {
                                    None
                                }
                            })
                            .collect();
                        for key in stale_keys {
                            if let Some((rel, _)) = pending_renames.remove(&key) {
                                push_recent_removed(&mut recent_removed, &rel);
                                pending.entry(rel).or_insert(PendingEvent {
                                    kind: "removed",
                                    from_rel: None,
                                });
                            }
                        }
                        while let Some(front) = recent_removed.front() {
                            if front.expires_at <= now {
                                recent_removed.pop_front();
                            } else {
                                break;
                            }
                        }
                        let drained: Vec<(String, PendingEvent)> =
                            pending.drain().collect();
                        deadline =
                            compute_next_deadline(&pending_renames, &recent_removed);
                        for (rel, info) in drained {
                            let mut payload = json!({
                                "kind": info.kind,
                                "relPath": rel,
                            });
                            if let Some(from) = info.from_rel {
                                payload["fromRelPath"] = json!(from);
                            }
                            let event = SseEvent::default().data(payload.to_string());
                            if out_tx.send(Ok(event)).await.is_err() {
                                return;
                            }
                        }
                    }
                    maybe_event = raw_rx.recv() => {
                        let Some(res) = maybe_event else { return };
                        let event = match res {
                            Ok(ev) => ev,
                            Err(err) => {
                                tracing::warn!(target: "sen::workspace_watch", error = %err, "notify backend error");
                                continue;
                            }
                        };
                        let mut should_invalidate_git = false;
                        let rels: Vec<String> = event
                            .paths
                            .iter()
                            .map(|p| relative_path(&root, p))
                            .filter(|r| !r.is_empty())
                            .collect();
                        for rel in &rels {
                            if !rel.starts_with(".git/") && rel != ".git" {
                                should_invalidate_git = true;
                            }
                        }
                        let mut handled = false;
                        match &event.kind {
                            EventKind::Modify(ModifyKind::Name(RenameMode::Both))
                                if rels.len() == 2 =>
                            {
                                let from = rels[0].clone();
                                let to = rels[1].clone();
                                pending.insert(
                                    to,
                                    PendingEvent {
                                        kind: "renamed",
                                        from_rel: Some(from),
                                    },
                                );
                                handled = true;
                            }
                            EventKind::Modify(ModifyKind::Name(RenameMode::From))
                                if !rels.is_empty() =>
                            {
                                let from = rels[0].clone();
                                if let Some(tracker) = event.attrs.tracker() {
                                    pending_renames
                                        .insert(tracker, (from, Instant::now()));
                                    handled = true;
                                } else {
                                    push_recent_removed(&mut recent_removed, &from);
                                    pending.insert(
                                        from,
                                        PendingEvent {
                                            kind: "removed",
                                            from_rel: None,
                                        },
                                    );
                                    handled = true;
                                }
                            }
                            EventKind::Modify(ModifyKind::Name(RenameMode::To))
                                if !rels.is_empty() =>
                            {
                                let to = rels[0].clone();
                                let paired = event
                                    .attrs
                                    .tracker()
                                    .and_then(|t| pending_renames.remove(&t));
                                if let Some((from, _)) = paired {
                                    pending.insert(
                                        to,
                                        PendingEvent {
                                            kind: "renamed",
                                            from_rel: Some(from),
                                        },
                                    );
                                } else if let Some(from) =
                                    match_recent_removed(&mut recent_removed, &to)
                                {
                                    pending.insert(
                                        to,
                                        PendingEvent {
                                            kind: "renamed",
                                            from_rel: Some(from),
                                        },
                                    );
                                } else {
                                    pending.insert(
                                        to,
                                        PendingEvent {
                                            kind: "renamed",
                                            from_rel: None,
                                        },
                                    );
                                }
                                handled = true;
                            }
                            _ => {}
                        }
                        if !handled {
                            let Some(kind_str) = classify_event_kind(event.kind) else {
                                if should_invalidate_git {
                                    crate::gateway::git_routes::invalidate_root(
                                        &git_status_cache,
                                        &root,
                                    );
                                }
                                continue;
                            };
                            for rel in rels {
                                let matched_from = if kind_str == "created" {
                                    match_recent_removed(&mut recent_removed, &rel)
                                } else {
                                    None
                                };
                                if let Some(from) = matched_from {
                                    pending.insert(
                                        rel,
                                        PendingEvent {
                                            kind: "renamed",
                                            from_rel: Some(from),
                                        },
                                    );
                                } else {
                                    pending
                                        .entry(rel)
                                        .and_modify(|existing| {
                                            if existing.kind != "renamed" {
                                                existing.kind = kind_str;
                                            }
                                        })
                                        .or_insert(PendingEvent {
                                            kind: kind_str,
                                            from_rel: None,
                                        });
                                }
                            }
                        }
                        if should_invalidate_git {
                            crate::gateway::git_routes::invalidate_root(
                                &git_status_cache,
                                &root,
                            );
                        }
                        let needs_deadline = !pending.is_empty()
                            || !pending_renames.is_empty()
                            || !recent_removed.is_empty();
                        if deadline.is_none() && needs_deadline {
                            let wait = if !pending.is_empty() {
                                DEBOUNCE_MS
                            } else if !pending_renames.is_empty() {
                                RENAME_WINDOW_MS
                            } else {
                                RECENT_REMOVED_TTL_MS
                            };
                            deadline = Some(
                                tokio::time::Instant::now()
                                    + Duration::from_millis(wait),
                            );
                        }
                    }
                }
            }
        });

        let stream = ReceiverStream::new(out_rx);
        Ok(Sse::new(stream).keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(KEEP_ALIVE_SECS))
                .text("keep-alive"),
        ))
    }

    fn classify_event_kind(kind: EventKind) -> Option<&'static str> {
        match kind {
            EventKind::Create(_) => Some("created"),
            EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Metadata(_)) => Some("modified"),
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => Some("removed"),
            EventKind::Modify(ModifyKind::Name(_)) => Some("renamed"),
            EventKind::Modify(_) => Some("modified"),
            EventKind::Remove(_) => Some("removed"),

            EventKind::Access(notify::event::AccessKind::Close(_)) => Some("modified"),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct StructureDocQuery {
    pub root: String,
}

pub async fn handle_workspace_structure_doc(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<StructureDocQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &q.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let doc = match tokio::task::spawn_blocking(move || {
        crate::services::magic_docs::structure_doc_for_workspace(
            &root,
            &crate::services::magic_docs::MagicDocsConfig::default(),
        )
    })
    .await
    {
        Ok(d) => d,
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "structure_doc_failed" })),
            )
                .into_response();
        }
    };
    Json(doc).into_response()
}
