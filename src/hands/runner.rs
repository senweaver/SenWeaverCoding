// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use chrono::Utc;
use tokio::time::{self, Duration};

use crate::config::Config;
use crate::cron::next_run_for_schedule;
use crate::observability::traits::ObserverMetric;
use crate::security::SecurityPolicy;

use super::types::{Hand, HandContext, HandRun, HandRunStatus};
use super::{load_hand_context, load_hands, save_hand_context};

const COMPONENT: &str = "hands";
const MIN_POLL_SECONDS: u64 = 5;
const MAX_FINDING_CHARS: usize = 4_000;

pub fn resolve_hands_dir(config: &Config) -> PathBuf {
    if let Some(dir) = config
        .hands
        .dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let path = PathBuf::from(dir);
        if path.is_absolute() {
            return path;
        }
        return config.workspace_dir.join(path);
    }

    config
        .config_path
        .parent()
        .map(|p| p.join("hands"))
        .unwrap_or_else(|| config.workspace_dir.join("hands"))
}

pub async fn run(config: Config) -> Result<()> {
    let poll_secs = config.hands.poll_secs.max(MIN_POLL_SECONDS);
    let mut interval = time::interval(Duration::from_secs(poll_secs));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    crate::health::mark_component_ok(COMPONENT);
    tracing::info!(
        dir = %resolve_hands_dir(&config).display(),
        poll_secs,
        "Hands worker started"
    );

    loop {
        interval.tick().await;
        crate::health::mark_component_ok(COMPONENT);
        process_due_hands(&config, &security).await;
    }
}

pub async fn process_due_hands(config: &Config, security: &SecurityPolicy) {
    if !config.hands.enabled {
        return;
    }

    let hands_dir = resolve_hands_dir(config);
    let hands = match load_hands(&hands_dir) {
        Ok(hands) => hands,
        Err(e) => {
            crate::health::mark_component_error(COMPONENT, e.to_string());
            tracing::warn!(error = %e, "Hands worker: failed to load hands");
            return;
        }
    };

    if hands.is_empty() {
        return;
    }

    let now = Utc::now();
    for hand in hands {
        if !hand.active {
            continue;
        }

        let mut ctx = match load_hand_context(&hands_dir, &hand.name) {
            Ok(ctx) => ctx,
            Err(e) => {
                tracing::warn!(hand = %hand.name, error = %e, "Hands worker: context load failed");
                continue;
            }
        };

        let due = match ctx.last_run {
            None => true,
            Some(last) => match next_run_for_schedule(&hand.schedule, last) {
                Ok(next) => next <= now,
                Err(e) => {
                    tracing::warn!(hand = %hand.name, error = %e, "Hands worker: invalid schedule");
                    false
                }
            },
        };

        if !due {
            continue;
        }

        run_due_hand(config, security, &hand, &mut ctx, &hands_dir).await;
    }
}

fn build_hand_prompt(hand: &Hand, ctx: &HandContext) -> String {
    let mut prompt = format!("[hand:{}] {}", hand.name, hand.prompt);

    if !hand.knowledge.is_empty() {
        prompt.push_str("\n\nReference knowledge:");
        for item in &hand.knowledge {
            prompt.push_str(&format!("\n- {item}"));
        }
    }

    if !ctx.learned_facts.is_empty() {
        prompt.push_str("\n\nPreviously learned facts:");
        for fact in &ctx.learned_facts {
            prompt.push_str(&format!("\n- {fact}"));
        }
    }

    prompt
}

async fn run_due_hand(
    config: &Config,
    security: &SecurityPolicy,
    hand: &Hand,
    ctx: &mut HandContext,
    hands_dir: &std::path::Path,
) {
    if !security.can_act() {
        tracing::debug!(hand = %hand.name, "Hands worker: skipping, autonomy is read-only");
        return;
    }
    if security.is_rate_limited() {
        tracing::debug!(hand = %hand.name, "Hands worker: skipping, rate limited");
        return;
    }
    if !security.record_action() {
        tracing::debug!(hand = %hand.name, "Hands worker: skipping, action budget exhausted");
        return;
    }

    let started_at = Utc::now();
    let run_id = format!("{}-{}", hand.name, started_at.timestamp_millis());
    let prompt = build_hand_prompt(hand, ctx);

    let mut effective_config = config.clone();
    effective_config.workspace_dir = config.workspace_dir.clone();

    let started = Instant::now();
    let run_result = Box::pin(crate::agent::run(
        effective_config,
        Some(prompt),
        None,
        hand.model.clone(),
        config.default_temperature,
        Vec::new(),
        false,
        None,
        hand.allowed_tools.clone(),
        None,
    ))
    .await;
    let duration = started.elapsed();
    let finished_at = Utc::now();

    let (status, findings, success) = match run_result {
        Ok(response) => {
            let trimmed = response.trim();
            let findings = if trimmed.is_empty() {
                Vec::new()
            } else {
                let mut snippet: String = trimmed.chars().take(MAX_FINDING_CHARS).collect();
                if trimmed.chars().count() > MAX_FINDING_CHARS {
                    snippet.push_str("...");
                }
                vec![snippet]
            };
            (HandRunStatus::Completed, findings, true)
        }
        Err(e) => (
            HandRunStatus::Failed {
                error: format!("{e:#}"),
            },
            Vec::new(),
            false,
        ),
    };

    let findings_count = findings.len() as u64;
    let run = HandRun {
        hand_name: hand.name.clone(),
        run_id,
        started_at,
        finished_at: Some(finished_at),
        status,
        findings,
        knowledge_added: Vec::new(),
        duration_ms: Some(duration.as_millis() as u64),
    };

    ctx.record_run(run, hand.max_history);

    if let Err(e) = save_hand_context(hands_dir, ctx) {
        tracing::warn!(hand = %hand.name, error = %e, "Hands worker: failed to save context");
    }

    if let Some(obs) = crate::observability::global_observer() {
        obs.record_metric(&ObserverMetric::HandRunDuration {
            hand_name: hand.name.clone(),
            duration,
        });
        obs.record_metric(&ObserverMetric::HandFindingsCount {
            hand_name: hand.name.clone(),
            count: findings_count,
        });
        obs.record_metric(&ObserverMetric::HandSuccessRate {
            hand_name: hand.name.clone(),
            success,
        });
    }

    if success {
        tracing::info!(hand = %hand.name, duration_ms = duration.as_millis() as u64, "Hand run completed");
    } else {
        tracing::warn!(hand = %hand.name, "Hand run failed");
    }
}
