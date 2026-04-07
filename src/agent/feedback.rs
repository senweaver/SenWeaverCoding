// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Multi-signal feedback collection and reward aggregation.
//!
//! Combines multiple feedback sources (PRM judge, user reactions, tool
//! outcomes, next-state evidence) into a scalar reward used for
//! policy adjustment.

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;

/// Configuration for the feedback collection system.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackConfig {
    /// Enable feedback collection.
    #[serde(default)]
    pub enabled: bool,
    /// Maximum number of feedback entries to keep in memory.
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
    /// Weight for LLM judge score in combined reward.
    #[serde(default = "default_judge_weight")]
    pub judge_weight: f64,
    /// Weight for user feedback (thumbs up/down, corrections) in combined reward.
    #[serde(default = "default_user_weight")]
    pub user_weight: f64,
    /// Weight for tool execution success rate in combined reward.
    #[serde(default = "default_tool_weight")]
    pub tool_weight: f64,
    /// Weight for next-state evidence (follow-up quality) in combined reward.
    #[serde(default = "default_next_state_weight")]
    pub next_state_weight: f64,
}

fn default_max_entries() -> usize {
    1000
}
fn default_judge_weight() -> f64 {
    0.35
}
fn default_user_weight() -> f64 {
    0.30
}
fn default_tool_weight() -> f64 {
    0.20
}
fn default_next_state_weight() -> f64 {
    0.15
}

impl Default for FeedbackConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_entries: default_max_entries(),
            judge_weight: default_judge_weight(),
            user_weight: default_user_weight(),
            tool_weight: default_tool_weight(),
            next_state_weight: default_next_state_weight(),
        }
    }
}

/// A single feedback signal from one source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FeedbackSignal {
    /// Score from the LLM-as-judge (-1.0 to 1.0).
    JudgeScore(f64),
    /// Explicit user feedback.
    UserReaction(UserReaction),
    /// Tool execution outcome.
    ToolOutcome {
        tool_name: String,
        success: bool,
        duration_ms: u64,
    },
    /// Next-state evidence: user's follow-up indicates satisfaction or frustration.
    NextStateEvidence(NextStateSignal),
    /// Heuristic dimension scores from fast eval.
    HeuristicEval {
        relevance: f64,
        completeness: f64,
        accuracy: f64,
    },
}

/// Explicit user reaction types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserReaction {
    ThumbsUp,
    ThumbsDown,
    Correction(String),
    Rephrase,
    Ignore,
}

impl UserReaction {
    pub fn to_score(&self) -> f64 {
        match self {
            Self::ThumbsUp => 1.0,
            Self::ThumbsDown => -1.0,
            Self::Correction(_) => -0.5,
            Self::Rephrase => -0.3,
            Self::Ignore => -0.1,
        }
    }
}

/// Evidence from the user's next message about the quality of the previous response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NextStateSignal {
    /// User continues with a new topic (neutral/positive).
    TopicChange,
    /// User says "thanks" or expresses satisfaction.
    Satisfaction,
    /// User repeats the same question (negative signal).
    Repetition,
    /// User expresses frustration.
    Frustration,
    /// User provides a correction.
    Correction,
}

impl NextStateSignal {
    pub fn to_score(&self) -> f64 {
        match self {
            Self::TopicChange => 0.2,
            Self::Satisfaction => 0.8,
            Self::Repetition => -0.6,
            Self::Frustration => -0.8,
            Self::Correction => -0.4,
        }
    }
}

/// Aggregated feedback for a single turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnFeedback {
    pub session_id: String,
    pub turn_index: usize,
    pub timestamp: DateTime<Utc>,
    pub signals: Vec<FeedbackSignal>,
    pub combined_reward: f64,
    pub model_used: String,
}

