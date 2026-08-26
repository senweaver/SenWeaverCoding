// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use base64::Engine as _;
use std::path::Path;

use crate::computer::activity::events::{is_meaningful_event, read_events};
use crate::computer::frames;
use crate::computer::sensitive::{FramePolicy, RedactionContext};
use crate::computer::timeline::load_bundle;

pub const MAX_EVENT_ROWS: usize = 500;
pub const MAX_STRING_CHARS: usize = 2000;
pub const MAX_IMAGES_PER_CALL: usize = 6;

pub struct ToolOutput {
    pub text: String,
    pub images: Vec<String>,
}

impl ToolOutput {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            images: Vec::new(),
        }
    }
}

fn truncate(value: &str) -> String {
    if value.chars().count() > MAX_STRING_CHARS {
        let cut: String = value.chars().take(MAX_STRING_CHARS).collect();
        format!("{cut}…[truncated]")
    } else {
        value.to_string()
    }
}

fn mmss(at_ms: i64) -> String {
    let total = (at_ms.max(0) as u64 + 500) / 1000;
    format!("{:02}:{:02}", total / 60, total % 60)
}

pub fn run_tool(
    dir: &Path,
    started_epoch: i64,
    redaction: &mut RedactionContext,
    name: &str,
    args: &serde_json::Value,
) -> ToolOutput {
    match name {
        "get_timeline" => get_timeline(dir, started_epoch, redaction),
        "get_events" => get_events(dir, started_epoch, redaction, args),
        "get_narration" => get_narration(dir, redaction, args),
        "list_frames" => list_frames_tool(dir, started_epoch),
        "get_frames" => get_frames(dir, redaction, args),
        other => ToolOutput::text(format!(
            "Unknown tool '{other}'. Available tools: get_timeline, get_events, get_narration, list_frames, get_frames, submit_analysis."
        )),
    }
}

fn get_timeline(dir: &Path, started_epoch: i64, redaction: &RedactionContext) -> ToolOutput {
    let Some(bundle) = load_bundle(dir) else {
        return ToolOutput::text(
            "No timeline is available yet (bundle.json missing). Use get_events to read the raw event stream instead.",
        );
    };
    let steps: Vec<serde_json::Value> = bundle
        .steps
        .iter()
        .map(|s| {
            serde_json::json!({
                "index": s.index,
                "atMs": (s.start_ms - started_epoch).max(0),
                "durationMs": s.duration_ms,
                "boundary": s.boundary,
                "app": s.app,
                "titles": s.titles,
                "hosts": s.hosts,
                "urls": s.urls,
                "inputs": s.inputs,
                "clipboardCount": s.clipboard_count,
                "markers": s.markers,
                "frameCount": s.frames.len(),
                "summary": s.summary,
            })
        })
        .collect();
    let view = serde_json::json!({
        "durationMs": bundle.session.duration_ms,
        "platform": bundle.session.platform,
        "taskDescription": bundle.session.task,
        "stats": {
            "eventCount": bundle.stats.event_count,
            "meaningfulEventCount": bundle.stats.meaningful_event_count,
            "stepCount": bundle.stats.step_count,
            "frameCount": bundle.stats.frame_count,
        },
        "steps": steps,
    });
    let text = serde_json::to_string_pretty(&view).unwrap_or_default();
    ToolOutput::text(redaction.redact_text(&text))
}

fn get_events(
    dir: &Path,
    started_epoch: i64,
    redaction: &RedactionContext,
    args: &serde_json::Value,
) -> ToolOutput {
    let wanted: Option<std::collections::BTreeSet<String>> = args
        .get("types")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_string)
                .collect()
        })
        .filter(|set: &std::collections::BTreeSet<String>| !set.is_empty());
    let from_ms = args.get("fromMs").and_then(|v| v.as_i64());
    let to_ms = args.get("toMs").and_then(|v| v.as_i64());

    let events = read_events(dir);
    let mut rows = Vec::new();
    let mut total = 0usize;
    for event in &events {
        let at = event.epoch - started_epoch;
        if let Some(from) = from_ms {
            if at < from {
                continue;
            }
        }
        if let Some(to) = to_ms {
            if at > to {
                continue;
            }
        }
        let included = match &wanted {
            Some(set) => set.contains(&event.kind),
            None => is_meaningful_event(&event.kind),
        };
        if !included {
            continue;
        }
        total += 1;
        if rows.len() >= MAX_EVENT_ROWS {
            continue;
        }
        let mut payload = event.payload.clone();
        if let Some(map) = payload.as_object_mut() {
            for value in map.values_mut() {
                if let Some(text) = value.as_str() {
                    *value = serde_json::json!(truncate(text));
                }
            }
        }
        rows.push(serde_json::json!({
            "seq": event.seq,
            "atMs": at,
            "type": event.kind,
            "payload": payload,
        }));
    }

    let mut text = serde_json::to_string_pretty(&rows).unwrap_or_default();
    if total > MAX_EVENT_ROWS {
        text.push_str(&format!(
            "\n\n{} matching events; showing the first {MAX_EVENT_ROWS}. Narrow the window with fromMs/toMs or types.",
            total
        ));
    }
    ToolOutput::text(redaction.redact_text(&text))
}

