// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::bundle::{BundleStep, SessionBundle};

pub const DESCRIPTION_FILE: &str = "description.md";

fn fmt_dur(ms: i64) -> String {
    let total = (ms.max(0) as u64 + 500) / 1000;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

fn fmt_offset(ms: i64) -> String {
    let total = (ms.max(0) as u64 + 500) / 1000;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn uniq(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in values {
        if !v.is_empty() && !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

fn overview(bundle: &SessionBundle) -> String {
    if bundle.steps.is_empty() {
        return "No activity was captured during this session.".to_string();
    }
    let apps = uniq(bundle.steps.iter().filter_map(|s| s.app.clone()).map(|a| a));
    let hosts = uniq(bundle.steps.iter().flat_map(|s| s.hosts.clone()));
    let input_count: usize = bundle.steps.iter().map(|s| s.inputs.len()).sum();
    let marker_count: usize = bundle.steps.iter().map(|s| s.markers.len()).sum();

    let mut text = format!(
        "Over {} the user moved through {} step{}",
        fmt_dur(bundle.session.duration_ms),
        bundle.steps.len(),
        if bundle.steps.len() == 1 { "" } else { "s" },
    );
    if !apps.is_empty() {
        let shown = apps.iter().take(4).cloned().collect::<Vec<_>>().join(", ");
        text.push_str(&format!(
            " across {} app{} ({shown})",
            apps.len(),
            if apps.len() == 1 { "" } else { "s" }
        ));
    }
    text.push('.');
    if !hosts.is_empty() {
        text.push_str(&format!(
            " Visited {} site{}: {}.",
            hosts.len(),
            if hosts.len() == 1 { "" } else { "s" },
            hosts.iter().take(6).cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if input_count > 0 {
        text.push_str(&format!(" Performed {input_count} recorded input action{}.", if input_count == 1 { "" } else { "s" }));
    }
    if marker_count > 0 {
        text.push_str(&format!(" Left {marker_count} note{}.", if marker_count == 1 { "" } else { "s" }));
    }
    text
}

fn step_details(step: &BundleStep) -> Vec<String> {
    let mut out = Vec::new();
    if !step.urls.is_empty() {
        out.push(format!(
            "- Opened: {}",
            step.urls
                .iter()
                .take(5)
                .map(|u| format!("`{u}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    } else if !step.hosts.is_empty() {
        out.push(format!("- Sites: {}", step.hosts.join(", ")));
    }
    if !step.titles.is_empty() {
        out.push(format!(
            "- Windows: {}",
            step.titles.iter().take(5).cloned().collect::<Vec<_>>().join("; ")
        ));
    }
    if !step.inputs.is_empty() {
        out.push(format!(
            "- Inputs: {}",
            step.inputs.iter().take(8).cloned().collect::<Vec<_>>().join("; ")
        ));
    }
    if step.clipboard_count > 0 {
        out.push(format!("- Clipboard changes: {}", step.clipboard_count));
    }
    if !step.markers.is_empty() {
        out.push(format!(
            "- Notes: {}",
            step.markers
                .iter()
                .map(|m| format!("\"{m}\""))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if !step.frames.is_empty() {
        out.push(format!("- Keyframes: {}", step.frames.len()));
    }
    out
}

pub fn render_description(bundle: &SessionBundle) -> String {
    let mut lines: Vec<String> = Vec::new();
    let started = chrono::DateTime::from_timestamp_millis(bundle.session.started_at)
        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default();

    lines.push(format!("# Session recording — {started} UTC"));
    lines.push(String::new());

    let mut meta = vec![
        fmt_dur(bundle.session.duration_ms),
        format!(
            "{} step{}",
            bundle.stats.step_count,
            if bundle.stats.step_count == 1 { "" } else { "s" }
        ),
        format!("{} events", bundle.stats.meaningful_event_count),
    ];
    if bundle.stats.frame_count > 0 {
        meta.push(format!("{} keyframes", bundle.stats.frame_count));
    }
    lines.push(format!("_{}_", meta.join(" · ")));
    lines.push(String::new());

    if !bundle.session.task.trim().is_empty() {
        lines.push("## Task".to_string());
        lines.push(bundle.session.task.trim().to_string());
        lines.push(String::new());
    }

    lines.push("## Overview".to_string());
    lines.push(overview(bundle));
    lines.push(String::new());

    lines.push("## Steps".to_string());
    if bundle.steps.is_empty() {
        lines.push(String::new());
        lines.push("_No activity captured._".to_string());
    }
    for step in &bundle.steps {
        let offset = fmt_offset(step.start_ms - bundle.session.started_at);
        lines.push(String::new());
        lines.push(format!("### {}. {}", step.index + 1, step.summary));
        let mut head = vec![format!("`+{offset}`"), fmt_dur(step.duration_ms)];
        if let Some(app) = step.app.as_deref() {
            head.push(app.to_string());
        }
        lines.push(head.join(" · "));
        for detail in step_details(step) {
            lines.push(detail);
        }
    }

    lines.push(String::new());
    lines.join("\n")
}
