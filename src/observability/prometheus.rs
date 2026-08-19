// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Observer, ObserverEvent, ObserverMetric};
use prometheus::{
    Encoder, GaugeVec, Histogram, HistogramOpts, HistogramVec, IntCounterVec, Registry, TextEncoder,
};

pub struct PrometheusObserver {
    registry: Registry,

    agent_starts: IntCounterVec,
    llm_requests: IntCounterVec,
    tokens_input_total: IntCounterVec,
    tokens_output_total: IntCounterVec,
    tool_calls: IntCounterVec,
    channel_messages: IntCounterVec,
    heartbeat_ticks: prometheus::IntCounter,
    errors: IntCounterVec,
    cache_hits: IntCounterVec,
    cache_misses: IntCounterVec,
    cache_tokens_saved: IntCounterVec,

    agent_duration: HistogramVec,
    tool_duration: HistogramVec,
    request_latency: Histogram,

    tokens_used: prometheus::IntGauge,
    active_sessions: GaugeVec,
    queue_depth: GaugeVec,

    hand_runs: IntCounterVec,
    hand_duration: HistogramVec,
    hand_findings: IntCounterVec,

    deployments_total: IntCounterVec,
    deployment_lead_time: Histogram,
    deployment_failure_rate: prometheus::Gauge,
    recovery_time: Histogram,
    mttr: prometheus::Gauge,

    session_events_total: IntCounterVec,
    keybindings_reload_total: prometheus::IntCounter,
    deploy_success_count: std::sync::atomic::AtomicU64,
    deploy_failure_count: std::sync::atomic::AtomicU64,

    first_token_latency_ms: Histogram,
    response_cache_hits_total: IntCounterVec,
    response_cache_misses_total: IntCounterVec,

    flow_runs_total: IntCounterVec,
    flow_duration_seconds: HistogramVec,
}

fn register_metric(registry: &Registry, collector: Box<dyn prometheus::core::Collector>) {
    if let Err(err) = registry.register(collector) {
        tracing::error!(
            target: "observability.prometheus",
            error = %err,
            "failed to register prometheus metric"
        );
    }
}

impl PrometheusObserver {
    pub fn new() -> Self {
        let registry = Registry::new();

        let agent_starts = IntCounterVec::new(
            prometheus::Opts::new("sen_agent_starts_total", "Total agent invocations"),
            &["provider", "model"],
        )
        .expect("valid metric");

        let llm_requests = IntCounterVec::new(
            prometheus::Opts::new("sen_llm_requests_total", "Total LLM provider requests"),
            &["provider", "model", "success"],
        )
        .expect("valid metric");

        let tokens_input_total = IntCounterVec::new(
            prometheus::Opts::new("sen_tokens_input_total", "Total input tokens consumed"),
            &["provider", "model"],
        )
        .expect("valid metric");

        let tokens_output_total = IntCounterVec::new(
            prometheus::Opts::new("sen_tokens_output_total", "Total output tokens consumed"),
            &["provider", "model"],
        )
        .expect("valid metric");

        let tool_calls = IntCounterVec::new(
            prometheus::Opts::new("sen_tool_calls_total", "Total tool calls"),
            &["tool", "success"],
        )
        .expect("valid metric");

        let channel_messages = IntCounterVec::new(
            prometheus::Opts::new("sen_channel_messages_total", "Total channel messages"),
            &["channel", "direction"],
        )
        .expect("valid metric");

        let heartbeat_ticks =
            prometheus::IntCounter::new("sen_heartbeat_ticks_total", "Total heartbeat ticks")
                .expect("valid metric");

        let errors = IntCounterVec::new(
            prometheus::Opts::new("sen_errors_total", "Total errors by component"),
            &["component"],
        )
        .expect("valid metric");

        let cache_hits = IntCounterVec::new(
            prometheus::Opts::new("sen_cache_hits_total", "Total response cache hits"),
            &["cache_type"],
        )
        .expect("valid metric");

        let cache_misses = IntCounterVec::new(
            prometheus::Opts::new("sen_cache_misses_total", "Total response cache misses"),
            &["cache_type"],
        )
        .expect("valid metric");

        let cache_tokens_saved = IntCounterVec::new(
            prometheus::Opts::new(
                "sen_cache_tokens_saved_total",
                "Total tokens saved by response cache",
            ),
            &["cache_type"],
        )
        .expect("valid metric");

        let agent_duration = HistogramVec::new(
            HistogramOpts::new(
                "sen_agent_duration_seconds",
                "Agent invocation duration in seconds",
            )
            .buckets(vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0]),
            &["provider", "model"],
        )
        .expect("valid metric");