fn get_narration(
    dir: &Path,
    redaction: &RedactionContext,
    args: &serde_json::Value,
) -> ToolOutput {
    let Some(transcript) = crate::computer::narration::load_transcript(dir) else {
        return ToolOutput::text("The user did not record narration for this session.");
    };
    if transcript.segments.is_empty() {
        return ToolOutput::text("Narration was recorded but no speech was detected.");
    }
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .map(str::to_lowercase)
        .filter(|s| !s.trim().is_empty());
    let mut lines = Vec::new();
    for segment in &transcript.segments {
        if let Some(query) = &query {
            if !segment.text.to_lowercase().contains(query) {
                continue;
            }
        }
        lines.push(format!("[{}] {}", mmss(segment.at_ms), segment.text));
    }
    if lines.is_empty() {
        return ToolOutput::text("No narration lines matched that query.");
    }
    ToolOutput::text(redaction.redact_text(&lines.join("\n")))
}

fn list_frames_tool(dir: &Path, started_epoch: i64) -> ToolOutput {
    let records = frames::list_frames(dir);
    if records.is_empty() {
        return ToolOutput::text("No screen keyframes were captured for this session.");
    }
    let view: Vec<serde_json::Value> = records
        .iter()
        .map(|f| {
            serde_json::json!({
                "file": f.file,
                "atMs": (f.t_ms - started_epoch).max(0),
                "source": f.source,
                "reason": f.reason,
            })
        })
        .collect();
    ToolOutput::text(serde_json::to_string_pretty(&view).unwrap_or_default())
}

fn get_frames(dir: &Path, redaction: &mut RedactionContext, args: &serde_json::Value) -> ToolOutput {
    let from = args
        .get("fromMs")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
        .max(0) as u64;
    let to = args
        .get("toMs")
        .and_then(|v| v.as_i64())
        .map(|v| v.max(0) as u64)
        .unwrap_or(from + 30_000);
    let fps = args.get("fps").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let crop: Option<frames::CropRect> = args
        .get("crop")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    if matches!(redaction.frame_policy, FramePolicy::Withhold) {
        return ToolOutput::text(
            "Screen frames are withheld for this analysis: the on-device sensitive-data protection could not verify them. Rely on the event timeline and narration instead.",
        );
    }

    let extracted = match frames::extract_window(dir, from, to, fps, MAX_IMAGES_PER_CALL, crop) {
        Ok(extracted) => extracted,
        Err(e) => return ToolOutput::text(format!("Frame extraction failed: {e}")),
    };
    if extracted.is_empty() {
        return ToolOutput::text("No keyframes exist in that window. Try list_frames to see what is available.");
    }

    let mut images = Vec::new();
    let mut shown = Vec::new();
    let mut withheld = 0usize;
    for frame in extracted.iter().take(MAX_IMAGES_PER_CALL) {
        let bytes: std::borrow::Cow<[u8]> = match (&redaction.frame_policy, redaction.frame_redactor.as_mut()) {
            (FramePolicy::Redact, Some(redactor)) => {
                match redactor.redact_frame_bytes(&cache_key(&frame.record.file, &crop), &frame.jpeg) {
                    Ok(redacted) => std::borrow::Cow::Owned(redacted.as_ref().clone()),
                    Err(_) => {
                        withheld += 1;
                        continue;
                    }
                }
            }
            _ => std::borrow::Cow::Borrowed(frame.jpeg.as_slice()),
        };
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes.as_ref());
        images.push(format!("data:image/jpeg;base64,{encoded}"));
        shown.push(format!(
            "{} (offset {})",
            frame.record.file,
            mmss(frame.record.offset_ms as i64)
        ));
    }

    let mut text = if shown.is_empty() {
        "Every frame in that window was withheld by the sensitive-data protection.".to_string()
    } else {
        format!(
            "Showing {} frame(s): {}. The images follow in this message.",
            shown.len(),
            shown.join(", ")
        )
    };
    if withheld > 0 && !shown.is_empty() {
        text.push_str(&format!(" {withheld} frame(s) were withheld by the sensitive-data protection."));
    }
    ToolOutput { text, images }
}

fn cache_key(file: &str, crop: &Option<frames::CropRect>) -> String {
    match crop {
        Some(rect) => format!("{file}#{},{},{},{}", rect.x, rect.y, rect.w, rect.h),
        None => file.to_string(),
    }
}
