// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::agent::eval::{ComplexityTier, estimate_complexity};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IntentAnalysisConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_min_confidence")]
    pub min_confidence: f64,

    #[serde(default = "default_true")]
    pub enrich_preamble: bool,

    #[serde(default)]
    pub enforce_plan_threshold: bool,
}

fn default_min_confidence() -> f64 {
    0.6
}

fn default_true() -> bool {
    true
}

impl Default for IntentAnalysisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_confidence: default_min_confidence(),
            enrich_preamble: default_true(),
            enforce_plan_threshold: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskIntent {
    Coding,
    Debug,
    Design,
    Plan,
    Qa,
    General,
}

impl TaskIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Coding => "coding",
            Self::Debug => "debug",
            Self::Design => "design",
            Self::Plan => "plan",
            Self::Qa => "qa",
            Self::General => "general",
        }
    }

    fn intent_note(self) -> Option<&'static str> {
        match self {
            Self::Coding => Some(
                "[Intent] The user is requesting a concrete code change or new functionality. \
                 Center this turn on that goal and act within the rules of the current coding mode.",
            ),
            Self::Debug => Some(
                "[Intent] The user is investigating or fixing a bug/failure. Prioritise \
                 understanding the root cause from evidence before acting, within the rules of the \
                 current coding mode.",
            ),
            Self::Design => Some(
                "[Intent] The user wants design/architecture reasoning. Weigh the approach and key \
                 trade-offs, within the rules of the current coding mode.",
            ),
            Self::Plan => Some(
                "[Intent] The user is describing a multi-step task. Keep the work organised into \
                 clear ordered steps, within the rules of the current coding mode.",
            ),
            Self::Qa => Some(
                "[Intent] The user is asking a question and expects a clear, well-grounded \
                 explanation, within the rules of the current coding mode.",
            ),
            Self::General => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IntentAnalysis {
    pub intent: TaskIntent,
    pub confidence: f64,
    pub complexity: ComplexityTier,
}

impl IntentAnalysis {
    pub fn is_confident(&self, min_confidence: f64) -> bool {
        self.confidence >= min_confidence
    }

    pub fn intent_note(&self) -> Option<&'static str> {
        self.intent.intent_note()
    }

    pub fn coding_mode(&self) -> crate::agent::coding_mode::CodingMode {
        use crate::agent::coding_mode::CodingMode;
        if self.confidence < 0.55 {
            return CodingMode::Agent;
        }
        match self.intent {
            TaskIntent::Debug => CodingMode::Debug,
            TaskIntent::Design => CodingMode::Architect,
            TaskIntent::Plan => CodingMode::Plan,
            TaskIntent::Qa => CodingMode::Ask,
            TaskIntent::Coding | TaskIntent::General => CodingMode::Agent,
        }
    }
}

pub fn auto_select_coding_mode(message: &str) -> crate::agent::coding_mode::CodingMode {
    analyze_intent(message).coding_mode()
}

const DEBUG_KEYWORDS: &[&str] = &[
    "bug", "error", "panic", "crash", "fail", "failing", "broken", "stack trace", "traceback",
    "exception", "not working", "doesn't work", "does not work", "regression", "调试", "报错",
    "崩溃", "异常", "修复",
];

const CODING_KEYWORDS: &[&str] = &[
    "implement", "add", "create", "write", "refactor", "rename", "build", "function", "class",
    "struct", "module", "method", "endpoint", "feature", "code", "实现", "添加", "新增", "重构",
    "编写", "修改",
];

const DESIGN_KEYWORDS: &[&str] = &[
    "design", "architecture", "approach", "trade-off", "tradeoff", "pattern", "structure",
    "compare", "options", "strategy", "架构", "设计", "方案",
];

const PLAN_KEYWORDS: &[&str] = &[
    "plan", "step by step", "steps", "roadmap", "milestone", "then", "after that", "phase",
    "计划", "步骤", "路线",
];

const QA_PREFIXES: &[&str] = &[
    "what", "why", "how", "when", "where", "who", "which", "is ", "are ", "does", "do ", "can ",
    "could", "should", "explain", "什么", "为什么", "怎么", "如何", "是否",
];

fn count_hits(lower: &str, keywords: &[&str]) -> usize {
    keywords.iter().filter(|kw| lower.contains(**kw)).count()
}

pub fn analyze_intent(message: &str) -> IntentAnalysis {
    let trimmed = message.trim();
    let lower = trimmed.to_lowercase();
    let complexity = estimate_complexity(trimmed);

    let debug_hits = count_hits(&lower, DEBUG_KEYWORDS);
    let coding_hits = count_hits(&lower, CODING_KEYWORDS);
    let design_hits = count_hits(&lower, DESIGN_KEYWORDS);
    let plan_hits = count_hits(&lower, PLAN_KEYWORDS);

    let is_question = trimmed.ends_with('?')
        || trimmed.ends_with('？')
        || QA_PREFIXES.iter().any(|p| lower.starts_with(p));

    let scored: [(TaskIntent, usize); 4] = [
        (TaskIntent::Debug, debug_hits),
        (TaskIntent::Coding, coding_hits),
        (TaskIntent::Design, design_hits),
        (TaskIntent::Plan, plan_hits),
    ];

    let best = scored
        .iter()
        .copied()
        .max_by_key(|(_, hits)| *hits)
        .filter(|(_, hits)| *hits > 0);

    let plan_worthy =
        plan_hits > 0 || matches!(complexity, ComplexityTier::Complex) && coding_hits > 0;

    let (intent, base) = match best {
        Some((TaskIntent::Plan, hits)) => (TaskIntent::Plan, hits),
        Some((_intent, hits)) if plan_worthy && plan_hits >= hits => (TaskIntent::Plan, plan_hits),
        Some((intent, hits)) => (intent, hits),
        None if is_question => (TaskIntent::Qa, 1),
        None => (TaskIntent::General, 0),
    };

    let mut confidence = match intent {
        TaskIntent::Qa => 0.65,
        TaskIntent::General => 0.3,
        _ => 0.5 + 0.15 * (base.min(3) as f64),
    };
    if matches!(complexity, ComplexityTier::Complex) {
        confidence += 0.05;
    }
    confidence = confidence.clamp(0.0, 1.0);

    IntentAnalysis {
        intent,
        confidence,
        complexity,
    }
}
