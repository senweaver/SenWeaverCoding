// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::config::SkillCreationConfig;
use crate::memory::embeddings::EmbeddingProvider;
use crate::memory::vector::cosine_similarity;
use anyhow::{Context, Result};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub name: String,
    pub args: serde_json::Value,
}

pub struct SkillCreator {
    workspace_dir: PathBuf,
    config: SkillCreationConfig,
}

impl SkillCreator {
    pub fn new(workspace_dir: PathBuf, config: SkillCreationConfig) -> Self {
        Self {
            workspace_dir,
            config,
        }
    }

    pub async fn create_from_execution(
        &self,
        task_description: &str,
        tool_calls: &[ToolCallRecord],
        embedding_provider: Option<&dyn EmbeddingProvider>,
    ) -> Result<Option<String>> {
        if !self.config.enabled {
            return Ok(None);
        }

        if tool_calls.len() < 2 {
            return Ok(None);
        }

        if let Some(provider) = embedding_provider {
            if provider.name() != "none" && self.is_duplicate(task_description, provider).await? {
                return Ok(None);
            }
        }

        let slug = Self::generate_slug(task_description);
        if !Self::validate_slug(&slug) {
            return Ok(None);
        }

        self.enforce_lru_limit().await?;

        let skill_dir = self.skills_dir().join(&slug);
        tokio::fs::create_dir_all(&skill_dir)
            .await
            .with_context(|| {
                format!("Failed to create skill directory: {}", skill_dir.display())
            })?;

        let toml_content = Self::generate_skill_toml(&slug, task_description, tool_calls);
        let toml_path = skill_dir.join("SKILL.toml");
        tokio::fs::write(&toml_path, toml_content.as_bytes())
            .await
            .with_context(|| format!("Failed to write {}", toml_path.display()))?;

        Ok(Some(slug))
    }

    fn generate_slug(description: &str) -> String {
        let slug: String = description
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();

        let mut collapsed = String::with_capacity(slug.len());
        let mut prev_hyphen = false;
        for c in slug.chars() {
            if c == '-' {
                if !prev_hyphen {
                    collapsed.push('-');
                }
                prev_hyphen = true;
            } else {
                collapsed.push(c);
                prev_hyphen = false;
            }
        }

        let trimmed = collapsed.trim_matches('-');
        if trimmed.len() > 64 {

            let safe_index = trimmed
                .char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i <= 64)
                .last()
                .unwrap_or(0);
            let truncated = &trimmed[..safe_index];
            truncated.trim_end_matches('-').to_string()
        } else {
            trimmed.to_string()
        }
    }

    fn validate_slug(slug: &str) -> bool {
        !slug.is_empty()
            && slug.len() <= 64
            && slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !slug.starts_with('-')
            && !slug.ends_with('-')
    }

    fn generate_skill_toml(slug: &str, description: &str, tool_calls: &[ToolCallRecord]) -> String {
        use std::fmt::Write;
        let mut toml = String::new();
        toml.push_str("[skill]\n");
        let _ = writeln!(toml, "name = {}", toml_escape(slug));
        let _ = writeln!(
            toml,
            "description = {}",
            toml_escape(&format!("Auto-generated: {description}"))
        );
        toml.push_str("version = \"0.1.0\"\n");
        toml.push_str("author = \"sen-auto\"\n");
        toml.push_str("tags = [\"auto-generated\"]\n");

        for call in tool_calls {
            toml.push('\n');
            toml.push_str("[[tools]]\n");
            let _ = writeln!(toml, "name = {}", toml_escape(&call.name));
            let _ = writeln!(
                toml,
                "description = {}",
                toml_escape(&format!("Tool used in task: {}", call.name))
            );
            toml.push_str("kind = \"shell\"\n");

            let command = call
                .args
                .get("command")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&call.name);
            let _ = writeln!(toml, "command = {}", toml_escape(command));
        }

        toml
    }

    async fn is_duplicate(
        &self,
        description: &str,
        embedding_provider: &dyn EmbeddingProvider,
    ) -> Result<bool> {
        let new_embedding = embedding_provider.embed_one(description).await?;
        if new_embedding.is_empty() {
            return Ok(false);
        }

        let skills_dir = self.skills_dir();
        if !skills_dir.exists() {
            return Ok(false);
        }

        let mut entries = tokio::fs::read_dir(&skills_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let toml_path = entry.path().join("SKILL.toml");
            if !toml_path.exists() {
                continue;
            }

            let content = tokio::fs::read_to_string(&toml_path).await?;

            if let Some(desc) = extract_description_from_toml(&content) {
                let existing_embedding = embedding_provider.embed_one(&desc).await?;
                if !existing_embedding.is_empty() {
                    #[allow(clippy::cast_possible_truncation)]
                    let similarity =
                        f64::from(cosine_similarity(&new_embedding, &existing_embedding));
                    if similarity > self.config.similarity_threshold {
                        return Ok(true);
                    }
                }
            }
        }

        Ok(false)
    }

    async fn enforce_lru_limit(&self) -> Result<()> {
        let skills_dir = self.skills_dir();
        if !skills_dir.exists() {
            return Ok(());
        }

        let mut auto_skills: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();

        let mut entries = tokio::fs::read_dir(&skills_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let toml_path = entry.path().join("SKILL.toml");
            if !toml_path.exists() {
                continue;
            }

            let content = tokio::fs::read_to_string(&toml_path).await?;
            if content.contains("\"sen-auto\"") || content.contains("\"auto-generated\"") {
                let modified = tokio::fs::metadata(&toml_path)
                    .await?
                    .modified()
                    .unwrap_or(std::time::UNIX_EPOCH);
                auto_skills.push((entry.path(), modified));
            }
        }

        if auto_skills.len() >= self.config.max_skills {
            auto_skills.sort_by_key(|(_, modified)| *modified);
            if let Some((oldest_dir, _)) = auto_skills.first() {
                tokio::fs::remove_dir_all(oldest_dir)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to remove oldest auto-generated skill: {}",
                            oldest_dir.display()
                        )
                    })?;
            }
        }

        Ok(())
    }

    fn skills_dir(&self) -> PathBuf {
        self.workspace_dir.join("skills")
    }
}

