// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Routines engine — event-triggered automation with pattern matching and
//! cooldown enforcement.
//!
//! A **routine** is a lightweight automation rule: when an event matches one of
//! its patterns, the associated action fires (provided cooldown has elapsed).
//! The engine bridges channel messages, cron ticks, webhooks, and system events
//! into the existing SOP pipeline.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::event_matcher::{EventPattern, RoutineEvent, matches_any};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoutineAction {

    Sop { name: String },

    Shell { command: String },

    Message { channel: String, text: String },

    CronJob { job_name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Routine {

    pub name: String,

    #[serde(default)]
    pub description: String,

    pub patterns: Vec<EventPattern>,

    pub action: RoutineAction,

    #[serde(default)]
    pub cooldown_secs: u64,

    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoutinesManifest {
    #[serde(default)]
    pub routines: Vec<Routine>,
}

#[derive(Debug, Clone)]
pub enum RoutineDispatchResult {

    Fired {
        routine_name: String,
        action: RoutineAction,
    },

    Cooldown {
        routine_name: String,
        remaining_secs: u64,
    },

    Disabled { routine_name: String },

    NoMatch,
}

pub struct RoutinesEngine {
    routines: Vec<Routine>,

    cooldowns: HashMap<String, Instant>,
}

impl RoutinesEngine {

    pub fn new(routines: Vec<Routine>) -> Self {
        Self {
            routines,
            cooldowns: HashMap::new(),
        }
    }

    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn len(&self) -> usize {
        self.routines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.routines.is_empty()
    }

    pub fn routines(&self) -> &[Routine] {
        &self.routines
    }

    pub fn add_routine(&mut self, routine: Routine) {
        self.routines.push(routine);
    }

    pub fn remove_routine(&mut self, name: &str) -> bool {
        let before = self.routines.len();
        self.routines.retain(|r| r.name != name);
        self.cooldowns.remove(name);
        self.routines.len() < before
    }

    pub fn dispatch(&mut self, event: &RoutineEvent) -> Vec<RoutineDispatchResult> {
        let mut results = Vec::new();
        let now = Instant::now();

        for routine in &self.routines {
            if !matches_any(&routine.patterns, event) {
                continue;
            }

            if !routine.enabled {
                debug!(routine = %routine.name, "routine matched but disabled");
                results.push(RoutineDispatchResult::Disabled {
                    routine_name: routine.name.clone(),
                });
                continue;
            }

            if routine.cooldown_secs > 0 {
                if let Some(last_fired) = self.cooldowns.get(&routine.name) {
                    let elapsed = now.saturating_duration_since(*last_fired);
                    let cooldown = Duration::from_secs(routine.cooldown_secs);
                    if elapsed < cooldown {
                        let remaining = cooldown.saturating_sub(elapsed).as_secs();
                        debug!(
                            routine = %routine.name,
                            remaining_secs = remaining,
                            "routine in cooldown"
                        );
                        results.push(RoutineDispatchResult::Cooldown {
                            routine_name: routine.name.clone(),
                            remaining_secs: remaining,
                        });
                        continue;
                    }
                }
            }

            info!(routine = %routine.name, source = %event.source, topic = %event.topic, "routine fired");
            self.cooldowns.insert(routine.name.clone(), now);
            results.push(RoutineDispatchResult::Fired {
                routine_name: routine.name.clone(),
                action: routine.action.clone(),
            });
        }

        if results.is_empty() {
            results.push(RoutineDispatchResult::NoMatch);
        }

        results
    }

    pub fn reset_cooldowns(&mut self) {
        self.cooldowns.clear();
    }
}

pub fn load_routines_from_file(path: &std::path::Path) -> Vec<Routine> {
    match std::fs::read_to_string(path) {
        Ok(content) => match toml::from_str::<RoutinesManifest>(&content) {
            Ok(manifest) => manifest.routines,
            Err(e) => {
                warn!("Failed to parse routines file {}: {e}", path.display());
                Vec::new()
            }
        },
        Err(e) => {
            debug!("Routines file not found at {}: {e}", path.display());
            Vec::new()
        }
    }
}

pub fn load_routines(workspace_dir: &std::path::Path) -> Vec<Routine> {
    let path = workspace_dir.join("routines.toml");
    load_routines_from_file(&path)
}
