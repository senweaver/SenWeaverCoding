// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::Utc;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::lesson::{ReflectionLesson, ReflectionLessonKind};
use super::reflector::ReflectionRequest;
use super::types::ReflectionWritebackReport;
use crate::config::domain::evolution::{ReflectionWritebackTarget, SelfReflectionConfig};
use crate::evolution::types::Lesson;
use crate::evolution::EvolutionEngine;

pub async fn apply_writeback(
    engine: &Arc<EvolutionEngine>,
    lessons: &[ReflectionLesson],
    cfg: &SelfReflectionConfig,
    req: &ReflectionRequest,
) -> ReflectionWritebackReport {
    let mut report = ReflectionWritebackReport::default();
    if lessons.is_empty() {
        return report;
    }
    let store = engine.store();
    let max_total = cfg.max_total_lessons.max(1);
    let existing_count = store
        .list_lessons(false)
        .map(|l| l.len())
        .unwrap_or(0);
    let mut budget = max_total.saturating_sub(existing_count);
    let targets: &[ReflectionWritebackTarget] = if cfg.writeback_targets.is_empty() {
        &[ReflectionWritebackTarget::Lessons]
    } else {
        cfg.writeback_targets.as_slice()
    };
    for lesson in lessons {
        if budget == 0 && targets.contains(&ReflectionWritebackTarget::Lessons)
        {
            report
                .errors
                .push("lesson_budget_exhausted".to_string());
            break;
        }
        for target in targets {
            match target {
                ReflectionWritebackTarget::Lessons => {
                    match write_to_lessons_table(engine, lesson, req) {
                        Ok(true) => {
                            report.lessons_written = report.lessons_written.saturating_add(1);
                            budget = budget.saturating_sub(1);
                        }
                        Ok(false) => {}
                        Err(error) => {
                            report.errors.push(format!("lessons:{error}"));
                        }
                    }
                }
                ReflectionWritebackTarget::Skills => {
                    match write_to_skill_files(lesson) {
                        Ok(true) => {
                            report.skills_written = report.skills_written.saturating_add(1);
                        }
                        Ok(false) => {}
                        Err(error) => {
                            report.errors.push(format!("skills:{error}"));
                        }
                    }
                }
                ReflectionWritebackTarget::Rules => {
                    match write_to_rule_files(lesson) {
                        Ok(true) => {
                            report.rules_written = report.rules_written.saturating_add(1);
                        }
                        Ok(false) => {}
                        Err(error) => {
                            report.errors.push(format!("rules:{error}"));
                        }
                    }
                }
                ReflectionWritebackTarget::Memory => {
                    match write_to_memory(engine, lesson) {
                        Ok(true) => {
                            report.memory_written = report.memory_written.saturating_add(1);
                        }
                        Ok(false) => {}
                        Err(error) => {
                            report.errors.push(format!("memory:{error}"));
                        }
                    }
                }
            }
        }
    }
    report
}

