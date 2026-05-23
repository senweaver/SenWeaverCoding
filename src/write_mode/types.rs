// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanContext {
    pub goal: String,
    pub workspace_root: PathBuf,
    #[serde(default)]
    pub hint: Option<String>,

    #[serde(default)]
    pub allow_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WriteStep {

    ReadFile { path: PathBuf },

    GrepSymbol { query: String },

    ApplyDiff {
        path: PathBuf,

        #[serde(default)]
        instruction: Option<String>,
        #[serde(default)]
        diff: Option<String>,
    },

    RunCommand {
        command: String,
        #[serde(default)]
        cwd: Option<PathBuf>,
    },

    Verify {
        #[serde(default)]
        expect_contains: Vec<String>,
    },
}

impl WriteStep {
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::ReadFile { .. } => "read_file",
            Self::GrepSymbol { .. } => "grep_symbol",
            Self::ApplyDiff { .. } => "apply_diff",
            Self::RunCommand { .. } => "run_command",
            Self::Verify { .. } => "verify",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WritePlan {
    pub goal: String,
    pub steps: Vec<WriteStep>,
}

impl WritePlan {
    #[must_use]
    pub fn new(goal: impl Into<String>, steps: Vec<WriteStep>) -> Self {
        Self {
            goal: goal.into(),
            steps,
        }
    }

    #[must_use]
    pub fn summary(&self) -> String {
        let mut out = format!("plan[{}]:", self.steps.len());
        for (i, step) in self.steps.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(step.label());
        }
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VerifyOutcome {
    Passed,
    Failed {
        reason: String,
    },

    Absent,
}
