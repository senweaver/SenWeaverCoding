// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::config::SkillImprovementConfig;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

pub struct SkillImprover {
    workspace_dir: PathBuf,
    config: SkillImprovementConfig,
    cooldowns: HashMap<String, Instant>,
}

impl SkillImprover {
    pub fn new(workspace_dir: PathBuf, config: SkillImprovementConfig) -> Self {
        Self {
            workspace_dir,
            config,
            cooldowns: HashMap::new(),
        }
    }

    pub fn should_improve_skill(&self, slug: &str) -> bool {
        if !self.config.enabled {
            return false;
        }
        if let Some(last) = self.cooldowns.get(slug) {
            let elapsed = Instant::now().saturating_duration_since(*last);
            elapsed.as_secs() >= self.config.cooldown_secs
        } else {
            true
        }
    }

    pub async fn improve_skill(
        &mut self,
        slug: &str,
        improved_content: &str,
        improvement_reason: &str,
    ) -> Result<Option<String>> {
        if !self.should_improve_skill(slug) {
            return Ok(None);
        }

        validate_skill_content(improved_content)?;

        let skill_dir = self.skills_dir().join(slug);
        let toml_path = skill_dir.join("SKILL.toml");

        if !toml_path.exists() {
            bail!("Skill file not found: {}", toml_path.display());
        }

        let existing = tokio::fs::read_to_string(&toml_path)
            .await
            .with_context(|| format!("Failed to read {}", toml_path.display()))?;

        let now = chrono::Utc::now().to_rfc3339();
        let audit_entry = format!(
            "\n# Improvement: {now}\n# Reason: {}\n",
            improvement_reason.replace('\n', " ")
        );

        let updated = append_improvement_metadata(improved_content, &now, improvement_reason);

        let audit_trail = extract_audit_trail(&existing);
        let final_content = if audit_trail.is_empty() {
            format!("{updated}{audit_entry}")
        } else {
            format!("{updated}\n{audit_trail}{audit_entry}")
        };

        let temp_path = skill_dir.join(".SKILL.toml.tmp");
        tokio::fs::write(&temp_path, final_content.as_bytes())
            .await
            .with_context(|| format!("Failed to write temp file: {}", temp_path.display()))?;

        let written = tokio::fs::read_to_string(&temp_path).await?;
        if let Err(e) = validate_skill_content(&written) {

            let _ = tokio::fs::remove_file(&temp_path).await;
            bail!("Validation failed after write: {e}");
        }

        tokio::fs::rename(&temp_path, &toml_path)
            .await
            .with_context(|| {
                format!(
                    "Failed to rename {} to {}",
                    temp_path.display(),
                    toml_path.display()
                )
            })?;

        self.cooldowns.insert(slug.to_string(), Instant::now());

        Ok(Some(slug.to_string()))
    }

    fn skills_dir(&self) -> PathBuf {
        self.workspace_dir.join("skills")
    }
}

pub fn validate_skill_content(content: &str) -> Result<()> {
    if content.trim().is_empty() {
        bail!("Skill content is empty");
    }

    #[derive(serde::Deserialize)]
    struct Partial {
        skill: PartialSkill,
    }
    #[derive(serde::Deserialize)]
    struct PartialSkill {
        name: Option<String>,
    }

    let toml_portion = strip_trailing_comments(content);
    let parsed: Partial = toml::from_str(&toml_portion)
        .with_context(|| "Skill content contains malformed TOML front-matter")?;

    if parsed.skill.name.as_deref().unwrap_or("").is_empty() {
        bail!("Skill TOML missing required 'name' field");
    }

    Ok(())
}

fn append_improvement_metadata(content: &str, timestamp: &str, reason: &str) -> String {

    let tools_pos = content.find("[[tools]]");
    let (skill_section, rest) = match tools_pos {
        Some(pos) => (&content[..pos], &content[pos..]),
        None => (content, ""),
    };

    let skill_section = if skill_section.contains("updated_at") {
        let mut lines: Vec<&str> = skill_section.lines().collect();
        lines.retain(|line| !line.trim_start().starts_with("updated_at"));
        lines.join("\n") + "\n"
    } else {
        skill_section.to_string()
    };

    let escaped_reason = reason.replace('"', "\\\"").replace('\n', " ");
    format!(
        "{skill_section}updated_at = \"{timestamp}\"\nimprovement_reason = \"{escaped_reason}\"\n{rest}"
    )
}

fn extract_audit_trail(content: &str) -> String {
    content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("# Improvement:") || trimmed.starts_with("# Reason:")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_trailing_comments(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut end = lines.len();
    while end > 0 {
        let line = lines[end - 1].trim();
        if line.is_empty() || line.starts_with('#') {
            end -= 1;
        } else {
            break;
        }
    }
    lines[..end].join("\n")
}
