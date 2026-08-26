// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

use super::correlate::CorrelationResult;
use crate::computer::activity::events::{is_meaningful_event, ActivityEvent};

pub const BUNDLE_FILE: &str = "bundle.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleStep {
    pub index: usize,
    pub start_ms: i64,
    pub end_ms: i64,
    pub duration_ms: i64,
    pub boundary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    pub titles: Vec<String>,
    pub hosts: Vec<String>,
    pub urls: Vec<String>,
    pub inputs: Vec<String>,
    pub clipboard_count: usize,
    pub markers: Vec<String>,
    pub event_seqs: Vec<u64>,
    pub frames: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleSession {
    pub id: String,
    pub started_at: i64,
    pub stopped_at: Option<i64>,
    pub duration_ms: i64,
    pub platform: String,
    pub task: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleStats {
    pub event_count: usize,
    pub meaningful_event_count: usize,
    pub step_count: usize,
    pub frame_count: usize,
    pub unexplained_frame_count: usize,
    pub silent_event_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBundle {
    pub version: u32,
    pub session: BundleSession,
    pub steps: Vec<BundleStep>,
    pub stats: BundleStats,
}

fn push_uniq(list: &mut Vec<String>, value: Option<&str>) {
    if let Some(value) = value {
        let trimmed = value.trim();
        if !trimmed.is_empty() && !list.iter().any(|v| v == trimmed) {
            list.push(trimmed.to_string());
        }
    }
}

fn payload_str<'a>(event: &'a ActivityEvent, key: &str) -> Option<&'a str> {
    event.payload.get(key).and_then(|v| v.as_str())
}

fn render_input(event: &ActivityEvent) -> Option<String> {
    let action = event.kind.strip_prefix("input.")?;
    let value = payload_str(event, "value");
    let out = match action {
        "type" => format!(
            "typed \"{}\"",
            value.map(|v| truncate_chars(v, 80)).unwrap_or_default()
        ),
        "key_press" => format!("pressed {}", value.unwrap_or("a key")),
        "scroll" => format!("scrolled {}", value.unwrap_or("")).trim_end().to_string(),
        "click" => "clicked".to_string(),
        "double_click" => "double-clicked".to_string(),
        "right_click" => "right-clicked".to_string(),
        "drag" => "dragged".to_string(),
        other => other.to_string(),
    };
    Some(out)
}