        let tool_duration = HistogramVec::new(
            HistogramOpts::new(
                "sen_tool_duration_seconds",
                "Tool execution duration in seconds",
            )
            .buckets(vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0]),
            &["tool"],
        )
        .expect("valid metric");

        let request_latency = Histogram::with_opts(
            HistogramOpts::new("sen_request_latency_seconds", "Request latency in seconds")
                .buckets(vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
        )
        .expect("valid metric");

        let tokens_used =
            prometheus::IntGauge::new("sen_tokens_used_last", "Tokens used in the last request")
                .expect("valid metric");

        let active_sessions = GaugeVec::new(
            prometheus::Opts::new("sen_active_sessions", "Number of active sessions"),
            &[],
        )
        .expect("valid metric");

        let queue_depth = GaugeVec::new(
            prometheus::Opts::new("sen_queue_depth", "Message queue depth"),
            &[],
        )
        .expect("valid metric");

        let hand_runs = IntCounterVec::new(
            prometheus::Opts::new("sen_hand_runs_total", "Total hand runs by outcome"),
            &["hand", "success"],
        )
        .expect("valid metric");

        let hand_duration = HistogramVec::new(
            HistogramOpts::new("sen_hand_duration_seconds", "Hand run duration in seconds")
                .buckets(vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0]),
            &["hand"],
        )
        .expect("valid metric");

        let hand_findings = IntCounterVec::new(
            prometheus::Opts::new(
                "sen_hand_findings_total",
                "Total findings produced by hand runs",
            ),
            &["hand"],
        )
        .expect("valid metric");

        let deployments_total = IntCounterVec::new(
            prometheus::Opts::new("sen_deployments_total", "Total deployments by status"),
            &["status"],
        )
        .expect("valid metric");

        let deployment_lead_time = Histogram::with_opts(
            HistogramOpts::new(
                "sen_deployment_lead_time_seconds",
                "Deployment lead time from commit to deploy in seconds",
            )
            .buckets(vec![
                60.0, 300.0, 600.0, 1800.0, 3600.0, 7200.0, 14400.0, 43200.0, 86400.0,
            ]),
        )
        .expect("valid metric");

        let deployment_failure_rate = prometheus::Gauge::new(
            "sen_deployment_failure_rate",
            "Ratio of failed deployments to total deployments",
        )
        .expect("valid metric");

        let recovery_time = Histogram::with_opts(
            HistogramOpts::new(
                "sen_recovery_time_seconds",
                "Time to recover from a failed deployment in seconds",
            )
            .buckets(vec![
                60.0, 300.0, 600.0, 1800.0, 3600.0, 7200.0, 14400.0, 43200.0, 86400.0,
            ]),
        )
        .expect("valid metric");

        let mttr = prometheus::Gauge::new("sen_mttr_seconds", "Mean time to recovery in seconds")
            .expect("valid metric");

        let session_events_total = IntCounterVec::new(
            prometheus::Opts::new(
                "sen_session_events_total",
                "Total SessionEvent broadcasts emitted to subscribers",
            ),
            &["kind"],
        )
        .expect("valid metric");
        let keybindings_reload_total = prometheus::IntCounter::new(
            "sen_keybindings_reload_total",
            "Total `~/.sen/keybindings.toml` reloads accepted",
        )
        .expect("valid metric");

        let first_token_latency_ms = Histogram::with_opts(
            HistogramOpts::new(
                "sen_first_token_latency_ms",
                "Latency (ms) from TurnStarted to first streamed token",
            )
            .buckets(vec![
                50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 30000.0,
            ]),
        )
        .expect("valid metric");

        let response_cache_hits_total = IntCounterVec::new(
            prometheus::Opts::new(
                "sen_response_cache_hits_total",
                "Provider response-cache hits labelled by provider/model",
            ),
            &["provider", "model"],
        )
        .expect("valid metric");

        let response_cache_misses_total = IntCounterVec::new(
            prometheus::Opts::new(
                "sen_response_cache_misses_total",
                "Provider response-cache misses labelled by provider/model",
            ),
            &["provider", "model"],
        )
        .expect("valid metric");

        let flow_runs_total = IntCounterVec::new(
            prometheus::Opts::new(
                "sen_flow_runs_total",
                "Total flow runs labelled by flow name and outcome",
            ),
            &["flow", "outcome"],
        )
        .expect("valid metric");

        let flow_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "sen_flow_duration_seconds",
                "End-to-end duration of a flow run, seconds",
            )
            .buckets(vec![
                0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0,
            ]),
            &["flow"],
        )
        .expect("valid metric");

        register_metric(&registry, Box::new(agent_starts.clone()));
        register_metric(&registry, Box::new(llm_requests.clone()));
        register_metric(&registry, Box::new(tokens_input_total.clone()));
        register_metric(&registry, Box::new(tokens_output_total.clone()));
        register_metric(&registry, Box::new(tool_calls.clone()));
        register_metric(&registry, Box::new(channel_messages.clone()));
        register_metric(&registry, Box::new(heartbeat_ticks.clone()));
        register_metric(&registry, Box::new(errors.clone()));
        register_metric(&registry, Box::new(cache_hits.clone()));
        register_metric(&registry, Box::new(cache_misses.clone()));
        register_metric(&registry, Box::new(cache_tokens_saved.clone()));
        register_metric(&registry, Box::new(agent_duration.clone()));
        register_metric(&registry, Box::new(tool_duration.clone()));
        register_metric(&registry, Box::new(request_latency.clone()));
        register_metric(&registry, Box::new(tokens_used.clone()));
        register_metric(&registry, Box::new(active_sessions.clone()));
        register_metric(&registry, Box::new(queue_depth.clone()));
        register_metric(&registry, Box::new(hand_runs.clone()));
        register_metric(&registry, Box::new(hand_duration.clone()));
        register_metric(&registry, Box::new(hand_findings.clone()));
        register_metric(&registry, Box::new(deployments_total.clone()));
        register_metric(&registry, Box::new(deployment_lead_time.clone()));
        register_metric(&registry, Box::new(deployment_failure_rate.clone()));
        register_metric(&registry, Box::new(recovery_time.clone()));
        register_metric(&registry, Box::new(mttr.clone()));
        register_metric(&registry, Box::new(session_events_total.clone()));
        register_metric(&registry, Box::new(keybindings_reload_total.clone()));
        register_metric(&registry, Box::new(first_token_latency_ms.clone()));
        register_metric(&registry, Box::new(response_cache_hits_total.clone()));
        register_metric(&registry, Box::new(response_cache_misses_total.clone()));
        register_metric(&registry, Box::new(flow_runs_total.clone()));
        register_metric(&registry, Box::new(flow_duration_seconds.clone()));

        Self {
            registry,
            agent_starts,
            llm_requests,
            tokens_input_total,
            tokens_output_total,
            tool_calls,
            channel_messages,
            heartbeat_ticks,
            errors,
            cache_hits,
            cache_misses,
            cache_tokens_saved,
            agent_duration,
            tool_duration,
            request_latency,
            tokens_used,
            active_sessions,
            queue_depth,
            hand_runs,
            hand_duration,
            hand_findings,
            deployments_total,
            deployment_lead_time,
            deployment_failure_rate,
            recovery_time,
            mttr,
            deploy_success_count: std::sync::atomic::AtomicU64::new(0),
            deploy_failure_count: std::sync::atomic::AtomicU64::new(0),
            session_events_total,
            keybindings_reload_total,
            first_token_latency_ms,
            response_cache_hits_total,
            response_cache_misses_total,
            flow_runs_total,
            flow_duration_seconds,
        }
    }

    pub fn record_flow_run(&self, flow: &str, outcome: &str, duration_seconds: f64) {
        self.flow_runs_total
            .with_label_values(&[flow, outcome])
            .inc();
        self.flow_duration_seconds
            .with_label_values(&[flow])
            .observe(duration_seconds);
    }

    pub fn observe_first_token_latency_ms(&self, elapsed_ms: u64) {
        self.first_token_latency_ms.observe(elapsed_ms as f64);
    }

    pub fn inc_response_cache(&self, provider: &str, model: &str, hit: bool) {
        if hit {
            self.response_cache_hits_total
                .with_label_values(&[provider, model])
                .inc();
        } else {
            self.response_cache_misses_total
                .with_label_values(&[provider, model])
                .inc();
        }
    }

    pub fn inc_session_event(&self, kind: &str) {
        self.session_events_total.with_label_values(&[kind]).inc();
    }

    pub fn inc_keybindings_reload(&self) {
        self.keybindings_reload_total.inc();
    }

    pub fn encode(&self) -> String {
        let encoder = TextEncoder::new();
        let families = self.registry.gather();
        let mut buf = Vec::new();
        encoder.encode(&families, &mut buf).unwrap_or_default();
        let mut text = String::from_utf8(buf).unwrap_or_default();
        text.push_str(
            &super::subsystem_metrics::global()
                .snapshot()
                .render_prometheus_text(),
        );
        text.push_str(
            &super::code_intel_metrics::global()
                .snapshot()
                .render_prometheus_text(),
        );
        text.push_str(
            &super::session_write_mode_metrics::global()
                .snapshot()
                .render_prometheus_text(),
        );

        text.push_str(
            &super::coordination_metrics::global()
                .snapshot()
                .render_prometheus_text(),
        );

        text.push_str(
            &super::scheduler_metrics::global()
                .snapshot()
                .render_prometheus_text(),
        );

        text.push_str(
            &super::tui_metrics::global()
                .snapshot()
                .render_prometheus_text(),
        );

        text.push_str(
            &super::token_saver_metrics::global()
                .snapshot()
                .render_prometheus_text(),
        );

        if let Some(svc) = crate::services::try_get_services() {
            text.push_str(&svc.agent_metrics.render_prometheus());
        }

        text
    }
}

