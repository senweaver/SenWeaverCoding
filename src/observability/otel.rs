// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Observer, ObserverEvent, ObserverMetric};
use opentelemetry::metrics::{Counter, Gauge, Histogram};
use opentelemetry::trace::{Span, SpanKind, Status, Tracer};
use opentelemetry::{KeyValue, global};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use std::any::Any;
use std::time::SystemTime;

pub struct OtelObserver {
    tracer_provider: SdkTracerProvider,
    meter_provider: SdkMeterProvider,

    agent_starts: Counter<u64>,
    agent_duration: Histogram<f64>,
    llm_calls: Counter<u64>,
    llm_duration: Histogram<f64>,
    tool_calls: Counter<u64>,
    tool_duration: Histogram<f64>,
    channel_messages: Counter<u64>,
    heartbeat_ticks: Counter<u64>,
    errors: Counter<u64>,
    request_latency: Histogram<f64>,
    tokens_used: Counter<u64>,
    active_sessions: Gauge<u64>,
    queue_depth: Gauge<u64>,
    hand_runs: Counter<u64>,
    hand_duration: Histogram<f64>,
    hand_findings: Counter<u64>,
    deployment_lead_time: Histogram<f64>,
    recovery_time: Histogram<f64>,
}

impl OtelObserver {

    pub fn new(endpoint: Option<&str>, service_name: Option<&str>) -> Result<Self, String> {
        let base_endpoint = endpoint.unwrap_or("http://localhost:4318");
        let traces_endpoint = format!("{}/v1/traces", base_endpoint.trim_end_matches('/'));
        let metrics_endpoint = format!("{}/v1/metrics", base_endpoint.trim_end_matches('/'));
        let service_name = service_name.unwrap_or("sen");

        let span_exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(&traces_endpoint)
            .build()
            .map_err(|e| format!("Failed to create OTLP span exporter: {e}"))?;

        let tracer_provider = SdkTracerProvider::builder()
            .with_batch_exporter(span_exporter)
            .with_resource(
                opentelemetry_sdk::Resource::builder()
                    .with_service_name(service_name.to_string())
                    .build(),
            )
            .build();

        global::set_tracer_provider(tracer_provider.clone());

        let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_endpoint(&metrics_endpoint)
            .build()
            .map_err(|e| format!("Failed to create OTLP metric exporter: {e}"))?;

        let metric_reader =
            opentelemetry_sdk::metrics::PeriodicReader::builder(metric_exporter).build();

        let meter_provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder()
            .with_reader(metric_reader)
            .with_resource(
                opentelemetry_sdk::Resource::builder()
                    .with_service_name(service_name.to_string())
                    .build(),
            )
            .build();

        let meter_provider_clone = meter_provider.clone();
        global::set_meter_provider(meter_provider);

        let meter = global::meter("sen");

        let agent_starts = meter
            .u64_counter("sen.agent.starts")
            .with_description("Total agent invocations")
            .build();

        let agent_duration = meter
            .f64_histogram("sen.agent.duration")
            .with_description("Agent invocation duration in seconds")
            .with_unit("s")
            .build();

        let llm_calls = meter
            .u64_counter("sen.llm.calls")
            .with_description("Total LLM provider calls")
            .build();

        let llm_duration = meter
            .f64_histogram("sen.llm.duration")
            .with_description("LLM provider call duration in seconds")
            .with_unit("s")
            .build();

        let tool_calls = meter
            .u64_counter("sen.tool.calls")
            .with_description("Total tool calls")
            .build();

        let tool_duration = meter
            .f64_histogram("sen.tool.duration")
            .with_description("Tool execution duration in seconds")
            .with_unit("s")
            .build();

        let channel_messages = meter
            .u64_counter("sen.channel.messages")
            .with_description("Total channel messages")
            .build();

        let heartbeat_ticks = meter
            .u64_counter("sen.heartbeat.ticks")
            .with_description("Total heartbeat ticks")
            .build();

        let errors = meter
            .u64_counter("sen.errors")
            .with_description("Total errors by component")
            .build();

        let request_latency = meter
            .f64_histogram("sen.request.latency")
            .with_description("Request latency in seconds")
            .with_unit("s")
            .build();

        let tokens_used = meter
            .u64_counter("sen.tokens.used")
            .with_description("Total tokens consumed (monotonic)")
            .build();

        let active_sessions = meter
            .u64_gauge("sen.sessions.active")
            .with_description("Current number of active sessions")
            .build();

        let queue_depth = meter
            .u64_gauge("sen.queue.depth")
            .with_description("Current message queue depth")
            .build();

        let hand_runs = meter
            .u64_counter("sen.hand.runs")
            .with_description("Total hand runs")
            .build();

        let hand_duration = meter
            .f64_histogram("sen.hand.duration")
            .with_description("Hand run duration in seconds")
            .with_unit("s")
            .build();

        let hand_findings = meter
            .u64_counter("sen.hand.findings")
            .with_description("Total findings produced by hand runs")
            .build();

        let deployment_lead_time = meter
            .f64_histogram("sen.dora.deployment_lead_time")
            .with_description("Time from commit to deployment completion (DORA metric)")
            .with_unit("s")
            .build();

        let recovery_time = meter
            .f64_histogram("sen.dora.recovery_time")
            .with_description("Time to recover from a failed deployment (DORA metric)")
            .with_unit("s")
            .build();

        Ok(Self {
            tracer_provider,
            meter_provider: meter_provider_clone,
            agent_starts,
            agent_duration,
            llm_calls,
            llm_duration,
            tool_calls,
            tool_duration,
            channel_messages,
            heartbeat_ticks,
            errors,
            request_latency,
            tokens_used,
            active_sessions,
            queue_depth,
            hand_runs,
            hand_duration,
            hand_findings,
            deployment_lead_time,
            recovery_time,
        })
    }
}

