// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use crate::config::domain::evolution::{
    EvolutionConfig, EvolutionExportConfig, EvolutionExportFormat, EvolutionSignalWeights,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnClass {
    Main,
    Side,
}

impl TurnClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Side => "side",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalSource {
    UserThumbs,
    NextState,
    Tool,
    Verification,
    Cost,
}

impl SignalSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UserThumbs => "thumbs",
            Self::NextState => "next_state",
            Self::Tool => "tool",
            Self::Verification => "verification",
            Self::Cost => "cost",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalScore {
    pub source: SignalSource,
    pub score: f32,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Reward {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbs: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_state: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f32>,
    #[serde(default)]
    pub final_score: f32,
    #[serde(default)]
    pub loss_mask: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutcome {
    pub name: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_excerpt: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatMessageView {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCallView {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicBlockView {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicMessageView {
    pub role: String,
    pub content: Vec<AnthropicBlockView>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResponseView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NextStateView {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostView {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnRecord {
    pub id: String,
    pub session_id: String,
    pub turn_idx: u64,
    pub turn_class: TurnClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coding_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openai_messages: Vec<ChatMessageView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anthropic_messages: Vec<AnthropicMessageView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic_system: Option<String>,
    #[serde(default)]
    pub response: ResponseView,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_outcomes: Vec<ToolOutcome>,
    #[serde(default)]
    pub reward: Reward,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_state: Option<NextStateView>,
    #[serde(default)]
    pub cost: CostView,
    pub ts: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_ts: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aborted: Option<String>,
}

impl TurnRecord {
    pub fn new(session_id: impl Into<String>, turn_idx: u64, turn_class: TurnClass) -> Self {
        Self {
            id: format!("turn_{}", uuid::Uuid::new_v4().simple()),
            session_id: session_id.into(),
            turn_idx,
            turn_class,
            coding_mode: None,
            provider: None,
            model: None,
            openai_messages: Vec::new(),
            anthropic_messages: Vec::new(),
            anthropic_system: None,
            response: ResponseView::default(),
            tool_outcomes: Vec::new(),
            reward: Reward::default(),
            next_state: None,
            cost: CostView::default(),
            ts: Utc::now(),
            completed_ts: None,
            aborted: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub id: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coding_mode: Option<String>,
    #[serde(default)]
    pub source_turn_ids: Vec<String>,
    #[serde(default)]
    pub hits: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playbook {
    pub id: String,
    pub title: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coding_mode: Option<String>,
    #[serde(default)]
    pub hits: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbVote {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub score: i8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRecord {
    pub id: String,
    pub format: EvolutionExportFormat,
    pub path: String,
    pub sample_count: u64,
    pub size_bytes: u64,
    pub md5: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_window_start: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_window_end: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudTargetKind {
    OpenaiFiles,
    HuggingfaceDataset,
    RlDatasetServer,
    Tinker,
    Fireworks,
    Webhook,
}

impl CloudTargetKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenaiFiles => "openai_files",
            Self::HuggingfaceDataset => "huggingface_dataset",
            Self::RlDatasetServer => "rl_dataset_server",
            Self::Tinker => "tinker",
            Self::Fireworks => "fireworks",
            Self::Webhook => "webhook",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "openai_files" => Some(Self::OpenaiFiles),
            "huggingface_dataset" => Some(Self::HuggingfaceDataset),
            "rl_dataset_server" => Some(Self::RlDatasetServer),
            "tinker" => Some(Self::Tinker),
            "fireworks" => Some(Self::Fireworks),
            "webhook" => Some(Self::Webhook),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudTarget {
    pub id: String,
    pub name: String,
    pub kind: CloudTargetKind,
    pub endpoint: String,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<String>,
    #[serde(default)]
    pub default_format: EvolutionExportFormat,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub auto_push: bool,
    #[serde(default)]
    pub auto_push_min_samples: u32,
    #[serde(default)]
    pub auto_push_min_interval_hours: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_pushed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushReceipt {
    pub id: String,
    pub export_id: String,
    pub target_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_excerpt: Option<String>,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub payload: serde_json::Value,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistenceStatus {
    pub persist_training_data: bool,
    pub turns_file_size: u64,
    pub turns_count: u64,
    pub events_file_size: u64,
    pub exports_total_bytes: u64,
    pub exports_count: u64,
    pub push_receipts_count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PurgeReport {
    pub turns: u64,
    pub exports: u64,
    pub push_history: u64,
    pub events: u64,
    pub freed_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PurgeScope {
    Turns,
    Exports,
    PushHistory,
    Events,
    All,
}

impl PurgeScope {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "turns" => Some(Self::Turns),
            "exports" => Some(Self::Exports),
            "push_history" => Some(Self::PushHistory),
            "events" => Some(Self::Events),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

fn default_true() -> bool {
    true
}
