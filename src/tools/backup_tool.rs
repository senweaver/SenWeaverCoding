// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;

pub struct BackupTool {
    workspace_dir: PathBuf,
    include_dirs: Vec<String>,
    max_keep: usize,
}

impl BackupTool {
    pub fn new(workspace_dir: PathBuf, include_dirs: Vec<String>, max_keep: usize) -> Self {
        Self {
            workspace_dir,
            include_dirs,
            max_keep,
        }
    }

    fn backups_dir(&self) -> PathBuf {
        self.workspace_dir.join("backups")
    }

    async fn cmd_create(&self) -> anyhow::Result<ToolResult> {
        let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
        let name = format!("backup-{ts}");
        let backup_dir = self.backups_dir().join(&name);
        fs::create_dir_all(&backup_dir).await?;

        for sub in &self.include_dirs {
            let src = self.workspace_dir.join(sub);
            if src.is_dir() {
                let dst = backup_dir.join(sub);
                copy_dir_recursive(&src, &dst).await?;
            }
        }

        let checksums = compute_checksums(&backup_dir).await?;
        let file_count = checksums.len();
        let manifest = serde_json::to_string_pretty(&checksums)?;
        fs::write(backup_dir.join("manifest.json"), &manifest).await?;

        self.enforce_max_keep().await?;

        Ok(ToolResult {
            success: true,
            output: json!({
                "backup": name,
                "file_count": file_count,
            })
            .to_string(),
            error: None,
        })
    }

    async fn enforce_max_keep(&self) -> anyhow::Result<()> {
        let mut backups = self.list_backup_dirs().await?;

        while backups.len() > self.max_keep {
            if let Some(old) = backups.pop() {
                fs::remove_dir_all(old).await?;
            }
        }
        Ok(())
    }

    async fn list_backup_dirs(&self) -> anyhow::Result<Vec<PathBuf>> {
        let dir = self.backups_dir();
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        let mut rd = fs::read_dir(&dir).await?;
        while let Some(e) = rd.next_entry().await? {
            let p = e.path();
            if p.is_dir() && e.file_name().to_string_lossy().starts_with("backup-") {
                entries.push(p);
            }
        }
        entries.sort();
        entries.reverse();
        Ok(entries)
    }

    async fn cmd_list(&self) -> anyhow::Result<ToolResult> {
        let dirs = self.list_backup_dirs().await?;
        let mut items = Vec::new();
        for d in &dirs {
            let name = d
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let manifest_path = d.join("manifest.json");
            let file_count = if manifest_path.is_file() {
                let data = fs::read_to_string(&manifest_path).await?;
                let map: HashMap<String, String> = serde_json::from_str(&data).unwrap_or_default();
                map.len()
            } else {
                0
            };
            let meta = fs::metadata(d).await?;
            let created = meta
                .created()
                .or_else(|_| meta.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            let dt: chrono::DateTime<chrono::Utc> = created.into();
            items.push(json!({
                "name": name,
                "file_count": file_count,
                "created": dt.to_rfc3339(),
            }));
        }
        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(&items)?,
            error: None,
        })
    }

    async fn cmd_verify(&self, backup_name: &str) -> anyhow::Result<ToolResult> {
        let backup_dir = self.backups_dir().join(backup_name);
        if !backup_dir.is_dir() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Backup not found: {backup_name}")),
            });
        }
        let manifest_path = backup_dir.join("manifest.json");
        let data = fs::read_to_string(&manifest_path).await?;
        let expected: HashMap<String, String> = serde_json::from_str(&data)?;
        let actual = compute_checksums(&backup_dir).await?;

        let mut mismatches = Vec::new();
        for (path, expected_hash) in &expected {
            match actual.get(path) {
                Some(actual_hash) if actual_hash == expected_hash => {}
                Some(actual_hash) => mismatches.push(json!({
                    "file": path,
                    "expected": expected_hash,
                    "actual": actual_hash,
                })),
                None => mismatches.push(json!({
                    "file": path,
                    "error": "missing",
                })),
            }
        }
        let pass = mismatches.is_empty();
        Ok(ToolResult {
            success: pass,
            output: json!({
                "backup": backup_name,
                "pass": pass,
                "checked": expected.len(),
                "mismatches": mismatches,
            })
            .to_string(),
            error: if pass {
                None
            } else {
                Some("Integrity check failed".into())
            },
        })
    }

    async fn cmd_restore(&self, backup_name: &str, confirm: bool) -> anyhow::Result<ToolResult> {
        let backup_dir = self.backups_dir().join(backup_name);
        if !backup_dir.is_dir() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Backup not found: {backup_name}")),
            });
        }

        let mut restore_items: Vec<String> = Vec::new();
        let mut rd = fs::read_dir(&backup_dir).await?;
        while let Some(e) = rd.next_entry().await? {
            let name = e.file_name().to_string_lossy().to_string();
            if name == "manifest.json" {
                continue;
            }
            if e.path().is_dir() {
                restore_items.push(name);
            }
        }

        if !confirm {
            return Ok(ToolResult {
                success: true,
                output: json!({
                    "dry_run": true,
                    "backup": backup_name,
                    "would_restore": restore_items,
                })
                .to_string(),
                error: None,
            });
        }

        for sub in &restore_items {
            let src = backup_dir.join(sub);
            let dst = self.workspace_dir.join(sub);
            copy_dir_recursive(&src, &dst).await?;
        }
        Ok(ToolResult {
            success: true,
            output: json!({
                "restored": backup_name,
                "directories": restore_items,
            })
            .to_string(),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for BackupTool {
    fn name(&self) -> &str {
        "backup"
    }

    fn description(&self) -> &str {
        "Create, list, verify, and restore workspace backups"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "enum": ["create", "list", "verify", "restore"],
                    "description": "Backup command to execute"
                },
                "backup_name": {
                    "type": "string",
                    "description": "Name of backup (for verify/restore)"
                },
                "confirm": {
                    "type": "boolean",
                    "description": "Confirm restore (required for actual restore, default false)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let command = match args.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Missing 'command' parameter".into()),
                });
            }
        };

        match command {
            "create" => self.cmd_create().await,
            "list" => self.cmd_list().await,
            "verify" => {
                let name = args
                    .get("backup_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing 'backup_name' for verify"))?;
                self.cmd_verify(name).await
            }
            "restore" => {
                let name = args
                    .get("backup_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing 'backup_name' for restore"))?;
                let confirm = args
                    .get("confirm")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.cmd_restore(name, confirm).await
            }
            other => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Unknown command: {other}")),
            }),
        }
    }
}

async fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst).await?;
    let mut rd = fs::read_dir(src).await?;
    while let Some(entry) = rd.next_entry().await? {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            fs::copy(&src_path, &dst_path).await?;
        }
    }
    Ok(())
}

async fn compute_checksums(dir: &Path) -> anyhow::Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    let base = dir.to_path_buf();
    walk_and_hash(&base, dir, &mut map).await?;
    Ok(map)
}

async fn walk_and_hash(
    base: &Path,
    dir: &Path,
    map: &mut HashMap<String, String>,
) -> anyhow::Result<()> {
    let mut rd = fs::read_dir(dir).await?;
    while let Some(entry) = rd.next_entry().await? {
        let path = entry.path();
        if path.is_dir() {
            Box::pin(walk_and_hash(base, &path, map)).await?;
        } else {
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if rel == "manifest.json" {
                continue;
            }
            let bytes = fs::read(&path).await?;
            let hash = hex::encode(Sha256::digest(&bytes));
            map.insert(rel, hash);
        }
    }
    Ok(())
}
