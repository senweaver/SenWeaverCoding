// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use regex::Regex;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

pub struct LspRenameTool {
    security: Arc<SecurityPolicy>,
}

impl LspRenameTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }

    fn resolve_path(&self, file_path: &str) -> PathBuf {
        let p = PathBuf::from(file_path);
        if p.is_absolute() {
            p
        } else {
            self.security.workspace_dir().join(p)
        }
    }

    fn word_boundary_regex(symbol_name: &str) -> anyhow::Result<Regex> {
        let escaped = regex::escape(symbol_name);
        Regex::new(&format!(r"\b{escaped}\b"))
            .map_err(|e| anyhow::anyhow!("Failed to build rename regex: {e}"))
    }

    fn validate_write_path(&self, file_path: &str) -> anyhow::Result<()> {
        if !self.security.is_path_allowed(file_path) {
            anyhow::bail!("Path not allowed by security policy: {file_path}");
        }
        Ok(())
    }

    async fn text_rename(
        &self,
        file_path: &PathBuf,
        symbol_name: &str,
        new_name: &str,
        dry_run: bool,
    ) -> anyhow::Result<ToolResult> {
        if !file_path.is_file() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("File not found: {}", file_path.display())),
            });
        }

        let re = Self::word_boundary_regex(symbol_name)?;
        let content = tokio::fs::read_to_string(file_path).await?;

        if !re.is_match(&content) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Symbol '{}' not found in file", symbol_name)),
            });
        }

        let occurrences: Vec<(usize, String)> = content
            .lines()
            .enumerate()
            .filter(|(_, line)| re.is_match(line))
            .map(|(i, line)| (i + 1, line.trim().to_string()))
            .collect();

        let match_count: usize = content.lines().map(|line| re.find_iter(line).count()).sum();

        if dry_run {
            Ok(ToolResult {
                success: true,
                output: format!(
                    "[DRY RUN] Would rename '{}' to '{}' ({} match(es) on {} line(s)):\n{}",
                    symbol_name,
                    new_name,
                    match_count,
                    occurrences.len(),
                    occurrences
                        .iter()
                        .map(|(line, content)| format!("  Line {}: {}", line, content))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                error: None,
            })
        } else {
            if !self.security.record_action() {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Rate limit exceeded: action budget exhausted".into()),
                });
            }

            let new_content = re.replace_all(&content, new_name).to_string();
            tokio::fs::write(file_path, &new_content).await?;

            Ok(ToolResult {
                success: true,
                output: format!(
                    "Renamed '{}' to '{}' ({} match(es)) in file: {}",
                    symbol_name,
                    new_name,
                    match_count,
                    file_path.display()
                ),
                error: None,
            })
        }
    }

    async fn glob_rename(
        &self,
        pattern: &str,
        symbol_name: &str,
        new_name: &str,
        dry_run: bool,
    ) -> anyhow::Result<ToolResult> {
        use glob::glob as glob_pattern;

        let re = Self::word_boundary_regex(symbol_name)?;

        let full_pattern = format!("{}/{}", self.security.workspace_dir().display(), pattern);
        let matches: Vec<PathBuf> = glob_pattern(&full_pattern)?
            .filter_map(|entry| entry.ok())
            .filter(|path| path.is_file())
            .collect();

        if matches.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: format!("No files found matching pattern: {}", pattern),
                error: None,
            });
        }

        let mut results: Vec<String> = Vec::new();
        results.push(format!(
            "Found {} file(s) matching pattern '{}'",
            matches.len(),
            pattern
        ));

        let mut total_renamed = 0;
        let mut files_changed = 0;

        for path in &matches {
            if !path.is_file() {
                continue;
            }

            let path_str = path.to_string_lossy();
            if !self.security.is_path_allowed(&path_str) {
                results.push(format!("  Skipped (policy): {}", path.display()));
                continue;
            }

            let content = match tokio::fs::read_to_string(path).await {
                Ok(c) => c,
                Err(e) => {
                    results.push(format!("  Error reading {}: {}", path.display(), e));
                    continue;
                }
            };

            let count = re.find_iter(&content).count();
            if count == 0 {
                continue;
            }

            if dry_run {
                total_renamed += count;
                results.push(format!("  [DRY] {} ({} match(es))", path.display(), count));
            } else {
                if !self.security.record_action() {
                    results.push(format!("  Rate limited, skipping: {}", path.display()));
                    continue;
                }

                let new_content = re.replace_all(&content, new_name).to_string();
                if let Err(e) = tokio::fs::write(path, &new_content).await {
                    results.push(format!("  Error writing {}: {}", path.display(), e));
                    continue;
                }
                total_renamed += count;
                files_changed += 1;
                results.push(format!(
                    "  Renamed {} match(es) in: {}",
                    count,
                    path.display()
                ));
            }
        }

        if dry_run {
            results.push(format!(
                "\n[DRY RUN] Would rename {} total match(es)",
                total_renamed
            ));
        } else {
            results.push(format!(
                "\nRenamed {} total match(es) across {} file(s)",
                total_renamed, files_changed
            ));
        }

        Ok(ToolResult {
            success: true,
            output: results.join("\n"),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for LspRenameTool {
    fn name(&self) -> &str {
        "lsp_rename"
    }

    fn description(&self) -> &str {
        "Rename a symbol across files. Supports single file rename and glob pattern rename. \
         Use file_path for single file, or glob_pattern for batch rename across multiple files. \
         Provides dry-run mode to preview changes before applying."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file containing the symbol to rename (use either file_path OR glob_pattern)"
                },
                "glob_pattern": {
                    "type": "string",
                    "description": "Glob pattern to match files (e.g., '**/*.rs', 'src/**/*.ts'). Use either glob_pattern OR file_path."
                },
                "symbol_name": {
                    "type": "string",
                    "description": "The current name of the symbol to rename"
                },
                "new_name": {
                    "type": "string",
                    "description": "The new name for the symbol"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "If true, show what would be changed without making edits (default: false)"
                }
            },
            "required": ["symbol_name", "new_name"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: autonomy is read-only".into()),
            });
        }

        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

        let symbol_name = args
            .get("symbol_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'symbol_name' parameter"))?;

        let new_name = args
            .get("new_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_name' parameter"))?;

        let cli_dry_run = crate::util::get_runtime_var("SEN_DRY_RUN").as_deref() == Some("1");
        let dry_run = args
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(cli_dry_run);

        if symbol_name.is_empty() || new_name.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("symbol_name and new_name cannot be empty".into()),
            });
        }

        if symbol_name == new_name {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("symbol_name and new_name must be different".into()),
            });
        }

        if let Some(pattern) = args.get("glob_pattern").and_then(|v| v.as_str()) {
            return self
                .glob_rename(pattern, symbol_name, new_name, dry_run)
                .await;
        }

        let file_path_str = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Either 'file_path' or 'glob_pattern' is required"))?;

        self.validate_write_path(file_path_str)?;

        let file_path = self.resolve_path(file_path_str);

        if !dry_run {
            if let Ok(lsp_result) = self.try_lsp_rename(&file_path, symbol_name, new_name).await {
                return Ok(lsp_result);
            }
        }

        self.text_rename(&file_path, symbol_name, new_name, dry_run)
            .await
    }
}

