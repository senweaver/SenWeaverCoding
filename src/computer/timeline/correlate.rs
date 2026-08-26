// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

use crate::computer::activity::events::{is_meaningful_event, ActivityEvent};
use crate::computer::frames::FrameRecord;

pub const CORRELATION_FILE: &str = "correlation.json";
pub const CORRELATION_WINDOW_MS: i64 = 1_500;
const PROBE_PAD_MS: u64 = 1_200;
const GAP_PROBE_THRESHOLD_MS: i64 = 10_000;
const MAX_GAP_PROBES: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrelatedFrame {
    pub file: String,
    pub t_ms: i64,
    pub offset_ms: u64,
    pub source: String,
    pub nearest_event_seqs: Vec<u64>,
    pub unexplained: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSuggestion {
    pub from_offset_ms: u64,
    pub to_offset_ms: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrelationResult {
    pub version: u32,
    pub window_ms: i64,
    pub frames: Vec<CorrelatedFrame>,
    pub silent_event_seqs: Vec<u64>,
    pub probes: Vec<ProbeSuggestion>,
}

pub fn correlate(events: &[ActivityEvent], frames: &[FrameRecord]) -> CorrelationResult {
    let meaningful: Vec<&ActivityEvent> = events
        .iter()
        .filter(|e| is_meaningful_event(&e.kind))
        .collect();

    let mut correlated = Vec::with_capacity(frames.len());
    for frame in frames {
        let nearest: Vec<u64> = meaningful
            .iter()
            .filter(|e| (e.epoch - frame.t_ms).abs() <= CORRELATION_WINDOW_MS)
            .map(|e| e.seq)
            .collect();
        let unexplained = nearest.is_empty() && frame.source != "heartbeat";
        correlated.push(CorrelatedFrame {
            file: frame.file.clone(),
            t_ms: frame.t_ms,
            offset_ms: frame.offset_ms,
            source: frame.source.clone(),
            nearest_event_seqs: nearest,
            unexplained,
        });
    }

    let silent_event_seqs: Vec<u64> = meaningful
        .iter()
        .filter(|e| {
            !frames
                .iter()
                .any(|f| (e.epoch - f.t_ms).abs() <= CORRELATION_WINDOW_MS)
        })
        .map(|e| e.seq)
        .collect();

    let mut probes: Vec<ProbeSuggestion> = Vec::new();
    let mut current: Option<(u64, u64)> = None;
    for frame in correlated.iter().filter(|f| f.unexplained) {
        let from = frame.offset_ms.saturating_sub(PROBE_PAD_MS);
        let to = frame.offset_ms + PROBE_PAD_MS;
        match current.as_mut() {
            Some((_, end)) if from <= *end => {
                *end = (*end).max(to);
            }
            Some(span) => {
                probes.push(ProbeSuggestion {
                    from_offset_ms: span.0,
                    to_offset_ms: span.1,
                    reason: "probe:unexplained".to_string(),
                });
                current = Some((from, to));
            }
            None => current = Some((from, to)),
        }
    }
    if let Some((from, to)) = current {
        probes.push(ProbeSuggestion {
            from_offset_ms: from,
            to_offset_ms: to,
            reason: "probe:unexplained".to_string(),
        });
    }

    let started_epoch = events.first().map(|e| e.epoch).unwrap_or(0);
    let mut gaps: Vec<(i64, i64, i64)> = Vec::new();
    for pair in meaningful.windows(2) {
        let gap = pair[1].epoch - pair[0].epoch;
        if gap > GAP_PROBE_THRESHOLD_MS {
            gaps.push((gap, pair[0].epoch, pair[1].epoch));
        }
    }
    gaps.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, from_epoch, to_epoch) in gaps.into_iter().take(MAX_GAP_PROBES) {
        let from = (from_epoch - started_epoch).max(0) as u64;
        let to = (to_epoch - started_epoch).max(0) as u64;
        probes.push(ProbeSuggestion {
            from_offset_ms: from,
            to_offset_ms: to,
            reason: "probe:gap".to_string(),
        });
    }

    CorrelationResult {
        version: 1,
        window_ms: CORRELATION_WINDOW_MS,
        frames: correlated,
        silent_event_seqs,
        probes,
    }
}
