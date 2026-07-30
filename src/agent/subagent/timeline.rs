// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::time::Instant;

use crate::agent::bridge_types::{AgentEvent, SubagentStatus};

const MAX_ENTRIES_PER_LANE: usize = 800;
const MAX_ENTRY_CHARS: usize = 100_000;
const MAX_LANES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneStatus {
    Starting,
    Running,
    Completed,
    Failed,
}

impl LaneStatus {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

#[derive(Debug, Clone)]
pub enum LaneEntry {
    Text(String),
    Thinking(String),
    ToolCall { summary: String },
    ToolResult { preview: String, is_error: bool },
    Status(String),
}

#[derive(Debug)]
pub struct AgentLane {
    pub agent_id: String,
    pub parent_tool_use_id: Option<String>,
    pub title: String,
    pub status: LaneStatus,
    pub entries: Vec<LaneEntry>,
    pub final_output: Option<String>,
    pub started_at: Instant,
    pub updated_at: Instant,
}

fn cap_text(text: &mut String) {
    if text.len() > MAX_ENTRY_CHARS {
        let keep_from = text.len() - MAX_ENTRY_CHARS;
        let boundary = (keep_from..text.len())
            .find(|i| text.is_char_boundary(*i))
            .unwrap_or(text.len());
        text.replace_range(..boundary, "… [truncated] ");
    }
}

impl AgentLane {
    fn new(agent_id: String, title: String, parent_tool_use_id: Option<String>) -> Self {
        let now = Instant::now();
        Self {
            agent_id,
            parent_tool_use_id,
            title,
            status: LaneStatus::Starting,
            entries: Vec::new(),
            final_output: None,
            started_at: now,
            updated_at: now,
        }
    }

    fn push_entry(&mut self, entry: LaneEntry) {
        self.updated_at = Instant::now();
        if let Some(last) = self.entries.last_mut() {
            match (last, &entry) {
                (LaneEntry::Text(prev), LaneEntry::Text(next)) => {
                    prev.push_str(next);
                    cap_text(prev);
                    return;
                }
                (LaneEntry::Thinking(prev), LaneEntry::Thinking(next)) => {
                    prev.push_str(next);
                    cap_text(prev);
                    return;
                }
                _ => {}
            }
        }
        self.entries.push(entry);
        if self.entries.len() > MAX_ENTRIES_PER_LANE {
            let overflow = self.entries.len() - MAX_ENTRIES_PER_LANE;
            self.entries.drain(..overflow);
        }
    }