impl Observer for OtelObserver {
    fn record_event(&self, event: &ObserverEvent) {
        let tracer = global::tracer("sen");

        match event {
            ObserverEvent::AgentStart { provider, model } => {
                self.agent_starts.add(
                    1,
                    &[
                        KeyValue::new("provider", provider.clone()),
                        KeyValue::new("model", model.clone()),
                    ],
                );
            }
            ObserverEvent::LlmRequest { .. }
            | ObserverEvent::ToolCallStart { .. }
            | ObserverEvent::TurnComplete
            | ObserverEvent::CacheHit { .. }
            | ObserverEvent::CacheMiss { .. } => {}
            ObserverEvent::LlmResponse {
                provider,
                model,
                duration,
                success,
                error_message: _,
                input_tokens: _,
                output_tokens: _,
            } => {
                let secs = duration.as_secs_f64();
                let attrs = [
                    KeyValue::new("provider", provider.clone()),
                    KeyValue::new("model", model.clone()),
                    KeyValue::new("success", success.to_string()),
                ];
                self.llm_calls.add(1, &attrs);
                self.llm_duration.record(secs, &attrs);

                let start_time = SystemTime::now()
                    .checked_sub(*duration)
                    .unwrap_or(SystemTime::now());
                let mut span = tracer.build(
                    opentelemetry::trace::SpanBuilder::from_name("llm.call")
                        .with_kind(SpanKind::Internal)
                        .with_start_time(start_time)
                        .with_attributes(vec![
                            KeyValue::new("provider", provider.clone()),
                            KeyValue::new("model", model.clone()),
                            KeyValue::new("success", *success),
                            KeyValue::new("duration_s", secs),
                        ]),
                );
                if *success {
                    span.set_status(Status::Ok);
                } else {
                    span.set_status(Status::error(""));
                }
                span.end();
            }
            ObserverEvent::AgentEnd {
                provider,
                model,
                duration,
                tokens_used,
                cost_usd,
            } => {
                let secs = duration.as_secs_f64();
                let start_time = SystemTime::now()
                    .checked_sub(*duration)
                    .unwrap_or(SystemTime::now());

                let mut span = tracer.build(
                    opentelemetry::trace::SpanBuilder::from_name("agent.invocation")
                        .with_kind(SpanKind::Internal)
                        .with_start_time(start_time)
                        .with_attributes(vec![
                            KeyValue::new("provider", provider.clone()),
                            KeyValue::new("model", model.clone()),
                            KeyValue::new("duration_s", secs),
                        ]),
                );
                if let Some(t) = tokens_used {
                    span.set_attribute(KeyValue::new("tokens_used", *t as i64));
                }
                if let Some(c) = cost_usd {
                    span.set_attribute(KeyValue::new("cost_usd", *c));
                }
                span.end();

                self.agent_duration.record(
                    secs,
                    &[
                        KeyValue::new("provider", provider.clone()),
                        KeyValue::new("model", model.clone()),
                    ],
                );

            }
            ObserverEvent::ToolCall {
                tool,
                duration,
                success,
            } => {
                let secs = duration.as_secs_f64();
                let start_time = SystemTime::now()
                    .checked_sub(*duration)
                    .unwrap_or(SystemTime::now());

                let status = if *success {
                    Status::Ok
                } else {
                    Status::error("")
                };

                let mut span = tracer.build(
                    opentelemetry::trace::SpanBuilder::from_name("tool.call")
                        .with_kind(SpanKind::Internal)
                        .with_start_time(start_time)
                        .with_attributes(vec![
                            KeyValue::new("tool.name", tool.clone()),
                            KeyValue::new("tool.success", *success),
                            KeyValue::new("duration_s", secs),
                        ]),
                );
                span.set_status(status);
                span.end();

                let attrs = [
                    KeyValue::new("tool", tool.clone()),
                    KeyValue::new("success", success.to_string()),
                ];
                self.tool_calls.add(1, &attrs);
                self.tool_duration
                    .record(secs, &[KeyValue::new("tool", tool.clone())]);
            }
            ObserverEvent::ChannelMessage { channel, direction } => {
                self.channel_messages.add(
                    1,
                    &[
                        KeyValue::new("channel", channel.clone()),
                        KeyValue::new("direction", direction.clone()),
                    ],
                );
            }
            ObserverEvent::HeartbeatTick => {
                self.heartbeat_ticks.add(1, &[]);
            }
            ObserverEvent::Error { component, message } => {

                let mut span = tracer.build(
                    opentelemetry::trace::SpanBuilder::from_name("error")
                        .with_kind(SpanKind::Internal)
                        .with_attributes(vec![
                            KeyValue::new("component", component.clone()),
                            KeyValue::new("error.message", message.clone()),
                        ]),
                );
                span.set_status(Status::error(message.clone()));
                span.end();

                self.errors
                    .add(1, &[KeyValue::new("component", component.clone())]);
            }
            ObserverEvent::HandStarted { .. } => {}
            ObserverEvent::HandCompleted {
                hand_name,
                duration_ms,
                findings_count,
            } => {
                let secs = *duration_ms as f64 / 1000.0;
                let duration = std::time::Duration::from_millis(*duration_ms);
                let start_time = SystemTime::now()
                    .checked_sub(duration)
                    .unwrap_or(SystemTime::now());

                let mut span = tracer.build(
                    opentelemetry::trace::SpanBuilder::from_name("hand.run")
                        .with_kind(SpanKind::Internal)
                        .with_start_time(start_time)
                        .with_attributes(vec![
                            KeyValue::new("hand.name", hand_name.clone()),
                            KeyValue::new("hand.success", true),
                            KeyValue::new("hand.findings", *findings_count as i64),
                            KeyValue::new("duration_s", secs),
                        ]),
                );
                span.set_status(Status::Ok);
                span.end();

                let attrs = [
                    KeyValue::new("hand", hand_name.clone()),
                    KeyValue::new("success", "true"),
                ];
                self.hand_runs.add(1, &attrs);
                self.hand_duration
                    .record(secs, &[KeyValue::new("hand", hand_name.clone())]);
                self.hand_findings.add(
                    *findings_count as u64,
                    &[KeyValue::new("hand", hand_name.clone())],
                );
            }
            ObserverEvent::HandFailed {
                hand_name,
                error,
                duration_ms,
            } => {
                let secs = *duration_ms as f64 / 1000.0;
                let duration = std::time::Duration::from_millis(*duration_ms);
                let start_time = SystemTime::now()
                    .checked_sub(duration)
                    .unwrap_or(SystemTime::now());

                let mut span = tracer.build(
                    opentelemetry::trace::SpanBuilder::from_name("hand.run")
                        .with_kind(SpanKind::Internal)
                        .with_start_time(start_time)
                        .with_attributes(vec![
                            KeyValue::new("hand.name", hand_name.clone()),
                            KeyValue::new("hand.success", false),
                            KeyValue::new("error.message", error.clone()),
                            KeyValue::new("duration_s", secs),
                        ]),
                );
                span.set_status(Status::error(error.clone()));
                span.end();

                let attrs = [
                    KeyValue::new("hand", hand_name.clone()),
                    KeyValue::new("success", "false"),
                ];
                self.hand_runs.add(1, &attrs);
                self.hand_duration
                    .record(secs, &[KeyValue::new("hand", hand_name.clone())]);
            }
            ObserverEvent::DeploymentStarted { deploy_id } => {
                let mut span = tracer.start_with_context(
                    format!("deployment.{deploy_id}"),
                    &opentelemetry::Context::current(),
                );
                span.set_attribute(KeyValue::new("deploy_id", deploy_id.clone()));
                span.set_attribute(KeyValue::new("event", "started"));
                span.end();
                tracing::info!(deploy_id = %deploy_id, "Deployment started");
            }
            ObserverEvent::DeploymentCompleted {
                deploy_id,
                commit_sha,
            } => {
                let mut span = tracer.start_with_context(
                    format!("deployment.{deploy_id}.complete"),
                    &opentelemetry::Context::current(),
                );
                span.set_attribute(KeyValue::new("deploy_id", deploy_id.clone()));
                span.set_attribute(KeyValue::new("commit_sha", commit_sha.clone()));
                span.set_attribute(KeyValue::new("event", "completed"));
                span.end();
                tracing::info!(deploy_id = %deploy_id, commit_sha = %commit_sha, "Deployment completed");
            }
            ObserverEvent::DeploymentFailed { deploy_id, reason } => {
                let mut span = tracer.start_with_context(
                    format!("deployment.{deploy_id}.failed"),
                    &opentelemetry::Context::current(),
                );
                span.set_attribute(KeyValue::new("deploy_id", deploy_id.clone()));
                span.set_attribute(KeyValue::new("reason", reason.clone()));
                span.set_status(Status::error(reason.clone()));
                span.end();
                self.errors
                    .add(1, &[KeyValue::new("kind", "deployment_failed")]);
                tracing::warn!(deploy_id = %deploy_id, reason = %reason, "Deployment failed");
            }
            ObserverEvent::RecoveryCompleted { deploy_id } => {
                let mut span = tracer.start_with_context(
                    format!("recovery.{deploy_id}"),
                    &opentelemetry::Context::current(),
                );
                span.set_attribute(KeyValue::new("deploy_id", deploy_id.clone()));
                span.set_attribute(KeyValue::new("event", "recovery_completed"));
                span.end();
                tracing::info!(deploy_id = %deploy_id, "Recovery completed");
            }
        }
    }