impl LspRenameTool {

    async fn try_lsp_rename(
        &self,
        file_path: &std::path::PathBuf,
        symbol_name: &str,
        new_name: &str,
    ) -> anyhow::Result<ToolResult> {
        let svc = crate::services::try_get_services()
            .ok_or_else(|| anyhow::anyhow!("Services not initialized"))?;

        let lang = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let file_uri = format!(
            "file://{}",
            file_path.display().to_string().replace('\\', "/")
        );

        let content = tokio::fs::read_to_string(file_path).await?;
        let (line, character) = content
            .lines()
            .enumerate()
            .find_map(|(i, l)| l.find(symbol_name).map(|col| (i as u32, col as u32)))
            .ok_or_else(|| anyhow::anyhow!("Symbol not found in file for LSP rename"))?;

        let params = serde_json::json!({
            "textDocument": { "uri": file_uri },
            "position": { "line": line, "character": character },
            "newName": new_name
        });

        match svc
            .lsp
            .request(
                lang,
                &self.security.workspace_dir(),
                Some(file_path),
                "textDocument/rename",
                params,
            )
            .await
        {
            Ok(resp) => {

                let edits_applied = tokio::task::spawn_blocking(move || apply_workspace_edit(&resp))
                    .await
                    .unwrap_or(0);
                if edits_applied > 0 {
                    Ok(ToolResult {
                        success: true,
                        output: format!(
                            "LSP rename: '{}' -> '{}' ({} edits applied via language server)",
                            symbol_name, new_name, edits_applied
                        ),
                        error: None,
                    })
                } else {
                    Err(anyhow::anyhow!("LSP rename returned no edits"))
                }
            }
            Err(e) => Err(anyhow::anyhow!("LSP rename failed: {e}")),
        }
    }
}

fn apply_workspace_edit(resp: &serde_json::Value) -> usize {
    let mut edits_applied = 0;
    let changes = resp.get("changes").and_then(|v| v.as_object());
    if let Some(changes) = changes {
        for (uri, edits_val) in changes {
            let file = uri
                .trim_start_matches("file:///")
                .trim_start_matches("file://")
                .replace('/', std::path::MAIN_SEPARATOR_STR);
            if let Ok(content) = std::fs::read_to_string(&file) {
                if let Some(edits) = edits_val.as_array() {
                    let mut lines: Vec<String> = content.lines().map(String::from).collect();
                    let mut sorted: Vec<_> = edits
                        .iter()
                        .filter_map(|e| {
                            let sl = e.pointer("/range/start/line")?.as_u64()? as usize;
                            let sc = e.pointer("/range/start/character")?.as_u64()? as usize;
                            let el = e.pointer("/range/end/line")?.as_u64()? as usize;
                            let ec = e.pointer("/range/end/character")?.as_u64()? as usize;
                            let new_text = e.get("newText")?.as_str()?.to_string();
                            Some((sl, sc, el, ec, new_text))
                        })
                        .collect();
                    sorted.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
                    for (sl, sc, el, ec, new_text) in sorted {
                        let sl = sl.min(lines.len().saturating_sub(1));
                        let el = el.min(lines.len().saturating_sub(1));
                        if sl == el {
                            if let Some(line) = lines.get_mut(sl) {
                                let chars: Vec<char> = line.chars().collect();
                                let sc = sc.min(chars.len());
                                let ec = ec.min(chars.len());
                                let before: String = chars[..sc].iter().collect();
                                let after: String = chars[ec..].iter().collect();
                                *line = format!("{before}{new_text}{after}");
                                edits_applied += 1;
                            }
                        }
                    }
                    let _ = std::fs::write(&file, lines.join("\n"));
                }
            }
        }
    }
    edits_applied
}
