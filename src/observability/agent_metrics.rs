// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Agent-level metrics aggregator.
//!
//! Tracks the key counters the operations team needs to monitor:
//!
//! - `sen_turns_total{status}`
//! - `sen_tool_calls_total{name, status}`
//! - `sen_tokens_in_total{provider, model}`
//! - `sen_tokens_out_total{provider, model}`
//! - `sen_cost_usd_total{provider, model}`
//! - `sen_active_agents` (gauge)
//! - `sen_blackboard_entries` (gauge)
//!
//! The implementation uses `parking_lot::RwLock<HashMap>` so it works in
//! both the `observability-prometheus` feature path and the bare build —
//! the Prometheus observer (when enabled) walks these counters and
//! exports them.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct LabelSet {
    pub pairs: Vec<(String, String)>,
}

impl LabelSet {
    pub fn new(pairs: Vec<(&str, &str)>) -> Self {
        Self {
            pairs: pairs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    pub fn to_prometheus_suffix(&self) -> String {
        if self.pairs.is_empty() {
            return String::new();
        }
        let mut sorted = self.pairs.clone();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let body: Vec<String> = sorted
            .iter()
            .map(|(k, v)| format!("{k}=\"{}\"", v.replace('"', "\\\"")))
            .collect();
        format!("{{{}}}", body.join(","))
    }
}

#[derive(Default, Clone)]
pub struct AgentMetrics {
    inner: Arc<RwLock<MetricsInner>>,
}

#[derive(Default)]
struct MetricsInner {
    counters: HashMap<(String, LabelSet), u64>,
    gauges: HashMap<(String, LabelSet), f64>,
    histograms: HashMap<(String, LabelSet), HistogramState>,
}

#[derive(Debug, Clone)]
pub struct HistogramState {

    buckets: Vec<f64>,

    counts: Vec<u64>,

    total_count: u64,

    sum: f64,
}

impl HistogramState {
    pub fn new(buckets: Vec<f64>) -> Self {
        let len = buckets.len();
        Self {
            buckets,
            counts: vec![0; len],
            total_count: 0,
            sum: 0.0,
        }
    }

    pub fn observe(&mut self, value: f64) {
        self.total_count = self.total_count.saturating_add(1);
        self.sum += value;
        for (idx, upper) in self.buckets.iter().enumerate() {
            if value <= *upper {
                self.counts[idx] = self.counts[idx].saturating_add(1);
            }
        }
    }

    pub fn buckets(&self) -> &[f64] {
        &self.buckets
    }

    pub fn counts(&self) -> &[u64] {
        &self.counts
    }

    pub fn total_count(&self) -> u64 {
        self.total_count
    }

    pub fn sum(&self) -> f64 {
        self.sum
    }
}

pub fn default_latency_ms_buckets() -> Vec<f64> {
    vec![
        50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 30000.0,
    ]
}

impl AgentMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn inc(&self, name: &str, labels: LabelSet) {
        self.inc_by(name, labels, 1);
    }

    pub fn inc_by(&self, name: &str, labels: LabelSet, delta: u64) {
        let mut guard = self.inner.write();
        let entry = guard
            .counters
            .entry((name.to_string(), labels))
            .or_insert(0);
        *entry = entry.saturating_add(delta);
    }

    pub fn set_gauge(&self, name: &str, labels: LabelSet, value: f64) {
        self.inner
            .write()
            .gauges
            .insert((name.to_string(), labels), value);
    }

    pub fn get_counter(&self, name: &str, labels: &LabelSet) -> u64 {
        self.inner
            .read()
            .counters
            .get(&(name.to_string(), labels.clone()))
            .copied()
            .unwrap_or(0)
    }

    pub fn get_gauge(&self, name: &str, labels: &LabelSet) -> f64 {
        self.inner
            .read()
            .gauges
            .get(&(name.to_string(), labels.clone()))
            .copied()
            .unwrap_or(0.0)
    }

    pub fn snapshot_counters(&self) -> Vec<(String, LabelSet, u64)> {
        self.inner
            .read()
            .counters
            .iter()
            .map(|((name, labels), v)| (name.clone(), labels.clone(), *v))
            .collect()
    }

    pub fn observe_histogram(&self, name: &str, labels: LabelSet, value: f64, buckets: &[f64]) {
        let mut guard = self.inner.write();
        let key = (name.to_string(), labels);
        let entry = guard
            .histograms
            .entry(key)
            .or_insert_with(|| HistogramState::new(buckets.to_vec()));
        entry.observe(value);
    }

    pub fn snapshot_histograms(&self) -> Vec<(String, LabelSet, HistogramState)> {
        self.inner
            .read()
            .histograms
            .iter()
            .map(|((name, labels), h)| (name.clone(), labels.clone(), h.clone()))
            .collect()
    }

    pub fn snapshot_gauges(&self) -> Vec<(String, LabelSet, f64)> {
        self.inner
            .read()
            .gauges
            .iter()
            .map(|((name, labels), v)| (name.clone(), labels.clone(), *v))
            .collect()
    }

    pub fn render_prometheus(&self) -> String {
        use std::collections::BTreeMap;
        let mut counter_series: BTreeMap<String, Vec<(LabelSet, u64)>> = BTreeMap::new();
        let mut gauge_series: BTreeMap<String, Vec<(LabelSet, f64)>> = BTreeMap::new();
        let mut histogram_series: BTreeMap<String, Vec<(LabelSet, HistogramState)>> =
            BTreeMap::new();
        for (name, labels, value) in self.snapshot_counters() {
            counter_series
                .entry(name)
                .or_default()
                .push((labels, value));
        }
        for (name, labels, value) in self.snapshot_gauges() {
            gauge_series.entry(name).or_default().push((labels, value));
        }
        for (name, labels, hist) in self.snapshot_histograms() {
            histogram_series
                .entry(name)
                .or_default()
                .push((labels, hist));
        }

        let mut out = String::new();
        for (name, samples) in counter_series {
            let help = prometheus_help(&name);
            out.push_str(&format!("# HELP {name} {help}\n"));
            out.push_str(&format!("# TYPE {name} counter\n"));
            for (labels, value) in samples {
                out.push_str(&format!(
                    "{}{} {}\n",
                    name,
                    labels.to_prometheus_suffix(),
                    value
                ));
            }
        }
        for (name, samples) in gauge_series {
            let help = prometheus_help(&name);
            out.push_str(&format!("# HELP {name} {help}\n"));
            out.push_str(&format!("# TYPE {name} gauge\n"));
            for (labels, value) in samples {
                out.push_str(&format!(
                    "{}{} {}\n",
                    name,
                    labels.to_prometheus_suffix(),
                    value
                ));
            }
        }
        for (name, samples) in histogram_series {
            let help = prometheus_help(&name);
            out.push_str(&format!("# HELP {name} {help}\n"));
            out.push_str(&format!("# TYPE {name} histogram\n"));
            for (labels, hist) in samples {
                let base_labels = &labels;
                for (idx, upper) in hist.buckets().iter().enumerate() {
                    let mut bucket_labels = base_labels.pairs.clone();
                    bucket_labels.push(("le".to_string(), format!("{}", upper)));
                    let suffix = label_suffix_from_pairs(&bucket_labels);
                    out.push_str(&format!(
                        "{}_bucket{} {}\n",
                        name,
                        suffix,
                        hist.counts()[idx]
                    ));
                }

                let mut inf_labels = base_labels.pairs.clone();
                inf_labels.push(("le".to_string(), "+Inf".to_string()));
                let suffix = label_suffix_from_pairs(&inf_labels);
                out.push_str(&format!(
                    "{}_bucket{} {}\n",
                    name,
                    suffix,
                    hist.total_count()
                ));
                out.push_str(&format!(
                    "{}_sum{} {}\n",
                    name,
                    base_labels.to_prometheus_suffix(),
                    hist.sum()
                ));
                out.push_str(&format!(
                    "{}_count{} {}\n",
                    name,
                    base_labels.to_prometheus_suffix(),
                    hist.total_count()
                ));
            }
        }
        out
    }
}

fn label_suffix_from_pairs(pairs: &[(String, String)]) -> String {
    if pairs.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<(String, String)> = pairs.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let body: Vec<String> = sorted
        .iter()
        .map(|(k, v)| format!("{k}=\"{}\"", v.replace('"', "\\\"")))
        .collect();
    format!("{{{}}}", body.join(","))
}

fn prometheus_help(name: &str) -> &'static str {
    match name {
        "sen_turns_total" => "Total number of agent turns, labelled by completion status",
        "sen_tool_calls_total" => "Total tool invocations by tool name and status",
        "sen_tokens_in_total" => "Cumulative input tokens consumed by provider/model",
        "sen_tokens_out_total" => "Cumulative output tokens produced by provider/model",
        "sen_cost_usd_total" => "Cumulative spend in USD by provider/model",
        "sen_last_turn_duration_secs" => "Wall-clock duration of the most recent completed turn",
        "sen_gc_memory_evicted_total" => "Blackboard entries expired by GC",
        "sen_gc_runtime_maintenance_runs_total" => {
            "Number of multi-agent runtime maintenance sweeps"
        }
        "sen_first_token_latency_ms" => {
            "Latency in milliseconds between TurnStarted and the first streamed token, \
             labelled by agent_id"
        }
        "sen_response_cache_hits_total" => {
            "Provider response-cache hit count, labelled by provider/model"
        }
        "sen_response_cache_misses_total" => {
            "Provider response-cache miss count, labelled by provider/model"
        }
        _ => "Runtime-registered metric",
    }
}