fn write_to_lessons_table(
    engine: &Arc<EvolutionEngine>,
    lesson: &ReflectionLesson,
    req: &ReflectionRequest,
) -> anyhow::Result<bool> {
    let store = engine.store();
    let coding_mode = req.coding_mode.clone();
    if store.lesson_exists_by_title(coding_mode.as_deref(), &lesson.title)? {
        return Ok(false);
    }
    let mut tags = lesson.tags.clone();
    tags.push(format!("reflection:{}", lesson.kind.as_str()));
    let source_turn_ids: Vec<String> = req
        .turns
        .iter()
        .map(|t| t.id.clone())
        .take(6)
        .collect();
    let record = Lesson {
        id: format!("lesson_{}", uuid::Uuid::new_v4().simple()),
        title: lesson.title.clone(),
        body: lesson.body.clone(),
        tags,
        coding_mode,
        source_turn_ids,
        hits: 0,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    store.upsert_lesson(&record)?;
    Ok(true)
}

fn write_to_skill_files(lesson: &ReflectionLesson) -> anyhow::Result<bool> {
    let dir = match skills_root() {
        Some(dir) => dir,
        None => anyhow::bail!("writeback_skill_skipped: home_dir_unavailable"),
    };
    std::fs::create_dir_all(&dir)?;
    let slug = slugify(&lesson.title);
    if slug.is_empty() {
        anyhow::bail!("writeback_skill_skipped: empty_slug");
    }
    let target = dir.join(format!("reflection-{slug}.md"));
    if target.exists() {
        return Ok(false);
    }
    let body = render_markdown(lesson);
    std::fs::write(&target, body)?;
    Ok(true)
}

fn write_to_rule_files(lesson: &ReflectionLesson) -> anyhow::Result<bool> {
    let dir = match rules_root() {
        Some(dir) => dir,
        None => anyhow::bail!("writeback_rule_skipped: home_dir_unavailable"),
    };
    std::fs::create_dir_all(&dir)?;
    let slug = slugify(&lesson.title);
    if slug.is_empty() {
        anyhow::bail!("writeback_rule_skipped: empty_slug");
    }
    let target = dir.join(format!("reflection-{slug}.md"));
    if target.exists() {
        return Ok(false);
    }
    let body = render_rule_markdown(lesson);
    std::fs::write(&target, body)?;
    Ok(true)
}

fn write_to_memory(engine: &Arc<EvolutionEngine>, lesson: &ReflectionLesson) -> anyhow::Result<bool> {
    let dir = engine.workspace_dir().join("memory").join("reflection");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("self_reflection_lessons.md");
    let mut contents = String::new();
    if path.exists() {
        contents = std::fs::read_to_string(&path).unwrap_or_default();
    } else {
        contents.push_str("# Reflection Lessons\n\n");
    }
    let block = render_memory_block(lesson);
    if contents.contains(lesson.title.trim()) {
        return Ok(false);
    }
    contents.push_str(&block);
    std::fs::write(&path, contents)?;
    Ok(true)
}

fn render_markdown(lesson: &ReflectionLesson) -> String {
    let mut buf = String::new();
    buf.push_str("---\n");
    buf.push_str(&format!("name: {}\n", lesson.title));
    if !lesson.tags.is_empty() {
        buf.push_str(&format!("tags: [{}]\n", lesson.tags.join(", ")));
    }
    buf.push_str(&format!("kind: {}\n", lesson.kind.as_str()));
    buf.push_str("source: reflection\n");
    buf.push_str("---\n\n");
    buf.push_str(&format!("# {}\n\n", lesson.title));
    buf.push_str(&lesson.body);
    buf.push('\n');
    buf
}

fn render_rule_markdown(lesson: &ReflectionLesson) -> String {
    let mut buf = String::new();
    buf.push_str("---\n");
    buf.push_str(&format!("name: {}\n", lesson.title));
    buf.push_str(match lesson.kind {
        ReflectionLessonKind::Avoid => "alwaysApply: true\n",
        _ => "alwaysApply: false\n",
    });
    buf.push_str(&format!("description: reflection {}\n", lesson.kind.as_str()));
    if !lesson.tags.is_empty() {
        buf.push_str(&format!("tags: [{}]\n", lesson.tags.join(", ")));
    }
    buf.push_str("---\n\n");
    buf.push_str(&format!("# {}\n\n", lesson.title));
    buf.push_str(&lesson.body);
    buf.push('\n');
    buf
}

fn render_memory_block(lesson: &ReflectionLesson) -> String {
    let mut buf = String::new();
    buf.push_str(&format!("\n## {} ({})\n\n", lesson.title, lesson.kind.as_str()));
    buf.push_str(&lesson.body);
    if !lesson.tags.is_empty() {
        buf.push_str(&format!("\n\n_tags: {}_", lesson.tags.join(", ")));
    }
    buf.push('\n');
    buf
}

fn skills_root() -> Option<PathBuf> {
    home_dir().map(|h| {
        h.join(".senweavercoding")
            .join("skills")
            .join("reflection")
    })
}

fn rules_root() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".senweavercoding").join("rules"))
}

fn home_dir() -> Option<PathBuf> {
    if let Ok(h) = std::env::var("HOME") {
        if !h.is_empty() {
            return Some(PathBuf::from(h));
        }
    }
    if let Ok(h) = std::env::var("USERPROFILE") {
        if !h.is_empty() {
            return Some(PathBuf::from(h));
        }
    }
    None
}

fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = false;
    for ch in input.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    let mut clipped: String = trimmed.chars().take(80).collect();
    if clipped.is_empty() {
        clipped.push_str("untitled");
    }
    let _: Option<&Path> = None;
    clipped
}
