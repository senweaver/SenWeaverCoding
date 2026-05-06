// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Shared bridge types for communication between UI frontends (TUI / GUI)
//! and the agent runtime. Both `tui` and `gui` features reference these types.

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum UserInput {

    Chat(String),

    SlashCommand { name: String, args: Vec<String> },

    ModeSwitch(String),

    Cancel,

    ReloadAgent,

    HotReloadProvider {
        provider: String,
        api_key: String,
        api_url: String,
        model: String,
    },

    ClearAndSeedHistory {
        messages: Vec<crate::providers::ChatMessage>,
    },

    ApprovalResponse { tool_id: String, approved: bool },

    ExecutePlan { plan_content: String },

    QuestionAnswer {
        question_id: String,
        prompt: String,
        selected: Vec<String>,
        selected_labels: Vec<String>,
    },

    QuestionAnswerBatch {
        answers: Vec<QuestionAnswerItem>,
    },

    ResumePlan { plan_content: String },

    CancelSubagent { id: String },

    PromoteToBackground { tool_id: String },

    KillBackgroundShell { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubagentStatus {
    StartingUp,
    Running,
    Completed,
    Failed,
}

impl std::fmt::Display for SubagentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StartingUp => write!(f, "Starting up"),
            Self::Running => write!(f, "Running"),
            Self::Completed => write!(f, "Completed"),
            Self::Failed => write!(f, "Failed"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TodoItem {
    pub id: String,
    pub content: String,

    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AgentEvent {

    AssistantMessage(String),

    StreamChunk(String),

    ToolUse {
        name: String,
        id: String,
        input: Option<String>,
    },

    ToolResult {
        id: String,
        output: String,
        success: bool,
    },

    Thinking,

    ThinkingChunk(String),

    Done,

    Error(String),

    CommandOutput(String),

    ModeChanged(String),

    ConfigWarning(String),

    FileEdit {
        path: String,
        additions: i32,
        deletions: i32,
        diff: Option<String>,
        edit_batch_id: Option<String>,
    },

    StatusUpdate { action: String, detail: String },

    TodoUpdate {
        todos: Vec<TodoItem>,
        completed: usize,
        total: usize,
    },

    BackgroundShell {
        id: String,
        command: String,
        elapsed_secs: u64,
        running: bool,
    },

    SubagentSpawn { id: String, description: String },

    SubagentUpdate {
        id: String,
        status: SubagentStatus,
        result: Option<String>,
    },

    PlanCreated {
        filename: String,
        title: String,
        overview: String,
        plan_content: String,
        todos: Vec<TodoItem>,
    },

    ApprovalRequest {
        tool_name: String,
        tool_id: String,
        args_summary: String,
    },

    QuestionAsked {
        question_id: String,
        prompt: String,
        options: Vec<QuestionOption>,
        allow_multiple: bool,
    },

    BackgroundShellChunk {
        id: String,
        stream: BgStream,
        line: String,
    },

    SubagentChildEvent {
        agent_id: String,
        task_id: String,
        block_kind: String,
        payload: serde_json::Value,
    },

    PlanReady {

        filename: String,

        path: String,
    },

    QuestionAnswered {

        items: Vec<QuestionAnswerItem>,
    },
}

#[derive(Debug, Clone)]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct QuestionAnswerItem {
    pub question_id: String,
    pub prompt: String,
    pub selected: Vec<String>,
    pub selected_labels: Vec<String>,
}