pub fn record_first_token_latency_ms(metrics: &AgentMetrics, agent_id: &str, elapsed_ms: u64) {
    metrics.observe_histogram(
        "sen_first_token_latency_ms",
        LabelSet::new(vec![("agent_id", agent_id)]),
        elapsed_ms as f64,
        &default_latency_ms_buckets(),
    );
}

pub fn record_response_cache_outcome(
    metrics: &AgentMetrics,
    provider: &str,
    model: &str,
    hit: bool,
) {
    let metric = if hit {
        "sen_response_cache_hits_total"
    } else {
        "sen_response_cache_misses_total"
    };
    metrics.inc(
        metric,
        LabelSet::new(vec![("provider", provider), ("model", model)]),
    );
}

pub fn inc_turns(metrics: &AgentMetrics, status: &str) {
    metrics.inc("sen_turns_total", LabelSet::new(vec![("status", status)]));
}

pub fn inc_tool_call(metrics: &AgentMetrics, tool_name: &str, status: &str) {
    metrics.inc(
        "sen_tool_calls_total",
        LabelSet::new(vec![("name", tool_name), ("status", status)]),
    );
}

pub fn record_tokens(
    metrics: &AgentMetrics,
    provider: &str,
    model: &str,
    tokens_in: u64,
    tokens_out: u64,
) {
    let labels = LabelSet::new(vec![("provider", provider), ("model", model)]);
    metrics.inc_by("sen_tokens_in_total", labels.clone(), tokens_in);
    metrics.inc_by("sen_tokens_out_total", labels, tokens_out);
}

pub fn record_cost(metrics: &AgentMetrics, provider: &str, model: &str, cost_usd_micros: u64) {
    metrics.inc_by(
        "sen_cost_usd_micros_total",
        LabelSet::new(vec![("provider", provider), ("model", model)]),
        cost_usd_micros,
    );
}

pub fn set_active_agents(metrics: &AgentMetrics, n: usize) {
    metrics.set_gauge("sen_active_agents", LabelSet::new(vec![]), n as f64);
}

pub fn set_blackboard_entries(metrics: &AgentMetrics, n: usize) {
    metrics.set_gauge("sen_blackboard_entries", LabelSet::new(vec![]), n as f64);
}
