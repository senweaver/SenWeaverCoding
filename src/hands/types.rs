// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::cron::Schedule;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hand {

    pub name: String,

    pub description: String,

    pub schedule: Schedule,

    pub prompt: String,

    #[serde(default)]
    pub knowledge: Vec<String>,

    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,

    #[serde(default)]
    pub model: Option<String>,

    #[serde(default = "default_true")]
    pub active: bool,

    #[serde(default = "default_max_runs")]
    pub max_history: usize,
}

fn default_true() -> bool {
    true
}

fn default_max_runs() -> usize {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum HandRunStatus {
    Running,
    Completed,
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandRun {

    pub hand_name: String,

    pub run_id: String,

    pub started_at: DateTime<Utc>,

    pub finished_at: Option<DateTime<Utc>>,

    pub status: HandRunStatus,

    #[serde(default)]
    pub findings: Vec<String>,

    #[serde(default)]
    pub knowledge_added: Vec<String>,

    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandContext {

    pub hand_name: String,

    #[serde(default)]
    pub history: Vec<HandRun>,

    #[serde(default)]
    pub learned_facts: Vec<String>,

    pub last_run: Option<DateTime<Utc>>,

    #[serde(default)]
    pub total_runs: u64,
}

impl HandContext {

    pub fn new(hand_name: &str) -> Self {
        Self {
            hand_name: hand_name.to_string(),
            history: Vec::new(),
            learned_facts: Vec::new(),
            last_run: None,
            total_runs: 0,
        }
    }

    pub fn record_run(&mut self, run: HandRun, max_history: usize) {
        if run.status == (HandRunStatus::Completed) {
            self.total_runs += 1;
            self.last_run = run.finished_at;
        }

        for fact in &run.knowledge_added {
            if !self.learned_facts.contains(fact) {
                self.learned_facts.push(fact.clone());
            }
        }

        self.history.insert(0, run);

        self.history.truncate(max_history);
    }
}