impl Observer for PrometheusObserver {
    fn record_event(&self, event: &ObserverEvent) {
        match event {
            ObserverEvent::AgentStart { provider, model } => {
                self.agent_starts
                    .with_label_values(&[provider, model])
                    .inc();
            }
            ObserverEvent::AgentEnd {
                provider,
                model,
                duration,
                tokens_used,
                cost_usd: _,
            } => {

                self.agent_duration
                    .with_label_values(&[provider, model])
                    .observe(duration.as_secs_f64());
                if let Some(t) = tokens_used {
                    self.tokens_used.set(i64::try_from(*t).unwrap_or(i64::MAX));
                }
            }
            ObserverEvent::LlmResponse {
                provider,
                model,
                success,
                input_tokens,
                output_tokens,
                ..
            } => {
                let success_str = if *success { "true" } else { "false" };
                self.llm_requests
                    .with_label_values(&[provider.as_str(), model.as_str(), success_str])
                    .inc();
                if let Some(input) = input_tokens {
                    self.tokens_input_total
                        .with_label_values(&[provider.as_str(), model.as_str()])
                        .inc_by(*input);
                }
                if let Some(output) = output_tokens {
                    self.tokens_output_total
                        .with_label_values(&[provider.as_str(), model.as_str()])
                        .inc_by(*output);
                }
            }
            ObserverEvent::ToolCallStart { .. }
            | ObserverEvent::TurnComplete
            | ObserverEvent::LlmRequest { .. }
            | ObserverEvent::DeploymentStarted { .. }
            | ObserverEvent::RecoveryCompleted { .. } => {}
            ObserverEvent::ToolCall {
                tool,
                duration,
                success,
            } => {
                let success_str = if *success { "true" } else { "false" };
                self.tool_calls
                    .with_label_values(&[tool.as_str(), success_str])
                    .inc();
                self.tool_duration
                    .with_label_values(&[tool.as_str()])
                    .observe(duration.as_secs_f64());
            }
            ObserverEvent::ChannelMessage { channel, direction } => {
                self.channel_messages
                    .with_label_values(&[channel, direction])
                    .inc();
            }
            ObserverEvent::HeartbeatTick => {
                self.heartbeat_ticks.inc();
            }
            ObserverEvent::CacheHit {
                cache_type,
                tokens_saved,
            } => {
                self.cache_hits.with_label_values(&[cache_type]).inc();
                self.cache_tokens_saved
                    .with_label_values(&[cache_type])
                    .inc_by(*tokens_saved);
            }
            ObserverEvent::CacheMiss { cache_type } => {
                self.cache_misses.with_label_values(&[cache_type]).inc();
            }
            ObserverEvent::Error {
                component,
                message: _,
            } => {
                self.errors.with_label_values(&[component]).inc();
            }
            ObserverEvent::HandStarted { hand_name } => {
                self.hand_runs
                    .with_label_values(&[hand_name.as_str(), "true"])
                    .inc_by(0);
            }
            ObserverEvent::HandCompleted {
                hand_name,
                duration_ms,
                findings_count,
            } => {
                self.hand_runs
                    .with_label_values(&[hand_name.as_str(), "true"])
                    .inc();
                self.hand_duration
                    .with_label_values(&[hand_name.as_str()])
                    .observe(*duration_ms as f64 / 1000.0);
                self.hand_findings
                    .with_label_values(&[hand_name.as_str()])
                    .inc_by(*findings_count as u64);
            }
            ObserverEvent::HandFailed {
                hand_name,
                duration_ms,
                ..
            } => {
                self.hand_runs
                    .with_label_values(&[hand_name.as_str(), "false"])
                    .inc();
                self.hand_duration
                    .with_label_values(&[hand_name.as_str()])
                    .observe(*duration_ms as f64 / 1000.0);
            }
            ObserverEvent::DeploymentCompleted { .. } => {
                self.deployments_total.with_label_values(&["success"]).inc();
                let s = self
                    .deploy_success_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    + 1;
                let f = self
                    .deploy_failure_count
                    .load(std::sync::atomic::Ordering::Relaxed);
                let total = s + f;
                if total > 0 {
                    self.deployment_failure_rate.set(f as f64 / total as f64);
                }
            }
            ObserverEvent::DeploymentFailed { .. } => {
                self.deployments_total.with_label_values(&["failure"]).inc();
                let f = self
                    .deploy_failure_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    + 1;
                let s = self
                    .deploy_success_count
                    .load(std::sync::atomic::Ordering::Relaxed);
                let total = s + f;
                if total > 0 {
                    self.deployment_failure_rate.set(f as f64 / total as f64);
                }
            }
        }
    }

