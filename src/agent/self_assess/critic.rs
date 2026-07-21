// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use serde::Deserialize;

use crate::providers::traits::{ChatMessage, Provider};

use super::eval::{SelfEvalConfig, judge_response};

const CODE_REVIEW_RUBRIC: &str = r#"You are an independent senior code reviewer. You did NOT write this code.
You are given ONLY the goal and the produced change. You must grade the change strictly
against the rubric below. Be skeptical: a passing claim is not proof.

Rubric (each is a hard requirement):
1. correctness: The change accomplishes the stated goal without introducing regressions.
2. completeness: No required edit is missing; no half-finished work is left behind.
3. no_placeholders: There is NO placeholder, stub, `todo!()`, `unimplemented!()`, dummy
   return value, or commented-out "to be implemented later" code.
4. integration: New or changed logic is actually wired into the project; nothing dangles.
5. safety: No obvious security flaw, data loss, or destructive operation is introduced.

Respond ONLY with valid JSON, no prose, no Markdown fences:
{"passed":true,"score":0.0,"should_retry":false,"rationale":"1-2 sentences",
 "findings":[{"severity":"error|warning","message":"specific, actionable problem"}]}

Set should_retry=true and passed=false if ANY hard requirement is violated.
Keep findings empty when the change passes."#;

#[derive(Debug, Clone)]
pub struct CriticFinding {
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct CriticVerdict {
    pub passed: bool,
    pub score: f64,
    pub should_retry: bool,
    pub rationale: String,
    pub findings: Vec<CriticFinding>,
}

#[derive(Clone)]
pub struct CriticContext {
    provider: Arc<dyn Provider>,
    model: String,
    eval_provider: Option<Arc<dyn Provider>>,
    config: SelfEvalConfig,
}

impl CriticContext {
    pub fn new(provider: Arc<dyn Provider>, model: impl Into<String>, config: SelfEvalConfig) -> Self {
        Self {
            provider,
            model: model.into(),
            eval_provider: None,
            config,
        }
    }

    pub fn with_eval_provider(mut self, eval_provider: Option<Arc<dyn Provider>>) -> Self {
        self.eval_provider = eval_provider;
        self
    }

    pub fn config(&self) -> &SelfEvalConfig {
        &self.config
    }

    pub fn is_code_edit_review_enabled(&self) -> bool {
        self.config.enabled && self.config.evaluate_code_edits
    }

    fn evaluator_target(&self) -> (&dyn Provider, String) {
        match self
            .config
            .evaluator_model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
        {
            Some(m) => {
                let provider = self
                    .eval_provider
                    .as_deref()
                    .unwrap_or_else(|| self.provider.as_ref());
                (provider, m.to_string())
            }
            None => (self.provider.as_ref(), self.model.clone()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawCriticResponse {
    #[serde(default)]
    passed: bool,
    #[serde(default)]
    score: f64,
    #[serde(default)]
    should_retry: bool,
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    findings: Vec<RawCriticFinding>,
}

#[derive(Debug, Deserialize)]
struct RawCriticFinding {
    #[serde(default)]
    severity: String,
    #[serde(default)]
    message: String,
}

pub struct IndependentCritic;

impl IndependentCritic {

    pub async fn review_code_edit(
        ctx: &CriticContext,
        goal: &str,
        artifact_path: &str,
        artifact_content: &str,
    ) -> Option<CriticVerdict> {
        if !ctx.is_code_edit_review_enabled() {
            return None;
        }

        let rubric = load_rubric(ctx.config.frozen_rubric_path.as_deref());
        let user_context = format!(
            "Goal:\n{}\n\nChanged file: {}\n\nProduced change:\n{}",
            truncate(goal, 1500),
            artifact_path,
            truncate(artifact_content, 6000),
        );

        let messages = vec![
            ChatMessage::system(rubric),
            ChatMessage::user(user_context),
        ];

        Self::vote(ctx, &messages).await
    }

    async fn vote(ctx: &CriticContext, messages: &[ChatMessage]) -> Option<CriticVerdict> {
        let votes = ctx.config.eval_votes.max(1);
        let (provider, model) = ctx.evaluator_target();

        let mut scores: Vec<f64> = Vec::new();
        let mut retry_votes = 0u32;
        let mut findings: Vec<CriticFinding> = Vec::new();
        let mut last_rationale = String::new();
        let mut parsed_any = false;

        const CRITIC_VOTE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
        let vote_futures: Vec<_> = (0..votes)
            .map(|_| {
                tokio::time::timeout(
                    CRITIC_VOTE_TIMEOUT,
                    provider.chat_with_history(messages, &model, ctx.config.judge_temperature),
                )
            })
            .collect();
        let vote_results = futures_util::future::join_all(vote_futures).await;
        for raw in vote_results {
            let raw = match raw {
                Ok(r) => r,
                Err(_) => {
                    tracing::warn!(
                        target: "agent.self_assess",
                        model = %model,
                        "critic vote timed out; skipping this vote so the turn is not blocked"
                    );
                    continue;
                }
            };
            let Ok(text) = raw else { continue };
            let parsed = crate::providers::traits::parse_first_json_object(&text)
                .and_then(|v| serde_json::from_value::<RawCriticResponse>(v).ok());
            let Some(parsed) = parsed else { continue };
            parsed_any = true;
            scores.push(parsed.score);
            let violated = parsed.should_retry || !parsed.passed;
            if violated {
                retry_votes += 1;
            }
            for f in parsed.findings {
                if f.message.trim().is_empty() {
                    continue;
                }
                let severity = if f.severity.trim().is_empty() {
                    "warning".to_string()
                } else {
                    f.severity
                };
                let already = findings.iter().any(|existing| existing.message == f.message);
                if !already {
                    findings.push(CriticFinding {
                        severity,
                        message: f.message,
                    });
                }
            }
            if !parsed.rationale.trim().is_empty() {
                last_rationale = parsed.rationale;
            }
        }

        if !parsed_any {
            return None;
        }

        let avg_score = if scores.is_empty() {
            0.0
        } else {
            scores.iter().sum::<f64>() / scores.len() as f64
        };
        let should_retry = retry_votes > votes / 2;

        Some(CriticVerdict {
            passed: !should_retry,
            score: avg_score.clamp(-1.0, 1.0),
            should_retry,
            rationale: last_rationale,
            findings,
        })
    }

    pub async fn review_turn(
        ctx: &CriticContext,
        user_query: &str,
        assistant_response: &str,
    ) -> Option<CriticVerdict> {
        if !ctx.config.enabled {
            return None;
        }
        let (provider, model) = ctx.evaluator_target();
        let verdict = judge_response(
            provider,
            &model,
            user_query,
            assistant_response,
            None,
            &ctx.config,
        )
        .await?;

        let findings: Vec<CriticFinding> = verdict
            .suggestions
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .map(|message| CriticFinding {
                severity: "warning".to_string(),
                message,
            })
            .collect();

        Some(CriticVerdict {
            passed: !verdict.should_retry,
            score: verdict.score,
            should_retry: verdict.should_retry,
            rationale: verdict.rationale,
            findings,
        })
    }
}

fn load_rubric(path: Option<&str>) -> String {
    if let Some(p) = path.filter(|p| !p.is_empty()) {
        if let Ok(contents) = std::fs::read_to_string(p) {
            let trimmed = contents.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    CODE_REVIEW_RUBRIC.to_string()
}

fn truncate(s: &str, max_chars: usize) -> &str {
    if s.len() <= max_chars {
        return s;
    }
    let end = s
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= max_chars)
        .last()
        .unwrap_or(0);
    &s[..end]
}