fn truncate_chars(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

fn summarize_step(step: &BundleStep) -> String {
    let mut bits: Vec<String> = Vec::new();
    if let Some(app) = step.app.as_deref() {
        bits.push(app.to_string());
    }
    if !step.hosts.is_empty() {
        bits.push(format!("on {}", step.hosts.iter().take(3).cloned().collect::<Vec<_>>().join(", ")));
    } else if let Some(title) = step.titles.first() {
        bits.push(format!("— {title}"));
    }
    if !step.inputs.is_empty() {
        bits.push(format!("({} inputs)", step.inputs.len()));
    }
    let mut label = bits.join(" ").trim().to_string();
    if label.is_empty() {
        label = "Activity".to_string();
    }
    if let Some(marker) = step.markers.first() {
        label.push_str(&format!(" — note: \"{marker}\""));
    }
    label
}

pub fn build_bundle(
    session_id: &str,
    task: &str,
    events: &[ActivityEvent],
    correlation: Option<&CorrelationResult>,
) -> SessionBundle {
    let mut meaningful: Vec<&ActivityEvent> = events
        .iter()
        .filter(|e| is_meaningful_event(&e.kind))
        .collect();
    meaningful.sort_by_key(|e| e.epoch);

    let started_at = events.first().map(|e| e.epoch).unwrap_or(0);
    let stopped_at = events
        .iter()
        .find(|e| e.kind == "session.stop")
        .map(|e| e.epoch)
        .or_else(|| events.last().map(|e| e.epoch));

    let mut steps: Vec<BundleStep> = Vec::new();
    let mut cur_app: Option<String> = None;
    let mut cur_host: Option<String> = None;

    for event in &meaningful {
        let app = payload_str(event, "app").map(str::to_string);
        let host = payload_str(event, "host")
            .map(str::to_string)
            .or_else(|| {
                payload_str(event, "url")
                    .and_then(|u| crate::computer::activity::url::host_of(u))
            });

        let boundary = if steps.is_empty() {
            Some("start")
        } else if event.kind == "app.activate"
            && app.is_some()
            && app != cur_app
        {
            Some("app-change")
        } else if event.kind == "browser.url" && host.is_some() && host != cur_host {
            Some("url-change")
        } else {
            None
        };

        if let Some(boundary) = boundary {
            steps.push(BundleStep {
                index: steps.len(),
                start_ms: event.epoch,
                end_ms: event.epoch,
                duration_ms: 0,
                boundary: boundary.to_string(),
                app: app.clone().or_else(|| cur_app.clone()),
                titles: Vec::new(),
                hosts: Vec::new(),
                urls: Vec::new(),
                inputs: Vec::new(),
                clipboard_count: 0,
                markers: Vec::new(),
                event_seqs: Vec::new(),
                frames: Vec::new(),
                summary: String::new(),
            });
        }

        if app.is_some() {
            cur_app = app.clone();
        }
        if host.is_some() {
            cur_host = host.clone();
        }

        let Some(step) = steps.last_mut() else {
            continue;
        };
        step.event_seqs.push(event.seq);
        if step.app.is_none() {
            step.app = app.clone();
        }

        match event.kind.as_str() {
            "app.activate" => {
                push_uniq(&mut step.titles, payload_str(event, "title"));
            }
            "app.title-change" => {
                push_uniq(&mut step.titles, payload_str(event, "title"));
            }
            "browser.url" => {
                push_uniq(&mut step.hosts, host.as_deref());
                push_uniq(&mut step.urls, payload_str(event, "url"));
                push_uniq(&mut step.titles, payload_str(event, "title"));
            }
            "clipboard.change" => {
                step.clipboard_count += 1;
            }
            "marker" => {
                push_uniq(&mut step.markers, payload_str(event, "note"));
            }
            kind if kind.starts_with("input.") => {
                if let Some(rendered) = render_input(event) {
                    if step.inputs.len() < 60 {
                        step.inputs.push(rendered);
                    }
                }
            }
            _ => {}
        }
    }

    let stop_epoch = stopped_at.unwrap_or(started_at);
    let count = steps.len();
    for i in 0..count {
        let end = if i + 1 < count {
            steps[i + 1].start_ms
        } else {
            stop_epoch.max(steps[i].start_ms)
        };
        steps[i].end_ms = end;
        steps[i].duration_ms = end - steps[i].start_ms;
    }

    let mut unexplained = 0usize;
    if let Some(correlation) = correlation {
        let last_index = steps.len().saturating_sub(1);
        for frame in &correlation.frames {
            if frame.unexplained {
                unexplained += 1;
            }
            let target_index = steps
                .iter()
                .position(|s| frame.t_ms >= s.start_ms && frame.t_ms < s.end_ms)
                .unwrap_or(last_index);
            if let Some(step) = steps.get_mut(target_index) {
                if !step.frames.iter().any(|f| f == &frame.file) {
                    step.frames.push(frame.file.clone());
                }
            }
        }
    }

    for step in &mut steps {
        step.summary = summarize_step(step);
    }

    let stats = BundleStats {
        event_count: events.len(),
        meaningful_event_count: meaningful.len(),
        step_count: steps.len(),
        frame_count: correlation.map(|c| c.frames.len()).unwrap_or(0),
        unexplained_frame_count: unexplained,
        silent_event_count: correlation.map(|c| c.silent_event_seqs.len()).unwrap_or(0),
    };

    SessionBundle {
        version: 1,
        session: BundleSession {
            id: session_id.to_string(),
            started_at,
            stopped_at,
            duration_ms: (stop_epoch - started_at).max(0),
            platform: std::env::consts::OS.to_string(),
            task: task.to_string(),
        },
        steps,
        stats,
    }
}

pub fn load_bundle(dir: &std::path::Path) -> Option<SessionBundle> {
    let content = std::fs::read_to_string(dir.join(BUNDLE_FILE)).ok()?;
    serde_json::from_str(&content).ok()
}
