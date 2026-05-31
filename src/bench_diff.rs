// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
struct RawEstimate {
    point_estimate: f64,
    confidence_interval: Option<ConfidenceInterval>,
    standard_error: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct ConfidenceInterval {
    confidence_level: f64,
    lower_bound: f64,
    upper_bound: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct RawEstimates {
    mean: RawEstimate,
    median: RawEstimate,
    std_dev: Option<RawEstimate>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchEstimate {
    pub name: String,

    pub mean_secs: f64,

    pub median_secs: f64,

    pub std_dev_secs: Option<f64>,

    pub mean_low: f64,

    pub mean_high: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchComparison {
    pub name: String,
    pub baseline_mean_secs: Option<f64>,
    pub current_mean_secs: f64,

    pub delta_ratio: Option<f64>,

    pub is_regression: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchReport {
    pub root: PathBuf,
    pub entries: Vec<BenchComparison>,
}

impl BenchEstimate {
    fn from_raw(name: String, raw: &RawEstimates) -> Self {
        let (low, high) = raw
            .mean
            .confidence_interval
            .as_ref()
            .map(|ci| (ci.lower_bound, ci.upper_bound))
            .unwrap_or((raw.mean.point_estimate, raw.mean.point_estimate));
        Self {
            name,
            mean_secs: raw.mean.point_estimate,
            median_secs: raw.median.point_estimate,
            std_dev_secs: raw.std_dev.as_ref().map(|e| e.point_estimate),
            mean_low: low,
            mean_high: high,
        }
    }
}

pub fn load_estimates(root: impl AsRef<Path>) -> std::io::Result<BenchReport> {
    let root = root.as_ref().to_path_buf();
    let mut pairs: BTreeMap<String, (Option<BenchEstimate>, Option<BenchEstimate>)> =
        BTreeMap::new();

    if !root.exists() {
        return Ok(BenchReport {
            root,
            entries: Vec::new(),
        });
    }

    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let read = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) != Some("estimates.json") {
                continue;
            }

            let variant = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if !matches!(variant, "new" | "base") {
                continue;
            }

            let rel = match path.strip_prefix(&root) {
                Ok(p) => p.to_path_buf(),
                Err(_) => continue,
            };
            let components: Vec<_> = rel
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect();
            if components.len() < 3 {
                continue;
            }

            let name = components[..components.len() - 2].join("/");

            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(raw) = serde_json::from_str::<RawEstimates>(&text) else {
                continue;
            };
            let est = BenchEstimate::from_raw(name.clone(), &raw);
            let slot = pairs.entry(name).or_insert((None, None));
            match variant {
                "new" => slot.0 = Some(est),
                "base" => slot.1 = Some(est),
                _ => {}
            }
        }
    }

    let entries = pairs
        .into_iter()
        .filter_map(|(name, (new, base))| {
            let cur = new?;
            let baseline = base.as_ref().map(|b| b.mean_secs);
            let delta = match baseline {
                Some(b) if b > 0.0 => Some((cur.mean_secs - b) / b),
                _ => None,
            };
            let is_regression = delta.map_or(false, |d| d > 0.0);
            Some(BenchComparison {
                name,
                baseline_mean_secs: baseline,
                current_mean_secs: cur.mean_secs,
                delta_ratio: delta,
                is_regression,
            })
        })
        .collect();

    Ok(BenchReport { root, entries })
}

pub fn format_regression_table(report: &BenchReport, regression_threshold: f64) -> String {
    let mut out = format!(
        "Benchmark regression report  -  source: {}\n",
        report.root.display()
    );
    out.push_str("─────────────────────────────────────────────────────────────────\n");
    out.push_str(&format!(
        "{:<45}  {:>12}  {:>12}  {:>8}\n",
        "bench", "baseline", "current", "Δ %"
    ));
    out.push_str("─────────────────────────────────────────────────────────────────\n");

    if report.entries.is_empty() {
        out.push_str("(no benchmark data found  -  run `cargo bench` first)\n");
        return out;
    }

    let mut regressions = 0u32;
    for cmp in &report.entries {
        let baseline = cmp
            .baseline_mean_secs
            .map(|s| format_duration(s))
            .unwrap_or_else(|| "-".to_string());
        let current = format_duration(cmp.current_mean_secs);
        let delta = match cmp.delta_ratio {
            Some(d) => {
                let pct = d * 100.0;
                let flag = if d > regression_threshold {
                    regressions += 1;
                    " ⚠"
                } else if d < -regression_threshold {
                    " ✓"
                } else {
                    ""
                };
                format!("{:+.1}%{flag}", pct)
            }
            None => "-".to_string(),
        };
        out.push_str(&format!(
            "{:<45}  {:>12}  {:>12}  {:>8}\n",
            truncate(&cmp.name, 45),
            baseline,
            current,
            delta
        ));
    }

    out.push_str("─────────────────────────────────────────────────────────────────\n");
    out.push_str(&format!(
        "Summary: {} benches, {regressions} regression(s) > {:.0}%\n",
        report.entries.len(),
        regression_threshold * 100.0
    ));
    out
}

fn format_duration(secs: f64) -> String {
    if secs >= 1.0 {
        format!("{:.3} s", secs)
    } else if secs >= 1e-3 {
        format!("{:.3} ms", secs * 1e3)
    } else if secs >= 1e-6 {
        format!("{:.3} µs", secs * 1e6)
    } else {
        format!("{:.3} ns", secs * 1e9)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}