/// Detects next-state signals from the user's follow-up message.
pub fn detect_next_state_signal(
    _previous_assistant: &str,
    next_user_message: &str,
) -> NextStateSignal {
    let lower = next_user_message.to_lowercase();

    let satisfaction_phrases = [
        "thank",
        "thanks",
        "perfect",
        "great",
        "awesome",
        "exactly",
        "that's right",
        "good job",
        "well done",
        "excellent",
    ];
    if satisfaction_phrases.iter().any(|p| lower.contains(p)) {
        return NextStateSignal::Satisfaction;
    }

    let frustration_phrases = [
        "wrong",
        "that's not",
        "no,",
        "incorrect",
        "doesn't work",
        "still broken",
        "not what i",
        "useless",
        "terrible",
    ];
    if frustration_phrases.iter().any(|p| lower.contains(p)) {
        return NextStateSignal::Frustration;
    }

    let correction_phrases = [
        "actually,",
        "i meant",
        "let me clarify",
        "correction:",
        "no, i want",
        "that's incorrect",
    ];
    if correction_phrases.iter().any(|p| lower.contains(p)) {
        return NextStateSignal::Correction;
    }

    NextStateSignal::TopicChange
}

/// Thread-safe feedback collector that aggregates multi-signal rewards.
#[derive(Clone)]
pub struct FeedbackCollector {
    config: FeedbackConfig,
    history: Arc<RwLock<VecDeque<TurnFeedback>>>,
}

impl FeedbackCollector {
    pub fn new(config: &FeedbackConfig) -> Self {
        Self {
            config: config.clone(),
            history: Arc::new(RwLock::new(VecDeque::with_capacity(config.max_entries))),
        }
    }

    /// Record feedback signals for a turn and compute the combined reward.
    pub fn record(
        &self,
        session_id: &str,
        turn_index: usize,
        model: &str,
        signals: Vec<FeedbackSignal>,
    ) -> f64 {
        let combined = self.compute_reward(&signals);

        let entry = TurnFeedback {
            session_id: session_id.to_string(),
            turn_index,
            timestamp: Utc::now(),
            signals,
            combined_reward: combined,
            model_used: model.to_string(),
        };

        let mut hist = self.history.write();
        if hist.len() >= self.config.max_entries {
            hist.pop_front();
        }
        hist.push_back(entry);

        combined
    }