    fn record_metric(&self, metric: &ObserverMetric) {
        match metric {
            ObserverMetric::RequestLatency(d) => {
                self.request_latency.observe(d.as_secs_f64());
            }
            ObserverMetric::TokensUsed(t) => {
                self.tokens_used.set(i64::try_from(*t).unwrap_or(i64::MAX));
            }
            ObserverMetric::ActiveSessions(s) => {
                self.active_sessions
                    .with_label_values(&[] as &[&str])
                    .set(*s as f64);
            }
            ObserverMetric::QueueDepth(d) => {
                self.queue_depth
                    .with_label_values(&[] as &[&str])
                    .set(*d as f64);
            }
            ObserverMetric::HandRunDuration {
                hand_name,
                duration,
            } => {
                self.hand_duration
                    .with_label_values(&[hand_name.as_str()])
                    .observe(duration.as_secs_f64());
            }
            ObserverMetric::HandFindingsCount { hand_name, count } => {
                self.hand_findings
                    .with_label_values(&[hand_name.as_str()])
                    .inc_by(*count);
            }
            ObserverMetric::HandSuccessRate { hand_name, success } => {
                let success_str = if *success { "true" } else { "false" };
                self.hand_runs
                    .with_label_values(&[hand_name.as_str(), success_str])
                    .inc();
            }
            ObserverMetric::DeploymentLeadTime(d) => {
                self.deployment_lead_time.observe(d.as_secs_f64());
            }
            ObserverMetric::RecoveryTime(d) => {
                self.recovery_time.observe(d.as_secs_f64());
                self.mttr.set(d.as_secs_f64());
            }
            ObserverMetric::FirstTokenLatency {
                agent_id: _,
                elapsed,
            } => {
                let ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
                self.observe_first_token_latency_ms(ms);
            }
            ObserverMetric::ResponseCacheOutcome {
                provider,
                model,
                hit,
            } => {
                self.inc_response_cache(provider, model, *hit);
            }
            ObserverMetric::FlowRun {
                flow,
                outcome,
                duration,
            } => {
                self.record_flow_run(flow, outcome, duration.as_secs_f64());
            }
        }
    }

    fn name(&self) -> &str {
        "prometheus"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

static GLOBAL_PROMETHEUS: std::sync::OnceLock<std::sync::Arc<PrometheusObserver>> =
    std::sync::OnceLock::new();

pub fn global() -> std::sync::Arc<PrometheusObserver> {
    GLOBAL_PROMETHEUS
        .get_or_init(|| std::sync::Arc::new(PrometheusObserver::new()))
        .clone()
}

pub struct SharedPrometheusObserver(pub std::sync::Arc<PrometheusObserver>);

impl Observer for SharedPrometheusObserver {
    fn record_event(&self, event: &ObserverEvent) {
        self.0.record_event(event);
    }

    fn record_metric(&self, metric: &ObserverMetric) {
        self.0.record_metric(metric);
    }

    fn flush(&self) {
        self.0.flush();
    }

    fn name(&self) -> &str {
        "prometheus"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self.0.as_any()
    }
}
