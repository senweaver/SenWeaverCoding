// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use super::types::Lesson;
use super::EvolutionEngine;

const APPROX_CHARS_PER_TOKEN: usize = 4;

const LESSON_BLOCK_TTL: std::time::Duration = std::time::Duration::from_secs(120);

fn lesson_block_cache()
-> &'static parking_lot::Mutex<std::collections::HashMap<String, (std::time::Instant, Option<String>)>>
{
    static CACHE: std::sync::OnceLock<
        parking_lot::Mutex<
            std::collections::HashMap<String, (std::time::Instant, Option<String>)>,
        >,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| parking_lot::Mutex::new(std::collections::HashMap::new()))
}

pub fn build_lesson_block(engine: &Arc<EvolutionEngine>, coding_mode: Option<&str>) -> Option<String> {
    let snapshot = engine.config_snapshot();
    if !snapshot.enabled {
        return None;
    }
    let cache_key = coding_mode.unwrap_or("__any__").to_string();
    {
        let cache = lesson_block_cache().lock();
        if let Some((cached_at, block)) = cache.get(&cache_key) {
            if cached_at.elapsed() < LESSON_BLOCK_TTL {
                return block.clone();
            }
        }
    }
    let block = build_lesson_block_uncached(engine, coding_mode, &snapshot);
    lesson_block_cache()
        .lock()
        .insert(cache_key, (std::time::Instant::now(), block.clone()));
    block
}

fn build_lesson_block_uncached(
    engine: &Arc<EvolutionEngine>,
    coding_mode: Option<&str>,
    snapshot: &super::EvolutionConfig,
) -> Option<String> {
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
    let _ = engine
        .store()
        .bump_lesson_hits(&chosen.iter().map(|l| l.id.clone()).collect::<Vec<_>>());
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
    Some(buf)
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