    /// Compute combined reward from multiple signals using configured weights.
    fn compute_reward(&self, signals: &[FeedbackSignal]) -> f64 {
        let mut judge_sum = 0.0f64;
        let mut judge_count = 0u32;
        let mut user_sum = 0.0f64;
        let mut user_count = 0u32;
        let mut tool_successes = 0u32;
        let mut tool_total = 0u32;
        let mut next_state_sum = 0.0f64;
        let mut next_state_count = 0u32;

        for signal in signals {
            match signal {
                FeedbackSignal::JudgeScore(s) => {
                    judge_sum += s;
                    judge_count += 1;
                }
                FeedbackSignal::UserReaction(r) => {
                    user_sum += r.to_score();
                    user_count += 1;
                }
                FeedbackSignal::ToolOutcome { success, .. } => {
                    if *success {
                        tool_successes += 1;
                    }
                    tool_total += 1;
                }
                FeedbackSignal::NextStateEvidence(ns) => {
                    next_state_sum += ns.to_score();
                    next_state_count += 1;
                }
                FeedbackSignal::HeuristicEval {
                    relevance,
                    completeness,
                    accuracy,
                } => {
                    let h_score = relevance * 0.4 + completeness * 0.3 + accuracy * 0.3;
                    judge_sum += h_score * 2.0 - 1.0;
                    judge_count += 1;
                }
            }
        }

        let mut total_weight = 0.0f64;
        let mut weighted_sum = 0.0f64;

        if judge_count > 0 {
            let avg = judge_sum / judge_count as f64;
            weighted_sum += avg * self.config.judge_weight;
            total_weight += self.config.judge_weight;
        }
        if user_count > 0 {
            let avg = user_sum / user_count as f64;
            weighted_sum += avg * self.config.user_weight;
            total_weight += self.config.user_weight;
        }
        if tool_total > 0 {
            let rate = tool_successes as f64 / tool_total as f64;
            let normalized = rate * 2.0 - 1.0;
            weighted_sum += normalized * self.config.tool_weight;
            total_weight += self.config.tool_weight;
        }
        if next_state_count > 0 {
            let avg = next_state_sum / next_state_count as f64;
            weighted_sum += avg * self.config.next_state_weight;
            total_weight += self.config.next_state_weight;
        }

        if total_weight > 0.0 {
            (weighted_sum / total_weight).clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }

    /// Get average reward over recent N turns.
    pub fn recent_average(&self, n: usize) -> f64 {
        let hist = self.history.read();
        let recent: Vec<f64> = hist
            .iter()
            .rev()
            .take(n)
            .map(|t| t.combined_reward)
            .collect();
        if recent.is_empty() {
            0.0
        } else {
            recent.iter().sum::<f64>() / recent.len() as f64
        }
    }

    /// Get reward trend (positive = improving, negative = degrading).
    pub fn reward_trend(&self, window: usize) -> f64 {
        let hist = self.history.read();
        if hist.len() < window * 2 {
            return 0.0;
        }

        let recent: Vec<f64> = hist
            .iter()
            .rev()
            .take(window)
            .map(|t| t.combined_reward)
            .collect();
        let older: Vec<f64> = hist
            .iter()
            .rev()
            .skip(window)
            .take(window)
            .map(|t| t.combined_reward)
            .collect();

        let recent_avg = recent.iter().sum::<f64>() / recent.len() as f64;
        let older_avg = older.iter().sum::<f64>() / older.len() as f64;

        recent_avg - older_avg
    }

    /// Get all feedback entries for a session.
    pub fn session_feedback(&self, session_id: &str) -> Vec<TurnFeedback> {
        let hist = self.history.read();
        hist.iter()
            .filter(|t| t.session_id == session_id)
            .cloned()
            .collect()
    }

    pub fn total_entries(&self) -> usize {
        self.history.read().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_reaction_scores() {
        assert!((UserReaction::ThumbsUp.to_score() - 1.0).abs() < f64::EPSILON);
        assert!((UserReaction::ThumbsDown.to_score() + 1.0).abs() < f64::EPSILON);
        assert!(UserReaction::Correction("fix".into()).to_score() < 0.0);
    }

    #[test]
    fn next_state_detection() {
        assert!(matches!(
            detect_next_state_signal("resp", "Thanks, that's perfect!"),
            NextStateSignal::Satisfaction
        ));
        assert!(matches!(
            detect_next_state_signal("resp", "That's wrong, I wanted something else"),
            NextStateSignal::Frustration
        ));
        assert!(matches!(
            detect_next_state_signal("resp", "Actually, I meant the other thing"),
            NextStateSignal::Correction
        ));
        assert!(matches!(
            detect_next_state_signal("resp", "Now let's talk about something else"),
            NextStateSignal::TopicChange
        ));
    }

    #[test]
    fn feedback_collector_record_and_average() {
        let config = FeedbackConfig {
            enabled: true,
            ..Default::default()
        };
        let collector = FeedbackCollector::new(&config);

        let r1 = collector.record("s1", 0, "model-a", vec![FeedbackSignal::JudgeScore(0.8)]);
        assert!(r1 > 0.0);

        let r2 = collector.record("s1", 1, "model-a", vec![FeedbackSignal::JudgeScore(-0.5)]);
        assert!(r2 < 0.0);

        let avg = collector.recent_average(10);
        assert!(avg.abs() < 1.0);
        assert_eq!(collector.total_entries(), 2);
    }

    #[test]
    fn combined_reward_multi_signal() {
        let config = FeedbackConfig {
            enabled: true,
            ..Default::default()
        };
        let collector = FeedbackCollector::new(&config);

        let reward = collector.record(
            "s1",
            0,
            "model-a",
            vec![
                FeedbackSignal::JudgeScore(0.9),
                FeedbackSignal::UserReaction(UserReaction::ThumbsUp),
                FeedbackSignal::ToolOutcome {
                    tool_name: "search".into(),
                    success: true,
                    duration_ms: 100,
                },
            ],
        );
        assert!(
            reward > 0.5,
            "positive signals should yield high reward: {reward}"
        );
    }

    #[test]
    fn max_entries_eviction() {
        let config = FeedbackConfig {
            enabled: true,
            max_entries: 3,
            ..Default::default()
        };
        let collector = FeedbackCollector::new(&config);

        for i in 0..5 {
            collector.record("s1", i, "m", vec![FeedbackSignal::JudgeScore(0.5)]);
        }
        assert_eq!(collector.total_entries(), 3);
    }

    #[test]
    fn reward_trend_insufficient_data() {
        let config = FeedbackConfig::default();
        let collector = FeedbackCollector::new(&config);
        assert!((collector.reward_trend(5)).abs() < f64::EPSILON);
    }
}
