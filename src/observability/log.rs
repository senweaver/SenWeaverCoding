// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Observer, ObserverEvent, ObserverMetric};
use std::any::Any;
use tracing::info;

pub struct LogObserver;

impl LogObserver {
    pub fn new() -> Self {
        Self
    }
}

impl Observer for LogObserver {
    fn record_event(&self, event: &ObserverEvent) {
        match event {
            ObserverEvent::AgentStart { provider, model } => {
                info!(provider = %provider, model = %model, "agent.start");
            }
            ObserverEvent::AgentEnd {
                provider,
                model,
                duration,
                tokens_used,
                cost_usd,
            } => {
                let ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
                info!(provider = %provider, model = %model, duration_ms = ms, tokens = ?tokens_used, cost_usd = ?cost_usd, "agent.end");
            }
            ObserverEvent::ToolCallStart { tool, .. } => {
                info!(tool = %tool, "tool.start");
            }
            ObserverEvent::ToolCall {
                tool,
                duration,
                success,
            } => {
                let ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
                info!(tool = %tool, duration_ms = ms, success = success, "tool.call");
            }
            ObserverEvent::TurnComplete => {
                info!("turn.complete");
            }
            ObserverEvent::ChannelMessage { channel, direction } => {
                info!(channel = %channel, direction = %direction, "channel.message");
            }
            ObserverEvent::HeartbeatTick => {
                info!("heartbeat.tick");
            }
            ObserverEvent::CacheHit {
                cache_type,
                tokens_saved,
            } => {
                info!(cache_type = %cache_type, tokens_saved = tokens_saved, "cache.hit");
            }
            ObserverEvent::CacheMiss { cache_type } => {
                info!(cache_type = %cache_type, "cache.miss");
            }
            ObserverEvent::Error { component, message } => {
                info!(component = %component, error = %message, "error");
            }
            ObserverEvent::LlmRequest {
                provider,
                model,
                messages_count,
            } => {
                info!(
                    provider = %provider,
                    model = %model,
                    messages_count = messages_count,
                    "llm.request"
                );
            }
            ObserverEvent::LlmResponse {
                provider,
                model,
                duration,
                success,
                error_message,
                input_tokens,
                output_tokens,
            } => {
                let ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
                info!(
                    provider = %provider,
                    model = %model,
                    duration_ms = ms,
                    success = success,
                    error = ?error_message,
                    input_tokens = ?input_tokens,
                    output_tokens = ?output_tokens,
                    "llm.response"
                );
            }
            ObserverEvent::HandStarted { hand_name } => {
                info!(hand = %hand_name, "hand.started");
            }
            ObserverEvent::HandCompleted {
                hand_name,
                duration_ms,
                findings_count,
            } => {
                info!(hand = %hand_name, duration_ms = duration_ms, findings = findings_count, "hand.completed");
            }
            ObserverEvent::HandFailed {
                hand_name,
                error,
                duration_ms,
            } => {
                info!(hand = %hand_name, error = %error, duration_ms = duration_ms, "hand.failed");
            }
            ObserverEvent::DeploymentStarted { deploy_id } => {
                info!(deploy_id = %deploy_id, "deployment.started");
            }
            ObserverEvent::DeploymentCompleted {
                deploy_id,
                commit_sha,
            } => {
                info!(deploy_id = %deploy_id, commit_sha = %commit_sha, "deployment.completed");
            }
            ObserverEvent::DeploymentFailed { deploy_id, reason } => {
                info!(deploy_id = %deploy_id, reason = %reason, "deployment.failed");
            }
            ObserverEvent::RecoveryCompleted { deploy_id } => {
                info!(deploy_id = %deploy_id, "recovery.completed");
            }
        }
    }

    fn record_metric(&self, metric: &ObserverMetric) {
        match metric {
            ObserverMetric::RequestLatency(d) => {
                let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
                info!(latency_ms = ms, "metric.request_latency");
            }
            ObserverMetric::TokensUsed(t) => {
                info!(tokens = t, "metric.tokens_used");
            }
            ObserverMetric::ActiveSessions(s) => {
                info!(sessions = s, "metric.active_sessions");
            }
            ObserverMetric::QueueDepth(d) => {
                info!(depth = d, "metric.queue_depth");
            }
            ObserverMetric::HandRunDuration {
                hand_name,
                duration,
            } => {
                let ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
                info!(hand = %hand_name, duration_ms = ms, "metric.hand_run_duration");
            }
            ObserverMetric::HandFindingsCount { hand_name, count } => {
                info!(hand = %hand_name, count = count, "metric.hand_findings_count");
            }
            ObserverMetric::HandSuccessRate { hand_name, success } => {
                info!(hand = %hand_name, success = success, "metric.hand_success_rate");
            }
            ObserverMetric::DeploymentLeadTime(d) => {
                let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
                info!(lead_time_ms = ms, "metric.deployment_lead_time");
            }
            ObserverMetric::RecoveryTime(d) => {
                let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
                info!(recovery_time_ms = ms, "metric.recovery_time");
            }
            ObserverMetric::FirstTokenLatency { agent_id, elapsed } => {
                let ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
                info!(agent_id = %agent_id, elapsed_ms = ms, "metric.first_token_latency");
            }
            ObserverMetric::ResponseCacheOutcome {
                provider,
                model,
                hit,
            } => {
                info!(
                    provider = %provider,
                    model = %model,
                    hit = hit,
                    "metric.response_cache_outcome"
                );
            }
            ObserverMetric::FlowRun {
                flow,
                outcome,
                duration,
            } => {
                info!(
                    flow = %flow,
                    outcome = %outcome,
                    duration_ms = duration.as_millis() as u64,
                    "metric.flow_run"
                );
            }
        }
    }

    fn name(&self) -> &str {
        "log"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