fn toml_escape(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

fn extract_description_from_toml(content: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Partial {
        skill: PartialSkill,
    }
    #[derive(serde::Deserialize)]
    struct PartialSkill {
        description: Option<String>,
    }
    toml::from_str::<Partial>(content)
        .ok()
        .and_then(|p| p.skill.description)
}

pub fn extract_tool_calls_from_history(
    history: &[crate::providers::ChatMessage],
) -> Vec<ToolCallRecord> {
    let mut records = Vec::new();

    for msg in history {
        if msg.role != "assistant" {
            continue;
        }

        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&msg.content) {
            if let Some(tool_calls) = value.get("tool_calls").and_then(|v| v.as_array()) {
                for call in tool_calls {
                    if let Some(function) = call.get("function") {
                        let name = function
                            .get("name")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let args_str = function
                            .get("arguments")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("{}");
                        let args = serde_json::from_str(args_str).unwrap_or_default();
                        if !name.is_empty() {
                            records.push(ToolCallRecord { name, args });
                        }
                    }
                }
            }
        }

        let content = &msg.content;
        let mut pos = 0;
        while pos < content.len() {
            if let Some(start) = content[pos..].find('<') {
                let abs_start = pos + start;
                if let Some(end) = content[abs_start..].find('>') {
                    let tag = &content[abs_start + 1..abs_start + end];

                    if tag.starts_with('/') || tag.starts_with('!') || tag.starts_with('?') {
                        pos = abs_start + end + 1;
                        continue;
                    }
                    let tag_name = tag.split_whitespace().next().unwrap_or(tag);
                    let close_tag = format!("</{tag_name}>");
                    if let Some(close_pos) = content[abs_start + end + 1..].find(&close_tag) {
                        let inner = &content[abs_start + end + 1..abs_start + end + 1 + close_pos];
                        let args: serde_json::Value =
                            serde_json::from_str(inner.trim()).unwrap_or_default();

                        if tag_name != "tool_result"
                            && tag_name != "tool_results"
                            && !tag_name.contains(':')
                            && args.is_object()
                            && !args.as_object().map_or(true, |o| o.is_empty())
                        {
                            records.push(ToolCallRecord {
                                name: tag_name.to_string(),
                                args,
                            });
                        }
                        pos = abs_start + end + 1 + close_pos + close_tag.len();
                    } else {
                        pos = abs_start + end + 1;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    records
}
