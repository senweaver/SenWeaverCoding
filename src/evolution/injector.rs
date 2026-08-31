// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use super::types::Lesson;
use super::EvolutionEngine;

const APPROX_CHARS_PER_TOKEN: usize = 4;

const LESSON_BLOCK_TTL: std::time::Duration = std::time::Duration::from_secs(120);

type LessonBlockEntry = (std::time::Instant, Option<(String, Vec<String>)>);

fn lesson_block_cache()
-> &'static parking_lot::Mutex<std::collections::HashMap<String, LessonBlockEntry>> {
    static CACHE: std::sync::OnceLock<
        parking_lot::Mutex<std::collections::HashMap<String, LessonBlockEntry>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| parking_lot::Mutex::new(std::collections::HashMap::new()))
}

fn last_built_ids()
-> &'static parking_lot::Mutex<std::collections::HashMap<String, Vec<String>>> {
    static LAST: std::sync::OnceLock<
        parking_lot::Mutex<std::collections::HashMap<String, Vec<String>>>,
    > = std::sync::OnceLock::new();
    LAST.get_or_init(|| parking_lot::Mutex::new(std::collections::HashMap::new()))
}

pub fn last_injected_lesson_ids(coding_mode: Option<&str>) -> Vec<String> {
    let cache_key = coding_mode.unwrap_or("__any__");
    last_built_ids()
        .lock()
        .get(cache_key)
        .cloned()
        .unwrap_or_default()
}

pub fn invalidate_lesson_cache() {
    lesson_block_cache().lock().clear();
}

pub fn current_lesson_block_chars(coding_mode: Option<&str>) -> usize {
    let cache_key = coding_mode.unwrap_or("__any__");
    let cache = lesson_block_cache().lock();
    cache
        .get(cache_key)
        .and_then(|(cached_at, entry)| {
            if cached_at.elapsed() < LESSON_BLOCK_TTL {
                entry.as_ref().map(|(block, _)| block.chars().count())
            } else {
                None
            }
        })
        .unwrap_or(0)
}

pub fn build_lesson_block(engine: &Arc<EvolutionEngine>, coding_mode: Option<&str>) -> Option<String> {
    let snapshot = engine.config_snapshot();
    if !snapshot.enabled {
        return None;
    }
    let cache_key = coding_mode.unwrap_or("__any__").to_string();
    let cached: Option<Option<(String, Vec<String>)>> = {
        let cache = lesson_block_cache().lock();
        cache.get(&cache_key).and_then(|(cached_at, entry)| {
            if cached_at.elapsed() < LESSON_BLOCK_TTL {
                Some(entry.clone())
            } else {
                None
            }
        })
    };
    let entry = match cached {
        Some(entry) => entry,
        None => {
            let built = build_lesson_block_uncached(engine, coding_mode, &snapshot);
            lesson_block_cache()
                .lock()
                .insert(cache_key, (std::time::Instant::now(), built.clone()));
            built
        }
    };
    let (block, ids) = entry?;
    last_built_ids()
        .lock()
        .insert(coding_mode.unwrap_or("__any__").to_string(), ids.clone());
    let _ = engine.store().bump_lesson_hits(&ids);
    super::collector::record_injected_lessons(&ids);
    Some(block)
}

fn build_lesson_block_uncached(
    engine: &Arc<EvolutionEngine>,
    coding_mode: Option<&str>,
    snapshot: &super::EvolutionConfig,
) -> Option<(String, Vec<String>)> {
    let max_lessons = snapshot.max_lessons_in_prompt.max(1);
    let token_budget = snapshot.lesson_token_budget.max(64);
    let char_budget = token_budget.saturating_mul(APPROX_CHARS_PER_TOKEN);
    let lessons = engine.store().list_lessons(true).ok()?;
    if lessons.is_empty() {
        return None;
    }
    let scored = rank_lessons(lessons, coding_mode);
    let mut chosen: Vec<&Lesson> = Vec::new();
    let mut used_chars: usize = 0;
    for entry in &scored {
        let approx = entry.title.len() + entry.body.len() + 16;
        if used_chars + approx > char_budget && !chosen.is_empty() {
            break;
        }
        chosen.push(entry);
        used_chars += approx;
        if chosen.len() >= max_lessons {
            break;
        }
    }
    if chosen.is_empty() {
        return None;
    }
    let ids: Vec<String> = chosen.iter().map(|l| l.id.clone()).collect();
    let mut buf = String::from(
        "## Lessons learned from prior runs\n\n\
         The following compact lessons were distilled from previous successful turns. \
         Treat them as soft hints; ignore any that conflict with the current task or with \
         user instructions:\n\n",
    );
    for lesson in chosen {
        buf.push_str("- **");
        buf.push_str(lesson.title.trim());
        buf.push_str("**  -  ");
        buf.push_str(lesson.body.trim());
        if !lesson.tags.is_empty() {
            buf.push_str(" _(");
            buf.push_str(&lesson.tags.join(", "));
            buf.push_str(")_");
        }
        buf.push('\n');
    }
    Some((buf, ids))
}

fn rank_lessons(lessons: Vec<Lesson>, coding_mode: Option<&str>) -> Vec<Lesson> {
    let mut with_score: Vec<(f32, Lesson)> = lessons
        .into_iter()
        .map(|l| (score_lesson(&l, coding_mode), l))
        .collect();
    with_score.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    with_score.into_iter().map(|(_, l)| l).collect()
}

fn score_lesson(lesson: &Lesson, coding_mode: Option<&str>) -> f32 {
    let mut score: f32 = 0.0;
    match (&lesson.coding_mode, coding_mode) {
        (Some(lm), Some(active)) if lm.eq_ignore_ascii_case(active) => score += 5.0,
        (Some(_), Some(_)) => score -= 1.0,
        (None, _) => score += 0.5,
        _ => {}
    }
    score += (lesson.hits.min(20) as f32) * 0.1;
    score
}