    #[must_use]
    pub fn last_activity_line(&self) -> String {
        match self.entries.last() {
            Some(LaneEntry::Text(text)) | Some(LaneEntry::Thinking(text)) => {
                first_meaningful_line(text)
            }
            Some(LaneEntry::ToolCall { summary }) => format!("-> {summary}"),
            Some(LaneEntry::ToolResult { preview, is_error }) => {
                if *is_error {
                    format!("<- error: {}", first_meaningful_line(preview))
                } else {
                    format!("<- {}", first_meaningful_line(preview))
                }
            }
            Some(LaneEntry::Status(text)) => first_meaningful_line(text),
            None => String::new(),
        }
    }
}

fn first_meaningful_line(text: &str) -> String {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

#[derive(Debug, Default)]
pub struct SubagentTimelines {
    lanes: Vec<AgentLane>,
    index: HashMap<String, usize>,
}

impl SubagentTimelines {
    #[must_use]
    pub fn lanes(&self) -> &[AgentLane] {
        &self.lanes
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }

    #[must_use]
    pub fn active_count(&self) -> usize {
        self.lanes
            .iter()
            .filter(|l| !l.status.is_terminal())
            .count()
    }

    pub fn clear(&mut self) {
        self.lanes.clear();
        self.index.clear();
    }

    fn lane_mut(
        &mut self,
        agent_id: &str,
        create_with_title: Option<(&str, Option<&str>)>,
    ) -> Option<&mut AgentLane> {
        if let Some(&idx) = self.index.get(agent_id) {
            return self.lanes.get_mut(idx);
        }
        let (title, parent) = create_with_title?;
        if self.lanes.len() >= MAX_LANES {
            if let Some(evict_idx) = self
                .lanes
                .iter()
                .enumerate()
                .filter(|(_, l)| l.status.is_terminal())
                .min_by_key(|(_, l)| l.updated_at)
                .map(|(i, _)| i)
            {
                let removed = self.lanes.remove(evict_idx);
                self.index.remove(&removed.agent_id);
                for slot in self.index.values_mut() {
                    if *slot > evict_idx {
                        *slot -= 1;
                    }
                }
            } else {
                return None;
            }
        }
        let lane = AgentLane::new(
            agent_id.to_string(),
            title.to_string(),
            parent.map(str::to_string),
        );
        self.lanes.push(lane);
        self.index.insert(agent_id.to_string(), self.lanes.len() - 1);
        self.lanes.last_mut()
    }

    pub fn record_spawn(&mut self, agent_id: &str, title: &str, parent_tool_use_id: Option<&str>) {
        if let Some(lane) = self.lane_mut(agent_id, Some((title, parent_tool_use_id))) {
            lane.status = LaneStatus::Starting;
            lane.updated_at = Instant::now();
            if !title.is_empty() {
                lane.title = title.to_string();
            }
        }
    }

    pub fn record_update(&mut self, agent_id: &str, status: LaneStatus, result: Option<&str>) {
        if let Some(lane) = self.lane_mut(agent_id, Some((agent_id, None))) {
            lane.status = status;
            lane.updated_at = Instant::now();
            if let Some(result) = result {
                if !result.trim().is_empty() {
                    let mut out = result.to_string();
                    cap_text(&mut out);
                    lane.final_output = Some(out);
                }
            }
        }
    }

    pub fn record_chunk(&mut self, agent_id: &str, kind: &str, text: &str) {
        let entry = match kind {
            "Thinking" | "thinking" => LaneEntry::Thinking(text.to_string()),
            "ToolCall" | "tool_call" => LaneEntry::ToolCall {
                summary: text.to_string(),
            },
            "ToolResult" | "tool_result" => {
                let lowered = text.trim_start().to_ascii_lowercase();
                LaneEntry::ToolResult {
                    preview: text.to_string(),
                    is_error: lowered.starts_with("error:") || lowered.starts_with("failed:"),
                }
            }
            "Status" | "status" => LaneEntry::Status(text.to_string()),
            _ => LaneEntry::Text(text.to_string()),
        };
        if let Some(lane) = self.lane_mut(agent_id, Some((agent_id, None))) {
            if lane.status == LaneStatus::Starting {
                lane.status = LaneStatus::Running;
            }
            lane.push_entry(entry);
        }
    }

    pub fn try_route_labeled_delta(&mut self, text: &str) -> bool {
        let Some((agent_id, task_id, rest)) = parse_lane_label(text) else {
            return false;
        };
        let known = self.index.contains_key(agent_id);
        if !known && !is_slug(agent_id) {
            return false;
        }
        let title = task_id.to_string();
        if !known {
            self.record_spawn(agent_id, &title, None);
        }
        let (kind, body) = if let Some(stripped) = rest.strip_prefix("[thinking] ") {
            ("Thinking", stripped)
        } else if let Some(stripped) = rest.strip_prefix("-> tool ") {
            ("ToolCall", stripped)
        } else if let Some(stripped) = rest.strip_prefix("<- ") {
            ("ToolResult", stripped)
        } else {
            ("Chunk", rest)
        };
        self.record_chunk(agent_id, kind, body);
        true
    }

    pub fn apply_agent_event(&mut self, event: &AgentEvent) -> bool {
        match event {
            AgentEvent::SubagentSpawn { id, description } => {
                self.record_spawn(id, description, None);
                true
            }
            AgentEvent::SubagentUpdate { id, status, result } => {
                let lane_status = match status {
                    SubagentStatus::StartingUp => LaneStatus::Starting,
                    SubagentStatus::Running => LaneStatus::Running,
                    SubagentStatus::Completed => LaneStatus::Completed,
                    SubagentStatus::Failed => LaneStatus::Failed,
                };
                self.record_update(id, lane_status, result.as_deref());
                true
            }
            AgentEvent::SubagentChildEvent {
                agent_id,
                block_kind,
                payload,
                ..
            } => {
                let text = payload
                    .get("text")
                    .or_else(|| payload.get("output"))
                    .or_else(|| payload.get("detail"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let action = payload.get("action").and_then(|v| v.as_str()).unwrap_or("");
                let combined = if action.is_empty() {
                    text.to_string()
                } else if text.is_empty() {
                    action.to_string()
                } else {
                    format!("{action}: {text}")
                };
                self.record_chunk(agent_id, block_kind, &combined);
                true
            }
            AgentEvent::StreamChunk(text) => self.try_route_labeled_delta(text),
            _ => false,
        }
    }
}

fn parse_lane_label(text: &str) -> Option<(&str, &str, &str)> {
    let inner_start = text.strip_prefix('[')?;
    let close = inner_start.find(']')?;
    let inner = &inner_start[..close];
    let (agent_id, task_id) = inner.split_once("::")?;
    if agent_id.is_empty() {
        return None;
    }
    let rest = inner_start[close + 1..].strip_prefix(' ').unwrap_or(&inner_start[close + 1..]);
    Some((agent_id, task_id, rest))
}

fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}