    fn record_metric(&self, metric: &ObserverMetric) {
        match metric {
            ObserverMetric::RequestLatency(d) => {
                self.request_latency.record(d.as_secs_f64(), &[]);
            }
            ObserverMetric::TokensUsed(t) => {
                self.tokens_used.add(*t as u64, &[]);
            }
            ObserverMetric::ActiveSessions(s) => {
                self.active_sessions.record(*s as u64, &[]);
            }
            ObserverMetric::QueueDepth(d) => {
                self.queue_depth.record(*d as u64, &[]);
            }
            ObserverMetric::HandRunDuration {
                hand_name,
                duration,
            } => {
                self.hand_duration.record(
                    duration.as_secs_f64(),
                    &[KeyValue::new("hand", hand_name.clone())],
                );
            }
            ObserverMetric::HandFindingsCount { hand_name, count } => {
                self.hand_findings
                    .add(*count, &[KeyValue::new("hand", hand_name.clone())]);
            }
            ObserverMetric::HandSuccessRate { hand_name, success } => {
                let success_str = if *success { "true" } else { "false" };
                self.hand_runs.add(
                    1,
                    &[
                        KeyValue::new("hand", hand_name.clone()),
                        KeyValue::new("success", success_str),
                    ],
                );
            }
            ObserverMetric::DeploymentLeadTime(duration) => {
                self.deployment_lead_time
                    .record(duration.as_secs_f64(), &[]);
            }
            ObserverMetric::RecoveryTime(duration) => {
                self.recovery_time.record(duration.as_secs_f64(), &[]);
            }
            ObserverMetric::FirstTokenLatency {
                agent_id: _,
                elapsed,
            } => {

                self.request_latency.record(
                    elapsed.as_secs_f64(),
                    &[KeyValue::new("kind", "first_token")],
                );
            }
            ObserverMetric::ResponseCacheOutcome {
                provider,
                model,
                hit,
            } => {

                self.llm_calls.add(
                    1,
                    &[
                        KeyValue::new("provider", provider.clone()),
                        KeyValue::new("model", model.clone()),
                        KeyValue::new("cache", if *hit { "hit" } else { "miss" }),
                    ],
                );
            }
            ObserverMetric::FlowRun {
                flow,
                outcome,
                duration,
            } => {

                let secs = duration.as_secs_f64();
                let start_time = SystemTime::now()
                    .checked_sub(*duration)
                    .unwrap_or(SystemTime::now());
                let tracer = global::tracer("sen");
                let mut span = tracer.build(
                    opentelemetry::trace::SpanBuilder::from_name("flow.run")
                        .with_kind(SpanKind::Internal)
                        .with_start_time(start_time)
                        .with_attributes(vec![
                            KeyValue::new("flow.name", flow.clone()),
                            KeyValue::new("flow.outcome", outcome.clone()),
                            KeyValue::new("duration_s", secs),
                        ]),
                );
                if outcome == "success" {
                    span.set_status(Status::Ok);
                } else {
                    span.set_status(Status::error(outcome.clone()));
                }
                span.end();

                self.request_latency.record(
                    secs,
                    &[
                        KeyValue::new("flow", flow.clone()),
                        KeyValue::new("outcome", outcome.clone()),
                        KeyValue::new("kind", "flow_run"),
                    ],
                );
            }
        }
    }

    fn flush(&self) {
        if let Err(e) = self.tracer_provider.force_flush() {
            tracing::warn!("OTel trace flush failed: {e}");
        }
        if let Err(e) = self.meter_provider.force_flush() {
            tracing::warn!("OTel metric flush failed: {e}");
        }
    }

    fn name(&self) -> &str {
        "otel"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
