// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod agent_bridge;
pub mod agents_panel;

pub mod chat_render_cache;
pub mod chat_view;

pub mod edit_batch_registry;

pub mod diff_review;

pub mod file_viewer;

pub mod inline_edit_modal;

pub mod ghost_overlay;

pub mod chat_message_reconciler;

pub mod event_loop;

pub mod panels;
pub mod syntax_highlight;
pub mod theme;

use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};

use crate::bootstrap::BootstrapState;
use crate::config::Config;
use crate::keybindings::parser::ParsedKey;
use crate::keybindings::resolver::KeybindingResolver;
use crate::keybindings::schema::{KeyAction, KeyModifier};
use crate::services::ServiceContainer;
use crate::vim_mode::transitions::{
    process_key_with_buffer as vim_process_key, process_text_object_key,
};
use crate::vim_mode::types::VimState;

fn try_get_bootstrap_state() -> Option<&'static BootstrapState> {
    crate::bootstrap::try_get_state()
}

fn try_get_services() -> Option<&'static ServiceContainer> {
    crate::services::try_get_services()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Dashboard,
    Chat,
    Memory,
    Channels,
    Tasks,
    Tools,
    Commands,
    Cost,

    Agents,
    Events,
    Logs,

    Diff,

    Files,
}

impl Tab {
    fn all() -> &'static [Tab] {
        &[
            Tab::Dashboard,
            Tab::Chat,
            Tab::Memory,
            Tab::Channels,
            Tab::Tasks,
            Tab::Tools,
            Tab::Commands,
            Tab::Cost,
            Tab::Agents,
            Tab::Events,
            Tab::Logs,
            Tab::Diff,
            Tab::Files,
        ]
    }

    fn title(&self) -> &'static str {
        match self {
            Tab::Dashboard => "Dashboard",
            Tab::Chat => "Chat",
            Tab::Memory => "Memory",
            Tab::Channels => "Channels",
            Tab::Tasks => "Tasks",
            Tab::Tools => "Tools",
            Tab::Commands => "Commands",
            Tab::Cost => "Cost",
            Tab::Agents => "Agents",
            Tab::Events => "Events",
            Tab::Logs => "Logs",
            Tab::Diff => "Diff",
            Tab::Files => "Files",
        }
    }

    fn index(&self) -> usize {
        match self {
            Tab::Dashboard => 0,
            Tab::Chat => 1,
            Tab::Memory => 2,
            Tab::Channels => 3,
            Tab::Tasks => 4,
            Tab::Tools => 5,
            Tab::Commands => 6,
            Tab::Cost => 7,
            Tab::Agents => 8,
            Tab::Events => 9,
            Tab::Logs => 10,
            Tab::Diff => 11,
            Tab::Files => 12,
        }
    }

    fn from_index(i: usize) -> Self {
        match i {
            0 => Tab::Dashboard,
            1 => Tab::Chat,
            2 => Tab::Memory,
            3 => Tab::Channels,
            4 => Tab::Tasks,
            5 => Tab::Tools,
            6 => Tab::Commands,
            7 => Tab::Cost,
            8 => Tab::Agents,
            9 => Tab::Events,
            10 => Tab::Logs,
            11 => Tab::Diff,
            12 => Tab::Files,
            _ => Tab::Dashboard,
        }
    }
}

pub struct App {
    pub active_tab: Tab,
    pub should_quit: bool,
    pub show_help: bool,
    pub config: Config,
    pub chat_input: String,
    pub chat_messages: Vec<ChatMessage>,
    pub log_entries: Vec<String>,
    pub event_entries: Vec<String>,
    pub memory_entries: Vec<MemoryEntry>,
    pub memory_list_state: ListState,
    pub status_info: StatusInfo,
    pub tick_count: u64,
    pub task_entries: Vec<TaskEntry>,
    pub task_list_state: ListState,
    pub tool_entries: Vec<ToolEntry>,
    pub tool_list_state: ListState,
    pub command_entries: Vec<CommandEntry>,
    pub command_list_state: ListState,
    pub cost_details: CostDetails,
    pub bridge: agent_bridge::AgentBridge,
    pub streaming_buffer: String,

    pub chat_cursor_pos: usize,

    pub chat_scroll_offset: usize,

    pub vim_enabled: bool,

    pub vim_state: VimState,

    pub keybinding_resolver: KeybindingResolver,

    pub vim_clipboard: String,

    pub undo_stack: Vec<String>,

    pub redo_stack: Vec<String>,

    pub command_palette_open: bool,

    pub command_palette_filter: String,

    pub command_palette_selected: usize,

    pub budget_view: crate::observability::views::BudgetView,

    pub provider_health_view: crate::observability::views::ProviderHealthView,

    pub dirty: bool,

    pub pending_delta_started_at: Option<std::time::Instant>,

    pub partial_redraw_pending: bool,

    pub chat_render_cache: chat_render_cache::ChatRenderCache,

    pub edit_batch_registry: edit_batch_registry::EditBatchRegistry,

    pub diff_review_state: diff_review::DiffReviewState,

    pub file_viewer_state: file_viewer::FileViewerState,

    pub workspace_root: std::path::PathBuf,

    pub inline_edit_modal: inline_edit_modal::InlineEditModal,

    pub pending_inline_path: Option<std::path::PathBuf>,

    pub chat_reconciler: chat_message_reconciler::ChatMessageReconciler,

    pub inline_edit_outcome_tx: tokio::sync::mpsc::UnboundedSender<RunnerOutcomeMessage>,
    pub inline_edit_outcome_rx: tokio::sync::mpsc::UnboundedReceiver<RunnerOutcomeMessage>,
}

#[derive(Debug)]
pub enum RunnerOutcomeMessage {
    Success(inline_edit_modal::RunnerSubmitOutcome),
    Failure { path: std::path::PathBuf, error: String },
}

pub struct KeyShortcut {
    pub key: &'static str,
    pub description: &'static str,
}

impl App {

    pub fn get_shortcuts() -> Vec<KeyShortcut> {
        vec![
            KeyShortcut {
                key: "Ctrl+Q / Ctrl+C",
                description: "Quit application",
            },
            KeyShortcut {
                key: "Tab / Shift+Tab",
                description: "Next / Previous tab",
            },
            KeyShortcut {
                key: "F1-F10",
                description: "Switch to specific tab",
            },
            KeyShortcut {
                key: "?",
                description: "Toggle this help panel",
            },
            KeyShortcut {
                key: "j / k",
                description: "Navigate up / down",
            },
            KeyShortcut {
                key: "Enter",
                description: "Send message / Execute",
            },
            KeyShortcut {
                key: "Esc",
                description: "Cancel current operation",
            },
            KeyShortcut {
                key: "Ctrl+L",
                description: "Clear chat",
            },
        ]
    }
}

#[derive(Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: String,

    pub metadata: std::collections::HashMap<String, serde_json::Value>,

    #[doc(hidden)]
    pub content_hash: u64,

    #[doc(hidden)]
    pub highlighted_cache: once_cell::sync::OnceCell<std::sync::Arc<Vec<Line<'static>>>>,
}

impl Clone for ChatMessage {
    fn clone(&self) -> Self {

        Self {
            role: self.role.clone(),
            content: self.content.clone(),
            timestamp: self.timestamp.clone(),
            metadata: self.metadata.clone(),
            content_hash: self.content_hash,
            highlighted_cache: once_cell::sync::OnceCell::new(),
        }
    }
}

impl ChatMessage {
    fn with_role_now(role: &str, content: String) -> Self {
        Self::from_parts(
            role,
            content,
            chrono::Local::now().format("%H:%M:%S").to_string(),
        )
    }

    pub fn from_parts(role: &str, content: String, timestamp: String) -> Self {
        let content_hash = chat_render_cache::fingerprint(&content);
        Self {
            role: role.to_string(),
            content,
            timestamp,
            metadata: Default::default(),
            content_hash,
            highlighted_cache: once_cell::sync::OnceCell::new(),
        }
    }

    pub fn mark_content_dirty(&mut self) {
        self.content_hash = chat_render_cache::fingerprint(&self.content);
        chat_render_cache::invalidate_message_cache(&mut self.highlighted_cache);
    }
}

impl crate::session::ChatViewSink for Vec<ChatMessage> {
    fn push_user(&mut self, text: &str) {
        self.push(ChatMessage::with_role_now("user", text.to_string()));
    }

    fn append_assistant_delta(&mut self, text: &str) {
        if let Some(last) = self.last_mut() {
            if last.role == "assistant" {
                last.content.push_str(text);
                last.mark_content_dirty();
                return;
            }
        }
        self.push(ChatMessage::with_role_now("assistant", text.to_string()));
    }

    fn close_assistant_turn(&mut self, output: &str) {
        if output.is_empty() {
            return;
        }
        let already_streamed = self
            .last()
            .map(|m| m.role == "assistant" && !m.content.is_empty())
            .unwrap_or(false);
        if !already_streamed {
            self.push(ChatMessage::with_role_now("assistant", output.to_string()));
        }
    }

    fn push_tool_call(&mut self, tool_name: &str, arguments: &serde_json::Value) {
        let preview = serde_json::to_string(arguments)
            .unwrap_or_default()
            .chars()
            .take(120)
            .collect::<String>();
        self.push(ChatMessage::with_role_now(
            "tool",
            format!("{tool_name}({preview})"),
        ));
    }

    fn push_tool_result(&mut self, output: &str, is_error: bool) {
        self.push(ChatMessage::with_role_now(
            if is_error { "tool_error" } else { "tool_result" },
            output.to_string(),
        ));
    }

    fn push_error(&mut self, message: &str) {
        self.push(ChatMessage::with_role_now("error", message.to_string()));
    }

    fn push_system(&mut self, message: &str) {
        self.push(ChatMessage::with_role_now("system", message.to_string()));
    }
}

#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub key: String,
    pub category: String,
    pub preview: String,
}

#[derive(Debug, Clone)]
pub struct StatusInfo {
    pub version: String,
    pub provider: String,
    pub model: String,
    pub autonomy: String,
    pub memory_backend: String,
    pub channels_active: usize,
    pub channels_total: usize,
    pub uptime_secs: u64,
    pub cost_today: f64,
    pub cost_month: f64,
}

#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub id: String,
    pub task_type: String,
    pub status: String,
    pub description: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ToolEntry {
    pub name: String,
    pub category: String,
    pub call_count: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct CommandEntry {
    pub name: String,
    pub description: String,
    pub category: String,
    pub usage: String,
}

#[derive(Debug, Clone, Default)]
pub struct CostDetails {
    pub session_cost_usd: f64,
    pub today_cost_usd: f64,
    pub month_cost_usd: f64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_write_tokens: u64,
    pub total_requests: u64,
    pub model_costs: Vec<ModelCostEntry>,
}

#[derive(Debug, Clone)]
pub struct ModelCostEntry {
    pub model_name: String,
    pub cost_usd: f64,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl App {

    pub fn new(config: Config, bridge: agent_bridge::AgentBridge) -> Self {
        let provider = config
            .default_provider
            .clone()
            .unwrap_or_else(|| "none".into());
        let model = config
            .default_model
            .clone()
            .unwrap_or_else(|| "none".into());
        let (inline_edit_outcome_tx, inline_edit_outcome_rx) =
            tokio::sync::mpsc::unbounded_channel::<RunnerOutcomeMessage>();

        Self {
            active_tab: Tab::Dashboard,
            should_quit: false,
            show_help: false,
            config,
            chat_input: String::new(),
            chat_messages: Vec::new(),
            log_entries: vec![format!(
                "[{}] SenWeaverCoding TUI started",
                chrono::Local::now().format("%H:%M:%S")
            )],
            event_entries: Vec::new(),
            memory_entries: Vec::new(),
            memory_list_state: ListState::default(),
            status_info: StatusInfo {
                version: env!("CARGO_PKG_VERSION").to_string(),
                provider,
                model,
                autonomy: "full".to_string(),
                memory_backend: "sqlite".to_string(),
                channels_active: 0,
                channels_total: 0,
                uptime_secs: 0,
                cost_today: 0.0,
                cost_month: 0.0,
            },
            tick_count: 0,
            task_entries: Vec::new(),
            task_list_state: ListState::default(),
            tool_entries: Vec::new(),
            tool_list_state: ListState::default(),
            command_entries: Vec::new(),
            command_list_state: ListState::default(),
            cost_details: CostDetails::default(),
            bridge,
            streaming_buffer: String::new(),
            chat_cursor_pos: 0,
            chat_scroll_offset: 0,
            vim_enabled: false,
            vim_state: VimState::default(),

            keybinding_resolver: {
                let resolver = crate::keybindings::install_global_resolver_from_disk();
                (*resolver).clone()
            },
            vim_clipboard: String::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            command_palette_open: false,
            command_palette_filter: String::new(),
            command_palette_selected: 0,
            budget_view: crate::observability::views::BudgetView::default(),
            provider_health_view: crate::observability::views::ProviderHealthView::default(),

            dirty: true,
            pending_delta_started_at: None,
            partial_redraw_pending: false,
            chat_render_cache: chat_render_cache::ChatRenderCache::new(),
            edit_batch_registry: edit_batch_registry::EditBatchRegistry::default(),
            diff_review_state: diff_review::DiffReviewState::new(),
            file_viewer_state: file_viewer::FileViewerState::new(),
            workspace_root: std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from(".")),
            inline_edit_modal: inline_edit_modal::InlineEditModal::default(),
            pending_inline_path: None,
            chat_reconciler: chat_message_reconciler::ChatMessageReconciler::default(),
            inline_edit_outcome_tx,
            inline_edit_outcome_rx,
        }
    }

    #[inline]
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn handle_key(&mut self, key: event::KeyEvent) {

        if self.command_palette_open {
            self.handle_command_palette_key(key);
            return;
        }

        if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.command_palette_open = true;
            self.command_palette_filter.clear();
            self.command_palette_selected = 0;
            return;
        }

        let parsed = Self::event_to_parsed_key(&key);
        if let Some(parsed) = &parsed {
            if let Some(action) = self.keybinding_resolver.resolve(parsed).cloned() {
                match action {
                    KeyAction::Exit | KeyAction::Interrupt => {
                        self.should_quit = true;
                        return;
                    }
                    KeyAction::Help => {
                        self.show_help = !self.show_help;
                        return;
                    }
                    KeyAction::Clear => {
                        self.chat_messages.clear();
                        self.chat_reconciler.reset();
                        return;
                    }
                    KeyAction::ToggleVim => {
                        self.vim_enabled = !self.vim_enabled;
                        self.vim_state = VimState::default();
                        return;
                    }
                    KeyAction::Submit => {
                        self.send_chat();
                        return;
                    }
                    KeyAction::NewLine => {
                        self.chat_input.insert(self.chat_cursor_pos, '\n');
                        self.chat_cursor_pos += 1;
                        return;
                    }
                    KeyAction::Cancel => {
                        if self.bridge.is_busy {
                            let _ = self.bridge.send(agent_bridge::UserInput::Cancel);
                        }
                        return;
                    }
                    KeyAction::HistoryPrev => {

                        self.chat_scroll_offset = (self.chat_scroll_offset + 1)
                            .min(self.chat_messages.len().saturating_sub(1));
                        return;
                    }
                    KeyAction::HistoryNext => {
                        self.chat_scroll_offset = self.chat_scroll_offset.saturating_sub(1);
                        return;
                    }
                    KeyAction::AutoMode
                    | KeyAction::PlanMode
                    | KeyAction::Compact
                    | KeyAction::TabComplete
                    | KeyAction::VoiceToggle
                    | KeyAction::Custom(_) => {}
                }
            }
        }

        match key.code {
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.chat_messages.clear();
                self.chat_reconciler.reset();
            }
            KeyCode::Tab => {
                let next = (self.active_tab.index() + 1) % Tab::all().len();
                self.active_tab = Tab::from_index(next);
            }
            KeyCode::BackTab => {
                let len = Tab::all().len();
                let prev = (self.active_tab.index() + len - 1) % len;
                self.active_tab = Tab::from_index(prev);
            }
            KeyCode::F(n) if (1..=10).contains(&n) => {
                self.active_tab = Tab::from_index((n - 1) as usize);
            }

            KeyCode::Char('1') if key.modifiers.contains(KeyModifiers::ALT) => {
                self.active_tab = Tab::Logs;
            }
            KeyCode::Char('2') if key.modifiers.contains(KeyModifiers::ALT) => {
                self.active_tab = Tab::Diff;
                crate::observability::tui_metrics::incr_tui_diff_review_opened();
            }
            KeyCode::Char('3') if key.modifiers.contains(KeyModifiers::ALT) => {
                self.active_tab = Tab::Files;
                crate::observability::tui_metrics::incr_tui_file_viewer_opened();
            }
            _ => match self.active_tab {
                Tab::Chat => self.handle_chat_key(key),
                Tab::Memory => self.handle_memory_key(key),
                Tab::Tasks => self.handle_list_key(key, ListTarget::Tasks),
                Tab::Tools => self.handle_list_key(key, ListTarget::Tools),
                Tab::Commands => self.handle_list_key(key, ListTarget::Commands),
                Tab::Diff => self.handle_diff_review_key(key),
                Tab::Files => self.handle_file_viewer_key(key),
                _ => {}
            },
        }
    }

    pub fn collect_open_files(
        &self,
    ) -> Vec<(std::path::PathBuf, Option<chrono::DateTime<chrono::Utc>>)> {
        let Some(actor) = self.bridge.session_actor_slot.get() else {
            return Vec::new();
        };
        let snapshot = actor.snapshot();
        let mut rows: Vec<_> = snapshot
            .open_files
            .iter()
            .map(|(p, meta)| (p.clone(), meta.last_read_at))
            .collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1));
        rows
    }

    fn handle_file_viewer_key(&mut self, key: event::KeyEvent) {
        let action = file_viewer::handle_key(
            &mut self.file_viewer_state,
            &self.workspace_root,
            key,
        );
        match action {
            file_viewer::FileViewerAction::Noop => {}
            file_viewer::FileViewerAction::Toast(msg) => {
                self.file_viewer_state.status = Some(msg);
            }
            file_viewer::FileViewerAction::Open { path } => {
                self.file_viewer_state.status =
                    Some(format!("opened {}", path.display()));
                if let Some(actor) = self.bridge.session_actor_slot.get() {
                    crate::observability::tui_metrics::incr_tui_file_viewer_file_open();
                    let kind = crate::session::event::SessionEventKind::OpenFileMarked {
                        path: path.display().to_string(),
                        cursor: None,
                        source: "tui".into(),
                    };
                    let _ = actor.apply(&crate::session::event::SessionEvent::new(kind));
                }
            }
        }
    }

    fn handle_diff_review_key(&mut self, key: event::KeyEvent) {
        let action = diff_review::handle_key(
            &mut self.diff_review_state,
            &self.edit_batch_registry,
            key,
        );
        match action {
            diff_review::DiffReviewAction::Noop => {}
            diff_review::DiffReviewAction::Toast(msg) => {
                self.diff_review_state.toast = Some(msg);
            }
            diff_review::DiffReviewAction::MarkApplied {
                entry_id,
                hunk_index,
            } => {
                diff_review::mark_applied(
                    &mut self.edit_batch_registry,
                    entry_id,
                    hunk_index,
                );
                self.diff_review_state.toast = Some("marked applied".into());
            }
            diff_review::DiffReviewAction::RevertFile { entry_id } => {
                let entry = self
                    .edit_batch_registry
                    .get_mut_by_id(entry_id)
                    .map(|e| e.clone());
                if let Some(entry) = entry {
                    let workspace = std::env::current_dir()
                        .unwrap_or_else(|_| std::path::PathBuf::from("."));
                    match diff_review::revert_file_entry(&entry, &workspace) {
                        Ok(summary) => {
                            diff_review::mark_reverted(
                                &mut self.edit_batch_registry,
                                entry_id,
                                None,
                            );
                            self.diff_review_state.toast = Some(summary);
                        }
                        Err(e) => {
                            self.diff_review_state.toast =
                                Some(format!("revert failed: {e}"));
                        }
                    }
                }
            }
            diff_review::DiffReviewAction::RevertHunk {
                entry_id,
                hunk_index,
            } => {
                let entry = self
                    .edit_batch_registry
                    .get_mut_by_id(entry_id)
                    .map(|e| e.clone());
                if let Some(entry) = entry {
                    let workspace = std::env::current_dir()
                        .unwrap_or_else(|_| std::path::PathBuf::from("."));

                    let entry_clone = entry.clone();
                    let ws_clone = workspace.clone();
                    let join = std::thread::spawn(move || {
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|e| anyhow::anyhow!("runtime build: {e}"))?;
                        rt.block_on(diff_review::revert_single_hunk(
                            &entry_clone,
                            hunk_index,
                            &ws_clone,
                        ))
                    });
                    match join.join() {
                        Ok(Ok(new_batch_id)) => {
                            diff_review::mark_reverted(
                                &mut self.edit_batch_registry,
                                entry_id,
                                Some(hunk_index),
                            );
                            self.diff_review_state.toast = Some(format!(
                                "hunk reverted via new batch {new_batch_id}"
                            ));
                        }
                        Ok(Err(e)) => {
                            self.diff_review_state.toast =
                                Some(format!("hunk revert failed: {e}"));
                        }
                        Err(_) => {
                            self.diff_review_state.toast =
                                Some("hunk revert worker panicked".into());
                        }
                    }
                }
            }
        }
    }

    fn event_to_parsed_key(key: &event::KeyEvent) -> Option<ParsedKey> {
        let key_str = match key.code {
            KeyCode::Enter => "enter".to_string(),
            KeyCode::Esc => "escape".to_string(),
            KeyCode::Tab => "tab".to_string(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::F(n) => format!("f{n}"),
            _ => return None,
        };
        let mut modifiers = Vec::new();
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            modifiers.push(KeyModifier::Ctrl);
        }
        if key.modifiers.contains(KeyModifiers::ALT) {
            modifiers.push(KeyModifier::Alt);
        }
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            modifiers.push(KeyModifier::Shift);
        }
        Some(ParsedKey {
            key: key_str,
            modifiers,
        })
    }

    fn handle_chat_key(&mut self, key: event::KeyEvent) {

        if self.inline_edit_modal.is_open {
            let action = inline_edit_modal::handle_key(&mut self.inline_edit_modal, key);
            match action {
                inline_edit_modal::ModalAction::Noop | inline_edit_modal::ModalAction::Close => {}
                inline_edit_modal::ModalAction::Submit { path, instruction } => {
                    crate::observability::tui_metrics::incr_tui_inline_edit_triggered();
                    self.pending_inline_path = Some(path.clone());

                    let runner = crate::inline_edit::service::default_runner(&self.config);
                    if let Some(runner) = runner {
                        let tx = self.inline_edit_outcome_tx.clone();
                        let target_path = path.clone();
                        let instruction_clone = instruction.clone();
                        tokio::spawn(async move {
                            let outcome = inline_edit_modal::run_through_runner(
                                runner.as_ref(),
                                target_path.clone(),
                                instruction_clone,
                            )
                            .await;
                            let msg = match outcome {
                                Ok(success) => RunnerOutcomeMessage::Success(success),
                                Err(err) => RunnerOutcomeMessage::Failure {
                                    path: target_path,
                                    error: err.to_string(),
                                },
                            };
                            let _ = tx.send(msg);
                        });
                        self.inline_edit_modal.status =
                            Some("running inline-edit runner...".into());
                    } else {
                        let prompt = inline_edit_modal::build_agent_prompt(&path, &instruction);
                        let sent = self
                            .bridge
                            .send(agent_bridge::UserInput::Chat(prompt));
                        if sent {
                            self.inline_edit_modal.status =
                                Some("dispatched to agent (no runner configured)".into());
                            crate::observability::tui_metrics::incr_tui_inline_edit_success();
                        } else {
                            self.inline_edit_modal.status =
                                Some("agent busy — retry after it finishes".into());
                            crate::observability::tui_metrics::incr_tui_inline_edit_failed();
                        }
                        self.inline_edit_modal.close();
                    }
                }
            }
            self.mark_dirty();
            return;
        }

        if key.code == KeyCode::Char('k') && key.modifiers.contains(KeyModifiers::CONTROL) {
            let seed_path = self
                .edit_batch_registry
                .entries_newest_first()
                .next()
                .map(|e| std::path::PathBuf::from(&e.path));
            self.inline_edit_modal.open_with_path(seed_path);
            self.mark_dirty();
            return;
        }

        if let KeyCode::Enter = key.code {
            if self.chat_input.trim() == "/vim" {
                self.vim_enabled = !self.vim_enabled;
                self.vim_state = VimState::default();
                let status = if self.vim_enabled {
                    "enabled"
                } else {
                    "disabled"
                };
                self.chat_messages.push(ChatMessage::with_role_now(
                    "system",
                    format!("Vim mode {status}"),
                ));
                self.chat_input.clear();
                self.chat_cursor_pos = 0;
                return;
            }
        }

        if self.vim_enabled {
            let key_char = match key.code {
                KeyCode::Char(c) => c,
                KeyCode::Esc => '\x1b',
                KeyCode::Enter => '\n',
                KeyCode::Backspace => '\x08',
                _ => {

                    self.handle_chat_key_normal(key);
                    return;
                }
            };
            let modifiers: Vec<&str> = if key.modifiers.contains(KeyModifiers::CONTROL) {
                vec!["ctrl"]
            } else if key.modifiers.contains(KeyModifiers::SHIFT) {
                vec!["shift"]
            } else {
                vec![]
            };

            if self.vim_state.pending_operator.is_some()
                && (self.vim_state.command_buffer == "i" || self.vim_state.command_buffer == "a")
            {
                let Some(op) = self.vim_state.pending_operator else {
                    tracing::warn!("vim pending_operator went missing despite is_some() guard");
                    return;
                };
                let Some(ia) = self.vim_state.command_buffer.chars().next() else {
                    tracing::warn!(
                        "vim command_buffer became empty despite i/a guard; ignoring key"
                    );
                    return;
                };
                let action = process_text_object_key(
                    &mut self.vim_state,
                    op,
                    ia,
                    key_char,
                    &self.chat_input,
                );
                self.apply_vim_action(action);
                return;
            }
            let action =
                vim_process_key(&mut self.vim_state, key_char, &modifiers, &self.chat_input);
            self.apply_vim_action(action);
            return;
        }

        self.handle_chat_key_normal(key);
    }

    fn push_undo_snapshot(&mut self) {
        if self.undo_stack.last().map(|s| s.as_str()) != Some(&self.chat_input) {
            self.undo_stack.push(self.chat_input.clone());
            if self.undo_stack.len() > 100 {
                self.undo_stack.remove(0);
            }
            self.redo_stack.clear();
        }
    }

    fn apply_vim_action(&mut self, action: crate::vim_mode::types::VimAction) {
        use crate::vim_mode::types::{VimAction, VimMode};
        match action {
            VimAction::InsertChar(c) => {
                self.push_undo_snapshot();
                self.chat_input.insert(self.chat_cursor_pos, c);
                self.chat_cursor_pos += c.len_utf8();
            }
            VimAction::CursorMove(logical_pos) => {

                let byte_pos = self
                    .chat_input
                    .char_indices()
                    .nth(logical_pos)
                    .map(|(b, _)| b)
                    .unwrap_or(self.chat_input.len());
                self.chat_cursor_pos = byte_pos;

                self.vim_state.cursor_pos = logical_pos;
            }
            VimAction::DeleteRange(start_char, end_char) => {
                let char_indices: Vec<(usize, char)> = self.chat_input.char_indices().collect();
                let s = char_indices
                    .get(start_char)
                    .map(|(b, _)| *b)
                    .unwrap_or(self.chat_input.len());
                let e = char_indices
                    .get(end_char)
                    .map(|(b, _)| *b)
                    .unwrap_or(self.chat_input.len());
                if s < e {
                    self.push_undo_snapshot();
                    self.chat_input.drain(s..e);
                    self.chat_cursor_pos = s;
                    self.vim_state.cursor_pos = start_char;
                }
            }
            VimAction::ModeChange(mode) => {

                match mode {
                    VimMode::Insert => {

                        let char_count = self.chat_input.chars().count();
                        if self.vim_state.cursor_pos >= char_count {
                            self.chat_cursor_pos = self.chat_input.len();
                        }
                    }
                    _ => {}
                }
            }
            VimAction::Submit => {
                self.send_chat();
            }
            VimAction::Cancel => {
                if self.bridge.is_busy {
                    let _ = self.bridge.send(agent_bridge::UserInput::Cancel);
                }
            }
            VimAction::YankRange(start_char, end_char) => {
                let chars: Vec<char> = self.chat_input.chars().collect();
                let s = start_char.min(chars.len());
                let e = end_char.min(chars.len());
                if s < e {
                    self.vim_clipboard = chars[s..e].iter().collect();
                }
            }
            VimAction::PasteAfter => {
                if !self.vim_clipboard.is_empty() {
                    self.push_undo_snapshot();
                    let text = self.vim_clipboard.clone();
                    self.chat_input.insert_str(self.chat_cursor_pos, &text);
                    self.chat_cursor_pos += text.len();
                }
            }
            VimAction::PasteBefore => {
                if !self.vim_clipboard.is_empty() {
                    self.push_undo_snapshot();
                    let text = self.vim_clipboard.clone();
                    self.chat_input.insert_str(self.chat_cursor_pos, &text);
                }
            }
            VimAction::Undo => {
                if let Some(prev) = self.undo_stack.pop() {
                    self.redo_stack.push(self.chat_input.clone());
                    self.chat_input = prev;
                    self.chat_cursor_pos = self.chat_input.len();
                    self.vim_state.cursor_pos = self.chat_input.chars().count();
                }
            }
            VimAction::Redo => {
                if let Some(next) = self.redo_stack.pop() {
                    self.undo_stack.push(self.chat_input.clone());
                    self.chat_input = next;
                    self.chat_cursor_pos = self.chat_input.len();
                    self.vim_state.cursor_pos = self.chat_input.chars().count();
                }
            }
            VimAction::NoOp => {}
        }
    }

    fn command_palette_commands() -> Vec<(&'static str, &'static str)> {
        vec![
            ("Switch to Dashboard", "tab:dashboard"),
            ("Switch to Chat", "tab:chat"),
            ("Switch to Memory", "tab:memory"),
            ("Switch to Channels", "tab:channels"),
            ("Switch to Tasks", "tab:tasks"),
            ("Switch to Tools", "tab:tools"),
            ("Switch to Commands", "tab:commands"),
            ("Switch to Cost", "tab:cost"),
            ("Switch to Agents", "tab:agents"),
            ("Switch to Events", "tab:events"),
            ("Switch to Logs", "tab:logs"),
            ("Toggle Vim Mode", "action:vim"),
            ("Clear Chat", "action:clear"),
            ("Toggle Help", "action:help"),
            ("Quit", "action:quit"),
        ]
    }

    fn filtered_palette_commands(&self) -> Vec<(&'static str, &'static str)> {
        let filter = self.command_palette_filter.to_lowercase();
        Self::command_palette_commands()
            .into_iter()
            .filter(|(label, _)| filter.is_empty() || label.to_lowercase().contains(&filter))
            .collect()
    }

    fn handle_command_palette_key(&mut self, key: event::KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.command_palette_open = false;
            }
            KeyCode::Enter => {
                let cmds = self.filtered_palette_commands();
                if let Some((_, action)) = cmds.get(self.command_palette_selected) {
                    self.execute_palette_action(action);
                }
                self.command_palette_open = false;
            }
            KeyCode::Up => {
                self.command_palette_selected = self.command_palette_selected.saturating_sub(1);
            }
            KeyCode::Down => {
                let max = self.filtered_palette_commands().len().saturating_sub(1);
                self.command_palette_selected = (self.command_palette_selected + 1).min(max);
            }
            KeyCode::Char(c) => {
                self.command_palette_filter.push(c);
                self.command_palette_selected = 0;
            }
            KeyCode::Backspace => {
                self.command_palette_filter.pop();
                self.command_palette_selected = 0;
            }
            _ => {}
        }
    }

    fn execute_palette_action(&mut self, action: &str) {
        match action {
            "tab:dashboard" => self.active_tab = Tab::Dashboard,
            "tab:chat" => self.active_tab = Tab::Chat,
            "tab:memory" => self.active_tab = Tab::Memory,
            "tab:channels" => self.active_tab = Tab::Channels,
            "tab:tasks" => self.active_tab = Tab::Tasks,
            "tab:tools" => self.active_tab = Tab::Tools,
            "tab:commands" => self.active_tab = Tab::Commands,
            "tab:cost" => self.active_tab = Tab::Cost,
            "tab:agents" => self.active_tab = Tab::Agents,
            "tab:events" => self.active_tab = Tab::Events,
            "tab:logs" => self.active_tab = Tab::Logs,
            "action:vim" => {
                self.vim_enabled = !self.vim_enabled;
                self.vim_state = VimState::default();
            }
            "action:clear" => {
                self.chat_messages.clear();
                self.chat_reconciler.reset();
            }
            "action:help" => self.show_help = !self.show_help,
            "action:quit" => self.should_quit = true,
            _ => {}
        }
    }

    fn send_chat(&mut self) {
        if !self.chat_input.is_empty() {
            let msg = self.chat_input.clone();
            self.chat_input.clear();
            self.chat_cursor_pos = 0;
            if let Some(cmd) = msg.strip_prefix('/') {
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                let name = parts.first().unwrap_or(&"").to_string();
                let args: Vec<String> = parts.iter().skip(1).map(|s| s.to_string()).collect();
                self.chat_messages.push(ChatMessage::with_role_now(
                    "system",
                    format!("/{name} {}", args.join(" ")),
                ));
                let _ = self
                    .bridge
                    .send(agent_bridge::UserInput::SlashCommand { name, args });
            } else {
                self.chat_messages.push(ChatMessage::with_role_now(
                    "user",
                    msg.clone(),
                ));
                let _ = self.bridge.send(agent_bridge::UserInput::Chat(msg));
            }
        }
    }

    fn handle_chat_key_normal(&mut self, key: event::KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                self.push_undo_snapshot();
                self.chat_input.insert(self.chat_cursor_pos, c);
                self.chat_cursor_pos += c.len_utf8();
            }
            KeyCode::Backspace => {
                if self.chat_cursor_pos > 0 {
                    self.push_undo_snapshot();
                    let prev = self.chat_input[..self.chat_cursor_pos]
                        .chars()
                        .last()
                        .map(|c| c.len_utf8())
                        .unwrap_or(1);
                    let start = self.chat_cursor_pos - prev;
                    self.chat_input.drain(start..self.chat_cursor_pos);
                    self.chat_cursor_pos = start;
                }
            }
            KeyCode::Delete => {
                if self.chat_cursor_pos < self.chat_input.len() {
                    self.push_undo_snapshot();
                    let next = self.chat_input[self.chat_cursor_pos..]
                        .chars()
                        .next()
                        .map(|c| c.len_utf8())
                        .unwrap_or(1);
                    self.chat_input
                        .drain(self.chat_cursor_pos..self.chat_cursor_pos + next);
                }
            }
            KeyCode::Left => {
                if self.chat_cursor_pos > 0 {
                    let prev = self.chat_input[..self.chat_cursor_pos]
                        .chars()
                        .last()
                        .map(|c| c.len_utf8())
                        .unwrap_or(1);
                    self.chat_cursor_pos -= prev;
                }
            }
            KeyCode::Right => {
                if self.chat_cursor_pos < self.chat_input.len() {
                    let next = self.chat_input[self.chat_cursor_pos..]
                        .chars()
                        .next()
                        .map(|c| c.len_utf8())
                        .unwrap_or(1);
                    self.chat_cursor_pos += next;
                }
            }
            KeyCode::Home => {
                self.chat_cursor_pos = 0;
            }
            KeyCode::End => {
                self.chat_cursor_pos = self.chat_input.len();
            }
            KeyCode::Esc if self.bridge.is_busy => {
                let _ = self.bridge.send(agent_bridge::UserInput::Cancel);
            }
            KeyCode::Enter
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.chat_input.insert(self.chat_cursor_pos, '\n');
                self.chat_cursor_pos += 1;
            }
            KeyCode::Enter => {
                self.send_chat();
            }
            _ => {}
        }
    }

    fn handle_memory_key(&mut self, key: event::KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.memory_list_state.selected().unwrap_or(0);
                if i > 0 {
                    self.memory_list_state.select(Some(i - 1));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.memory_list_state.selected().unwrap_or(0);
                if i + 1 < self.memory_entries.len() {
                    self.memory_list_state.select(Some(i + 1));
                }
            }
            _ => {}
        }
    }

    fn handle_list_key(&mut self, key: event::KeyEvent, target: ListTarget) {
        let (state, len) = match target {
            ListTarget::Tasks => (&mut self.task_list_state, self.task_entries.len()),
            ListTarget::Tools => (&mut self.tool_list_state, self.tool_entries.len()),
            ListTarget::Commands => (&mut self.command_list_state, self.command_entries.len()),
        };
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let i = state.selected().unwrap_or(0);
                if i > 0 {
                    state.select(Some(i - 1));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let i = state.selected().unwrap_or(0);
                if i + 1 < len {
                    state.select(Some(i + 1));
                }
            }
            _ => {}
        }
    }

    fn tick(&mut self) {
        self.tick_count += 1;
        self.status_info.uptime_secs += 1;
        self.drain_agent_events();
        self.drain_inline_edit_outcomes();
        self.sync_from_services();
    }

    fn drain_agent_events(&mut self) {
        let events = self.bridge.poll_events();
        for ev in events {
            self.handle_agent_event(ev);
        }
        self.reconcile_chat_messages();
    }

    fn drain_inline_edit_outcomes(&mut self) {
        loop {
            match self.inline_edit_outcome_rx.try_recv() {
                Ok(RunnerOutcomeMessage::Success(success)) => {
                    let path_display = success.path.display().to_string();
                    let ts = chrono::Local::now().format("%H:%M:%S").to_string();
                    self.edit_batch_registry.push_from_inline_edit(
                        path_display.clone(),
                        success.additions,
                        success.deletions,
                        Some(success.diff.clone()),
                        success.checkpoint_id.clone(),
                        ts,
                    );
                    let summary = format!(
                        "inline-edit ready for review: {path_display} (+{add} / -{del})",
                        add = success.additions,
                        del = success.deletions,
                    );
                    self.chat_messages.push(ChatMessage::with_role_now(
                        "system",
                        summary.clone(),
                    ));
                    if !success.validator_issues.is_empty() {
                        let issues = success.validator_issues.join("; ");
                        self.chat_messages.push(ChatMessage::with_role_now(
                            "system",
                            format!("validator issues: {issues}"),
                        ));
                    }
                    self.event_entries.push(format!(
                        "[{}] inline_edit: {summary}",
                        chrono::Local::now().format("%H:%M:%S")
                    ));
                    self.inline_edit_modal.status = Some(summary);
                    self.inline_edit_modal.close();
                    crate::observability::tui_metrics::incr_tui_inline_edit_success();
                    self.mark_dirty();
                }
                Ok(RunnerOutcomeMessage::Failure { path, error }) => {
                    let summary = format!(
                        "inline-edit failed for {}: {error}",
                        path.display()
                    );
                    self.chat_messages.push(ChatMessage::with_role_now(
                        "system",
                        summary.clone(),
                    ));
                    self.event_entries.push(format!(
                        "[{}] inline_edit_error: {summary}",
                        chrono::Local::now().format("%H:%M:%S")
                    ));
                    self.inline_edit_modal.status = Some(summary);
                    crate::observability::tui_metrics::incr_tui_inline_edit_failed();
                    self.mark_dirty();
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
                | Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
            }
        }
    }

    pub fn reconcile_chat_messages(&mut self) {
        let outcome = self
            .chat_reconciler
            .reconcile(&mut self.chat_messages, &self.bridge.session_actor_slot);
        if matches!(
            outcome,
            chat_message_reconciler::ReconcileOutcome::Backfilled
        ) {
            self.mark_dirty();
        }
    }

    pub fn handle_agent_event(&mut self, ev: agent_bridge::AgentEvent) {
        let ts = chrono::Local::now().format("%H:%M:%S").to_string();
        {
            match ev {
                agent_bridge::AgentEvent::AssistantMessage(text) => {
                    self.event_entries
                        .push(format!("[{ts}] assistant: response received"));
                    self.chat_messages.push(ChatMessage::from_parts(
                        "assistant",
                        text,
                        ts.clone(),
                    ));
                }
                agent_bridge::AgentEvent::ToolUse { name, id, .. } => {
                    self.event_entries.push(format!("[{ts}] tool_use: {name}"));
                    self.chat_messages.push(ChatMessage::from_parts(
                        "tool",
                        format!("▶ {name} ({id})"),
                        ts.clone(),
                    ));
                }
                agent_bridge::AgentEvent::ToolResult {
                    id,
                    output,
                    success,
                } => {
                    let prefix = if success { "✓" } else { "✗" };
                    self.event_entries
                        .push(format!("[{ts}] tool_result: {prefix} {id}"));
                    self.chat_messages.push(ChatMessage::from_parts(
                        "tool",
                        format!("{prefix} [{id}] {output}"),
                        ts.clone(),
                    ));
                }
                agent_bridge::AgentEvent::Thinking => {
                    self.event_entries.push(format!("[{ts}] thinking"));

                    let already_thinking = self
                        .chat_messages
                        .last()
                        .map(|m| m.role == "system" && m.content == "Thinking\u{2026}")
                        .unwrap_or(false);
                    if !already_thinking {
                        self.chat_messages.push(ChatMessage::from_parts(
                            "system",
                            "Thinking\u{2026}".into(),
                            ts.clone(),
                        ));
                    }
                }
                agent_bridge::AgentEvent::Done => {
                    self.event_entries.push(format!("[{ts}] done"));

                    if !self.streaming_buffer.is_empty() {
                        let text = std::mem::take(&mut self.streaming_buffer);

                        let replaced = if let Some(last) = self.chat_messages.last_mut() {
                            if last.role == "system" && last.content == "Thinking\u{2026}" {
                                last.role = "assistant".into();
                                last.content = text.clone();
                                last.timestamp = ts.clone();
                                last.mark_content_dirty();
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        if !replaced {
                            self.chat_messages.push(ChatMessage::from_parts(
                                "assistant",
                                text,
                                ts.clone(),
                            ));
                        }
                    }
                }
                agent_bridge::AgentEvent::Error(e) => {
                    self.event_entries.push(format!("[{ts}] error: {e}"));
                    self.chat_messages
                        .push(ChatMessage::from_parts("error", e, ts.clone()));
                }
                agent_bridge::AgentEvent::CommandOutput(output) => {
                    self.event_entries.push(format!("[{ts}] command_output"));
                    self.chat_messages
                        .push(ChatMessage::from_parts("system", output, ts.clone()));
                }
                agent_bridge::AgentEvent::ModeChanged(mode_name) => {
                    self.event_entries
                        .push(format!("[{ts}] mode_changed: {mode_name}"));
                }
                agent_bridge::AgentEvent::StreamChunk(chunk) => {
                    self.streaming_buffer.push_str(&chunk);
                }
                agent_bridge::AgentEvent::ThinkingChunk(_delta) => {

                    self.event_entries.push(format!("[{ts}] thinking_chunk"));
                }
                agent_bridge::AgentEvent::ConfigWarning(msg) => {
                    self.event_entries
                        .push(format!("[{ts}] config_warning: {msg}"));
                }
                agent_bridge::AgentEvent::FileEdit {
                    path,
                    additions,
                    deletions,
                    diff,
                    edit_batch_id,
                } => {
                    self.event_entries.push(format!(
                        "[{ts}] file_edit: {path} (+{additions}/-{deletions})"
                    ));
                    self.chat_messages.push(ChatMessage::from_parts(
                        "tool",
                        format!("\u{270E} Edited {path} (+{additions}/-{deletions})"),
                        ts.clone(),
                    ));

                    self.edit_batch_registry.push_from_file_edit(
                        path.clone(),
                        additions,
                        deletions,
                        diff.clone(),
                        edit_batch_id,
                        ts.clone(),
                    );

                    let pending = self.pending_inline_path.take();
                    if let Some(pp) = pending {
                        let tagged = self
                            .edit_batch_registry
                            .mark_latest_inline_for(std::path::Path::new(&path));
                        if !tagged {
                            self.pending_inline_path = Some(pp);
                        }
                    }
                    if let Some(diff_text) = diff {
                        let max_lines = 30;
                        for (i, line) in diff_text.lines().enumerate() {
                            if i >= max_lines {
                                self.chat_messages.push(ChatMessage::from_parts(
                                    "tool",
                                    format!(
                                        "  ... ({} more lines)",
                                        diff_text.lines().count() - max_lines
                                    ),
                                    ts.clone(),
                                ));
                                break;
                            }
                            self.chat_messages.push(ChatMessage::from_parts(
                                "tool",
                                format!("  {line}"),
                                ts.clone(),
                            ));
                        }
                    }
                }
                agent_bridge::AgentEvent::StatusUpdate { action, detail } => {
                    self.event_entries
                        .push(format!("[{ts}] status: {action} {detail}"));
                }
                agent_bridge::AgentEvent::TodoUpdate {
                    completed, total, ..
                } => {
                    self.event_entries
                        .push(format!("[{ts}] todo: {completed}/{total}"));
                }
                agent_bridge::AgentEvent::BackgroundShell {
                    command, running, ..
                } => {
                    let status = if running { "running" } else { "done" };
                    self.event_entries
                        .push(format!("[{ts}] shell ({status}): {command}"));
                }
                agent_bridge::AgentEvent::SubagentSpawn { description, .. } => {
                    self.event_entries
                        .push(format!("[{ts}] subagent spawned: {description}"));
                }
                agent_bridge::AgentEvent::SubagentUpdate { id, status, .. } => {
                    self.event_entries
                        .push(format!("[{ts}] subagent {id}: {status:?}"));
                }
                agent_bridge::AgentEvent::PlanCreated { title, .. } => {
                    self.event_entries
                        .push(format!("[{ts}] plan created: {title}"));
                    self.chat_messages.push(ChatMessage::from_parts(
                        "system",
                        format!("\u{1F4CB} Plan created: {title}"),
                        ts.clone(),
                    ));
                }
                agent_bridge::AgentEvent::ApprovalRequest {
                    tool_name,
                    args_summary,
                    ..
                } => {
                    self.chat_messages.push(ChatMessage::from_parts(
                        "system",
                        format!("\u{26A0} Approval needed: {tool_name} — {args_summary}"),
                        ts.clone(),
                    ));
                }
                agent_bridge::AgentEvent::QuestionAsked { prompt, .. } => {

                    self.chat_messages.push(ChatMessage::from_parts(
                        "system",
                        format!("\u{2753} {prompt}"),
                        ts.clone(),
                    ));
                }
                agent_bridge::AgentEvent::QuestionAnswered { items } => {

                    let summary = items
                        .iter()
                        .map(|i| {
                            let labels = if i.selected_labels.is_empty() {
                                i.selected.join(", ")
                            } else {
                                i.selected_labels.join(", ")
                            };
                            if i.prompt.is_empty() {
                                labels
                            } else {
                                format!("{} → {}", i.prompt, labels)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" | ");
                    self.chat_messages.push(ChatMessage::from_parts(
                        "system",
                        format!("\u{1F4DD} Answers: {summary}"),
                        ts.clone(),
                    ));
                }
                agent_bridge::AgentEvent::BackgroundShellChunk { id, line, .. } => {

                    self.chat_messages.push(ChatMessage::from_parts(
                        "system",
                        format!("[bg:{id}] {line}"),
                        ts.clone(),
                    ));
                }
                agent_bridge::AgentEvent::SubagentChildEvent {
                    agent_id,
                    block_kind,
                    payload,
                    ..
                } => {

                    let summary = payload
                        .get("text")
                        .or_else(|| payload.get("output"))
                        .or_else(|| payload.get("action"))
                        .or_else(|| payload.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    self.chat_messages.push(ChatMessage::from_parts(
                        "system",
                        format!("[{agent_id}::{block_kind}] {summary}"),
                        ts.clone(),
                    ));
                }
                agent_bridge::AgentEvent::PlanReady { filename, path } => {

                    self.event_entries
                        .push(format!("[{ts}] plan_ready: {filename}"));
                    self.chat_messages.push(ChatMessage::from_parts(
                        "system",
                        format!("plan saved: {path}"),
                        ts.clone(),
                    ));
                }
            }
        }
    }

    fn sync_from_services(&mut self) {
        if let Some(bs) = try_get_bootstrap_state() {
            bs.read(|s| {
                self.status_info.cost_today = s.total_cost_usd;
                self.cost_details.session_cost_usd = s.total_cost_usd;
                self.cost_details.today_cost_usd = s.total_cost_usd;
                self.cost_details.total_requests =
                    s.model_usage.values().map(|u| u.request_count).sum();
                self.cost_details.total_input_tokens =
                    s.model_usage.values().map(|u| u.input_tokens).sum();
                self.cost_details.total_output_tokens =
                    s.model_usage.values().map(|u| u.output_tokens).sum();
                self.cost_details.total_cache_read_tokens = s
                    .model_usage
                    .values()
                    .map(|u| u.cache_read_input_tokens)
                    .sum();
                self.cost_details.total_cache_write_tokens = s
                    .model_usage
                    .values()
                    .map(|u| u.cache_creation_input_tokens)
                    .sum();

                self.cost_details.model_costs = s
                    .model_usage
                    .iter()
                    .map(|(name, u)| ModelCostEntry {
                        model_name: name.clone(),
                        cost_usd: u.total_cost_usd,
                        requests: u.request_count,
                        input_tokens: u.input_tokens,
                        output_tokens: u.output_tokens,
                    })
                    .collect();

                if let Some(ref m) = s.main_loop_model_override {
                    self.status_info.model = m.clone();
                }
            });
        }

        if let Some(svc) = try_get_services() {
            if self.command_entries.is_empty() {
                self.command_entries = svc
                    .command_registry
                    .list(None)
                    .iter()
                    .map(|c| CommandEntry {
                        name: c.name.clone(),
                        description: c.description.clone(),
                        category: c.category.to_string(),
                        usage: c.usage.clone(),
                    })
                    .collect();
            }

            if self.tool_entries.is_empty() || self.tick_count % 10 == 0 {
                if let Ok(guard) = svc.tool_use_summary.lock() {
                    self.tool_entries = guard
                        .aggregate()
                        .into_iter()
                        .map(|s| ToolEntry {
                            name: s.tool_name,
                            category: String::new(),
                            call_count: s.call_count,
                            enabled: true,
                        })
                        .collect();
                }
            }
        }

        if self.tick_count % 5 == 0 {
            if let Ok(sessions) = crate::cli::bg::list_sessions_sync(&self.config.workspace_dir) {
                self.task_entries = sessions
                    .into_iter()
                    .map(|s| TaskEntry {
                        id: s.id,
                        task_type: "background".into(),
                        status: s.status.to_string(),
                        description: s.cwd.display().to_string(),
                        duration_ms: 0,
                    })
                    .collect();
            }
        }

        if self.memory_entries.is_empty() || self.tick_count % 30 == 0 {
            let cwd = std::env::current_dir().unwrap_or_default();
            let candidates = [
                "CLAUDE.md",
                "AGENTS.md",
                "MEMORY.md",
                ".senweavercoding/MEMORY.md",
                ".claude/CLAUDE.md",
            ];
            let mut entries = Vec::new();
            for name in &candidates {
                let path = cwd.join(name);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let summary: String = content.lines().take(3).collect::<Vec<_>>().join(" | ");
                    let preview = if summary.len() > 120 {
                        let mut end = 120;
                        while end > 0 && !summary.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}...", &summary[..end])
                    } else {
                        summary
                    };
                    entries.push(MemoryEntry {
                        key: name.to_string(),
                        category: "file".to_string(),
                        preview,
                    });
                }
            }
            if !entries.is_empty() {
                self.memory_entries = entries;
            }
        }
    }
}

enum ListTarget {
    Tasks,
    Tools,
    Commands,
}

fn draw_chat_partial(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());
    draw_chat(f, app, chunks[1]);
    inline_edit_modal::draw(f, &app.inline_edit_modal, chunks[1]);
}

fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_tabs(f, app, chunks[0]);

    match app.active_tab {
        Tab::Dashboard => draw_dashboard(f, app, chunks[1]),
        Tab::Chat => {
            draw_chat(f, app, chunks[1]);

            inline_edit_modal::draw(f, &app.inline_edit_modal, chunks[1]);
        }
        Tab::Memory => draw_memory(f, app, chunks[1]),
        Tab::Channels => draw_channels(f, app, chunks[1]),
        Tab::Tasks => draw_tasks(f, app, chunks[1]),
        Tab::Tools => draw_tools(f, app, chunks[1]),
        Tab::Commands => draw_commands(f, app, chunks[1]),
        Tab::Cost => draw_cost(f, app, chunks[1]),
        Tab::Agents => draw_agents(f, app, chunks[1]),
        Tab::Events => draw_events(f, app, chunks[1]),
        Tab::Logs => draw_logs(f, app, chunks[1]),
        Tab::Diff => diff_review::draw(
            f,
            &mut app.diff_review_state,
            &app.edit_batch_registry,
            chunks[1],
        ),
        Tab::Files => {
            let open_files = app.collect_open_files();
            file_viewer::draw(
                f,
                &mut app.file_viewer_state,
                &app.workspace_root,
                &open_files,
                chunks[1],
            );
        }
    }

    draw_status_bar(f, app, chunks[2]);

    if app.show_help {
        draw_help_overlay(f);
    }

    if app.command_palette_open {
        draw_command_palette(f, app);
    }
}

fn draw_help_overlay(f: &mut Frame) {
    let area = f.area();
    let shortcuts = App::get_shortcuts();
    let line_count = shortcuts.len() as u16 + 4;
    let w = 56u16.min(area.width.saturating_sub(4));
    let h = line_count.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);

    let block = Block::default()
        .title(" Help \u{2014} press ? or Esc to close ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ratatui::style::Color::Cyan));

    let mut help_text: Vec<Line> = Vec::with_capacity(shortcuts.len() + 4);
    for sc in &shortcuts {
        help_text.push(Line::from(format!("  {:<20} {}", sc.key, sc.description)));
    }
    help_text.push(Line::from(""));
    help_text.push(Line::from("  Chat tab extras:"));
    help_text.push(Line::from("  Shift+Enter        New line"));
    help_text.push(Line::from("  Left/Right         Move cursor in input"));
    help_text.push(Line::from("  Home/End           Jump to start/end"));
    help_text.push(Line::from("  /command           Slash command"));
    help_text.push(Line::from("  Scroll wheel       Scroll chat history"));

    let para = Paragraph::new(help_text)
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(para, popup);
}

fn draw_command_palette(f: &mut Frame, app: &App) {
    let area = f.area();
    let w = 50u16.min(area.width.saturating_sub(4));
    let cmds = app.filtered_palette_commands();
    let h = (cmds.len() as u16 + 4)
        .min(area.height.saturating_sub(4))
        .max(6);
    let x = (area.width.saturating_sub(w)) / 2;
    let y = area.height / 6;
    let popup = Rect::new(x, y, w, h);

    let block = Block::default()
        .title(" Command Palette (Esc to close) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::ACCENT));

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("> ", theme::title()),
        Span::styled(
            if app.command_palette_filter.is_empty() {
                "Type to filter..."
            } else {
                &app.command_palette_filter
            },
            if app.command_palette_filter.is_empty() {
                theme::dim()
            } else {
                theme::normal()
            },
        ),
    ]));
    lines.push(Line::from(Span::styled(
        "─".repeat(w as usize - 2),
        theme::dim(),
    )));

    for (i, (label, _action)) in cmds.iter().enumerate() {
        let style = if i == app.command_palette_selected {
            theme::selected()
        } else {
            theme::normal()
        };
        let prefix = if i == app.command_palette_selected {
            "▸ "
        } else {
            "  "
        };
        lines.push(Line::from(Span::styled(format!("{prefix}{label}"), style)));
    }

    if cmds.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No matching commands",
            theme::dim(),
        )));
    }

    let para = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(para, popup);
}

fn draw_tabs(f: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = Tab::all()
        .iter()
        .map(|t| {
            let style = if *t == app.active_tab {
                theme::tab_active()
            } else {
                theme::tab_inactive()
            };
            Line::from(Span::styled(format!(" {} ", t.title()), style))
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .title(" SenWeaverCoding ")
                .title_style(theme::title()),
        )
        .select(app.active_tab.index())
        .highlight_style(theme::selected());

    f.render_widget(tabs, area);
}

fn draw_dashboard(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    let info = vec![
        Line::from(vec![
            Span::styled("Version:    ", theme::dim()),
            Span::styled(&app.status_info.version, theme::normal()),
        ]),
        Line::from(vec![
            Span::styled("Provider:   ", theme::dim()),
            Span::styled(&app.status_info.provider, theme::info_style()),
        ]),
        Line::from(vec![
            Span::styled("Model:      ", theme::dim()),
            Span::styled(&app.status_info.model, theme::info_style()),
        ]),
        Line::from(vec![
            Span::styled("Autonomy:   ", theme::dim()),
            Span::styled(&app.status_info.autonomy, theme::success_style()),
        ]),
        Line::from(vec![
            Span::styled("Memory:     ", theme::dim()),
            Span::styled(&app.status_info.memory_backend, theme::normal()),
        ]),
        Line::from(vec![
            Span::styled("Uptime:     ", theme::dim()),
            Span::styled(format_uptime(app.status_info.uptime_secs), theme::normal()),
        ]),
    ];

    let info_block = Paragraph::new(info)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" System ")
                .title_style(theme::title()),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(info_block, left_chunks[0]);

    let cost_info = vec![
        Line::from(vec![
            Span::styled("Today:  ", theme::dim()),
            Span::styled(
                format!("${:.4}", app.status_info.cost_today),
                theme::normal(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Month:  ", theme::dim()),
            Span::styled(
                format!("${:.4}", app.status_info.cost_month),
                theme::normal(),
            ),
        ]),
    ];

    let cost_block = Paragraph::new(cost_info).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Cost Tracking ")
            .title_style(theme::title()),
    );
    f.render_widget(cost_block, left_chunks[1]);

    let channels_text = if app.status_info.channels_active > 0 {
        format!(
            "{} active / {} total",
            app.status_info.channels_active, app.status_info.channels_total
        )
    } else {
        "No channels configured".to_string()
    };

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[1]);

    let channels_block = Paragraph::new(vec![
        Line::from(Span::styled(channels_text, theme::normal())),
        Line::from(""),
        Line::from(Span::styled(
            "CLI channel is always active",
            theme::success_style(),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Channels ")
            .title_style(theme::title()),
    );
    f.render_widget(channels_block, right_chunks[0]);

    let spinner_idx = (app.tick_count as usize) % theme::SPINNER_FRAMES.len();
    let spinner = theme::SPINNER_FRAMES[spinner_idx];

    let recent: Vec<Line> = app
        .log_entries
        .iter()
        .rev()
        .take(10)
        .map(|entry| Line::from(Span::styled(entry.as_str(), theme::dim())))
        .collect();

    let recent_block = Paragraph::new(recent)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Activity {spinner} "))
                .title_style(theme::title()),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(recent_block, right_chunks[1]);
}

fn draw_chat(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let mut messages: Vec<Line> = Vec::new();
    let viewport_fp = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        use std::hash::Hasher;
        h.write_usize(app.chat_messages.len());
        h.write_u64(
            app.chat_messages
                .last()
                .map(|m| m.content_hash)
                .unwrap_or(0),
        );
        h.write_usize(app.streaming_buffer.len());
        h.write_u8(app.bridge.is_busy as u8);
        h.write_u16(chunks[0].width);
        h.write_u16(chunks[0].height);
        h.write_usize(app.chat_scroll_offset);
        h.finish()
    };
    let _viewport_hit = app.chat_render_cache.viewport_matches(viewport_fp);
    for m in &app.chat_messages {
        let header = vec![
            Span::styled(format!("[{}] ", m.timestamp), theme::dim()),
            Span::styled(format!("{}: ", m.role), theme::style_for_role(&m.role)),
        ];
        if m.content.contains("```") {
            messages.push(Line::from(header));
            let highlighted =
                chat_render_cache::render_message_cached(&m.highlighted_cache, &m.content);

            messages.extend(highlighted.iter().cloned());
        } else {
            let mut spans = header;
            spans.push(Span::styled(m.content.clone(), theme::normal()));
            messages.push(Line::from(spans));
        }
    }

    if app.bridge.is_busy && !app.streaming_buffer.is_empty() {
        let spinner_idx = (app.tick_count as usize) % theme::SPINNER_FRAMES.len();
        let spinner = theme::SPINNER_FRAMES[spinner_idx];

        let preview: &str = if app.streaming_buffer.len() > 800 {
            let start = app.streaming_buffer.len() - 800;
            let start = app.streaming_buffer.floor_char_boundary(start);
            &app.streaming_buffer[start..]
        } else {
            &app.streaming_buffer
        };
        for line in preview.lines() {
            messages.push(Line::from(Span::styled(
                format!("{spinner} {line}"),
                theme::thinking_style(),
            )));
        }
    } else if app.bridge.is_busy {
        let spinner_idx = (app.tick_count as usize) % theme::SPINNER_FRAMES.len();
        let spinner = theme::SPINNER_FRAMES[spinner_idx];
        messages.push(Line::from(Span::styled(
            format!("{spinner} Agent is thinking… (Esc to cancel)"),
            theme::thinking_style(),
        )));
    }

    let visible_messages = if app.chat_scroll_offset > 0 && messages.len() > app.chat_scroll_offset
    {
        messages[..messages.len() - app.chat_scroll_offset].to_vec()
    } else {
        messages
    };

    let visible_len = visible_messages.len();
    let messages_block = Paragraph::new(visible_messages)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Conversation ")
                .title_style(theme::title()),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(messages_block, chunks[0]);

    let first_visible_idx = app
        .chat_messages
        .len()
        .saturating_sub(visible_len.saturating_add(app.chat_scroll_offset));
    app.chat_render_cache
        .record_render(viewport_fp, first_visible_idx, chunks[0].height);
    crate::observability::tui_metrics::add_tui_chat_lines_rendered(visible_len as u64);

    let input_title = if app.bridge.is_busy {
        " Input (Esc to cancel) "
    } else {
        " Input (Enter to send, / for commands) "
    };

    let before_cursor = &app.chat_input[..app.chat_cursor_pos.min(app.chat_input.len())];
    let after_cursor = &app.chat_input[app.chat_cursor_pos.min(app.chat_input.len())..];
    let input = Paragraph::new(Line::from(vec![
        Span::styled("> ", theme::info_style()),
        Span::styled(before_cursor, theme::normal()),
        Span::styled(
            "\u{2588}",
            Style::default().add_modifier(Modifier::SLOW_BLINK),
        ),
        Span::styled(after_cursor, theme::normal()),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(input_title)
            .title_style(theme::title()),
    );
    f.render_widget(input, chunks[1]);
}

fn draw_memory(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .memory_entries
        .iter()
        .map(|e| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("[{}] ", e.category), theme::info_style()),
                Span::styled(&e.key, theme::normal()),
                Span::styled(format!(" - {}", e.preview), theme::dim()),
            ]))
        })
        .collect();

    let placeholder = if items.is_empty() {
        vec![ListItem::new(Span::styled(
            "No memory entries. Use 'sen agent -m \"remember ...\"' to store memories.",
            theme::dim(),
        ))]
    } else {
        items
    };

    let list = List::new(placeholder)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Memory Entries (j/k to navigate) ")
                .title_style(theme::title()),
        )
        .highlight_style(theme::selected())
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, &mut app.memory_list_state);
}

fn draw_channels(f: &mut Frame, _app: &App, area: Rect) {
    let channel_names = [
        "CLI", "Telegram", "Discord", "Slack", "Matrix", "WhatsApp", "Email", "IRC", "Lark",
        "DingTalk", "Signal", "Reddit",
    ];

    let items: Vec<ListItem> = channel_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let (status, style) = if i == 0 {
                ("active", theme::success_style())
            } else {
                ("not configured", theme::dim())
            };
            ListItem::new(Line::from(vec![
                Span::styled(if i == 0 { " [*] " } else { " [ ] " }, style),
                Span::styled(*name, theme::normal()),
                Span::styled(format!(" - {status}"), style),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Channel Status ")
            .title_style(theme::title()),
    );
    f.render_widget(list, area);
}

fn draw_agents(f: &mut Frame, app: &App, area: Rect) {

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    panels::render_budget(f, chunks[0], &app.budget_view);
    panels::render_provider_health(f, chunks[1], &app.provider_health_view);
}

fn draw_events(f: &mut Frame, app: &App, area: Rect) {
    let events: Vec<Line> = if app.event_entries.is_empty() {
        vec![Line::from(Span::styled(
            "No events yet. Events will appear as the agent processes requests.",
            theme::dim(),
        ))]
    } else {
        app.event_entries
            .iter()
            .rev()
            .take(50)
            .map(|e| Line::from(Span::styled(e.as_str(), theme::normal())))
            .collect()
    };

    let block = Paragraph::new(events)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Event Bus Monitor ")
                .title_style(theme::title()),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(block, area);
}

fn draw_logs(f: &mut Frame, app: &App, area: Rect) {
    let logs: Vec<Line> = app
        .log_entries
        .iter()
        .rev()
        .take(100)
        .map(|l| Line::from(Span::styled(l.as_str(), theme::normal())))
        .collect();

    let block = Paragraph::new(logs)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Logs ")
                .title_style(theme::title()),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(block, area);
}

fn draw_tasks(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = if app.task_entries.is_empty() {
        vec![ListItem::new(Span::styled(
            "No background tasks running. Tasks will appear when the agent spawns sub-agents or scheduled work.",
            theme::dim(),
        ))]
    } else {
        app.task_entries
            .iter()
            .map(|t| {
                let status_style = match t.status.as_str() {
                    "running" => theme::info_style(),
                    "completed" => theme::success_style(),
                    "failed" => Style::default().fg(ratatui::style::Color::Red),
                    _ => theme::dim(),
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("[{}] ", t.status), status_style),
                    Span::styled(format!("{} ", t.task_type), theme::info_style()),
                    Span::styled(&t.id, theme::dim()),
                    Span::styled(format!(" - {}", t.description), theme::normal()),
                    Span::styled(format!(" ({}ms)", t.duration_ms), theme::dim()),
                ]))
            })
            .collect()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Background Tasks (j/k to navigate) ")
                .title_style(theme::title()),
        )
        .highlight_style(theme::selected())
        .highlight_symbol("> ");
    f.render_stateful_widget(list, area, &mut app.task_list_state);
}

fn draw_tools(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = if app.tool_entries.is_empty() {
        vec![ListItem::new(Span::styled(
            "No tools registered yet. Tools load at agent startup.",
            theme::dim(),
        ))]
    } else {
        app.tool_entries
            .iter()
            .map(|t| {
                let enabled_indicator = if t.enabled { "[*]" } else { "[ ]" };
                let enabled_style = if t.enabled {
                    theme::success_style()
                } else {
                    theme::dim()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {enabled_indicator} "), enabled_style),
                    Span::styled(&t.name, theme::normal()),
                    Span::styled(format!(" ({}) ", t.category), theme::info_style()),
                    Span::styled(format!("calls: {}", t.call_count), theme::dim()),
                ]))
            })
            .collect()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Tools (j/k to navigate) ")
                .title_style(theme::title()),
        )
        .highlight_style(theme::selected())
        .highlight_symbol("> ");
    f.render_stateful_widget(list, area, &mut app.tool_list_state);
}

fn draw_commands(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = if app.command_entries.is_empty() {
        vec![ListItem::new(Span::styled(
            "No slash commands registered. Commands load via ServiceContainer at startup.",
            theme::dim(),
        ))]
    } else {
        app.command_entries
            .iter()
            .map(|c| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" /{} ", c.name), theme::info_style()),
                    Span::styled(format!("[{}] ", c.category), theme::dim()),
                    Span::styled(&c.description, theme::normal()),
                    Span::styled(format!("  {}", c.usage), theme::dim()),
                ]))
            })
            .collect()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Slash Commands (j/k to navigate) ")
                .title_style(theme::title()),
        )
        .highlight_style(theme::selected())
        .highlight_symbol("> ");
    f.render_stateful_widget(list, area, &mut app.command_list_state);
}

fn draw_cost(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(0)])
        .split(area);

    let cd = &app.cost_details;

    let summary = vec![
        Line::from(vec![
            Span::styled("Session Cost:  ", theme::dim()),
            Span::styled(format!("${:.6}", cd.session_cost_usd), theme::info_style()),
        ]),
        Line::from(vec![
            Span::styled("Today Cost:    ", theme::dim()),
            Span::styled(format!("${:.6}", cd.today_cost_usd), theme::normal()),
        ]),
        Line::from(vec![
            Span::styled("Month Cost:    ", theme::dim()),
            Span::styled(format!("${:.6}", cd.month_cost_usd), theme::normal()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Input Tokens:  ", theme::dim()),
            Span::styled(format!("{}", cd.total_input_tokens), theme::normal()),
        ]),
        Line::from(vec![
            Span::styled("Output Tokens: ", theme::dim()),
            Span::styled(format!("{}", cd.total_output_tokens), theme::normal()),
        ]),
        Line::from(vec![
            Span::styled("Cache Read:    ", theme::dim()),
            Span::styled(format!("{}", cd.total_cache_read_tokens), theme::normal()),
        ]),
        Line::from(vec![
            Span::styled("Cache Write:   ", theme::dim()),
            Span::styled(format!("{}", cd.total_cache_write_tokens), theme::normal()),
        ]),
    ];

    let summary_block = Paragraph::new(summary).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Cost Summary ")
            .title_style(theme::title()),
    );
    f.render_widget(summary_block, chunks[0]);

    let model_lines: Vec<Line> = if cd.model_costs.is_empty() {
        vec![Line::from(Span::styled(
            "No model usage recorded yet.",
            theme::dim(),
        ))]
    } else {
        let mut lines = vec![Line::from(vec![
            Span::styled("Model", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled("                    ", theme::dim()),
            Span::styled("Cost", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled("         ", theme::dim()),
            Span::styled("Requests", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled("   ", theme::dim()),
            Span::styled("In Tokens", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled("    ", theme::dim()),
            Span::styled("Out Tokens", Style::default().add_modifier(Modifier::BOLD)),
        ])];
        for mc in &cd.model_costs {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:<24}", truncate_str(&mc.model_name, 24)),
                    theme::info_style(),
                ),
                Span::styled(format!("${:<12.6}", mc.cost_usd), theme::normal()),
                Span::styled(format!("{:<11}", mc.requests), theme::normal()),
                Span::styled(format!("{:<13}", mc.input_tokens), theme::dim()),
                Span::styled(format!("{}", mc.output_tokens), theme::dim()),
            ]));
        }
        lines
    };

    let model_block = Paragraph::new(model_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Per-Model Breakdown ")
                .title_style(theme::title()),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(model_block, chunks[1]);
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![
        Span::styled(" SenWeaverCoding ", theme::title()),
        Span::styled("| ", theme::dim()),
        Span::styled("Tab/Shift+Tab: switch ", theme::dim()),
        Span::styled("| ", theme::dim()),
        Span::styled("F1-F10: jump ", theme::dim()),
        Span::styled("| ", theme::dim()),
        Span::styled("Ctrl+Q: quit ", theme::dim()),
    ];

    if app.vim_enabled {
        let mode_str = format!(" [{}] ", app.vim_state.mode);
        spans.push(Span::styled("| ", theme::dim()));
        spans.push(Span::styled(
            mode_str,
            Style::default()
                .fg(ratatui::style::Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    let bar = Paragraph::new(Line::from(spans));
    f.render_widget(bar, area);
}

fn format_uptime(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    if hours > 0 {
        format!("{hours}h {mins}m {s}s")
    } else if mins > 0 {
        format!("{mins}m {s}s")
    } else {
        format!("{s}s")
    }
}

pub async fn run_tui(config: Config) -> anyhow::Result<()> {
    run_tui_with_opts(config, false).await
}

pub async fn run_tui_with_opts(config: Config, legacy: bool) -> anyhow::Result<()> {
    let svc_data_dir = config
        .config_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| config.workspace_dir.join(".senweavercoding"));
    let _ = crate::services::init_services(crate::services::container::ServiceContainerConfig {
        data_dir: svc_data_dir,
        ..Default::default()
    });

    let bridge = agent_bridge::spawn_agent_task(config.clone());
    run_tui_inner(config, bridge, legacy).await
}

pub async fn run_tui_standalone() -> anyhow::Result<()> {
    run_tui_standalone_with_opts(false).await
}

pub async fn run_tui_standalone_with_opts(legacy: bool) -> anyhow::Result<()> {
    let config = crate::Config::load_or_init().await?;

    let cwd = std::env::current_dir().unwrap_or_default();
    crate::bootstrap::init_state(cwd);

    let svc_cfg = crate::services::container::ServiceContainerConfig::default();
    let _ = crate::services::init_services(svc_cfg);

    let bridge = agent_bridge::spawn_agent_task(config.clone());
    run_tui_inner(config, bridge, legacy).await
}

struct TerminalGuard;

impl TerminalGuard {
    fn new() -> Self {
        Self
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    }
}

async fn run_tui_inner(
    config: Config,
    bridge: agent_bridge::AgentBridge,
    legacy: bool,
) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let _guard = TerminalGuard::new();
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config, bridge);

    if event_loop::is_legacy_loop_enabled(legacy) {
        crate::observability::tui_metrics::incr_tui_legacy_loop_activated();
        tracing::info!(
            target: "tui.event_loop",
            "TUI main loop = legacy (100ms poll; --tui-legacy or TUI_LEGACY_LOOP active)"
        );
        run_legacy_loop(&mut terminal, &mut app).await?;
    } else {
        tracing::info!(
            target: "tui.event_loop",
            "TUI main loop = event-driven (spawn_blocking input + 16ms tick)"
        );
        run_event_driven_loop(&mut terminal, &mut app).await?;
    }

    terminal.show_cursor()?;

    Ok(())
}

async fn run_event_driven_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    use crate::observability::tui_metrics;

    let mut input = event_loop::spawn_input_thread();
    let tick_period = Duration::from_millis(16);
    let mut tick = tokio::time::interval(tick_period);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let one_second = Duration::from_secs(1);
    let mut last_second = Instant::now();

    loop {
        if app.dirty {
            if app.partial_redraw_pending && app.active_tab == Tab::Chat {

                terminal.draw(|f| draw_chat_partial(f, app))?;
            } else {
                terminal.draw(|f| draw(f, app))?;
            }
            app.dirty = false;
            app.partial_redraw_pending = false;
            tui_metrics::incr_tui_frame_draws();
            if let Some(started) = app.pending_delta_started_at.take() {
                let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                tui_metrics::observe_tui_streaming_token_latency_ms(elapsed_ms);
            }
        } else {
            tui_metrics::incr_tui_frame_skipped_dirty();
        }

        if app.should_quit {
            break;
        }

        tokio::select! {
            biased;
            maybe_input = input.rx.recv() => {
                match maybe_input {
                    Some(ev) => {
                        tui_metrics::incr_tui_input_events();
                        handle_crossterm_event(app, terminal, ev)?;

                        app.partial_redraw_pending = false;
                        app.mark_dirty();
                    }
                    None => {

                        tracing::warn!(
                            target: "tui.event_loop",
                            "input thread channel closed; exiting TUI"
                        );
                        break;
                    }
                }
            }
            maybe_agent = app.bridge.receiver.recv() => {
                match maybe_agent {
                    Some(ev) => {
                        tui_metrics::incr_tui_session_deltas();
                        if app.pending_delta_started_at.is_none() {
                            app.pending_delta_started_at = Some(Instant::now());
                        }

                        let first_is_stream = matches!(
                            ev,
                            agent_bridge::AgentEvent::StreamChunk(_)
                                | agent_bridge::AgentEvent::ThinkingChunk(_)
                        );
                        match &ev {
                            agent_bridge::AgentEvent::Done
                            | agent_bridge::AgentEvent::Error(_) => app.bridge.is_busy = false,
                            agent_bridge::AgentEvent::Thinking
                            | agent_bridge::AgentEvent::ThinkingChunk(_) => {
                                app.bridge.is_busy = true;
                            }
                            _ => {}
                        }
                        app.handle_agent_event(ev);

                        let before_msgs = app.chat_messages.len();
                        let drained = app.bridge.poll_events();
                        let all_stream = first_is_stream
                            && drained.iter().all(|e| {
                                matches!(
                                    e,
                                    agent_bridge::AgentEvent::StreamChunk(_)
                                        | agent_bridge::AgentEvent::ThinkingChunk(_)
                                )
                            });
                        for dev in drained {
                            match &dev {
                                agent_bridge::AgentEvent::Done
                                | agent_bridge::AgentEvent::Error(_) => {
                                    app.bridge.is_busy = false;
                                }
                                agent_bridge::AgentEvent::Thinking
                                | agent_bridge::AgentEvent::ThinkingChunk(_) => {
                                    app.bridge.is_busy = true;
                                }
                                _ => {}
                            }
                            app.handle_agent_event(dev);
                        }
                        app.reconcile_chat_messages();

                        if all_stream && app.chat_messages.len() == before_msgs {
                            if app.active_tab == Tab::Chat {
                                app.partial_redraw_pending = true;
                                app.mark_dirty();
                            }

                        } else {
                            app.partial_redraw_pending = false;
                            app.mark_dirty();
                        }
                    }
                    None => {

                        app.chat_messages.push(ChatMessage::with_role_now(
                            "error",
                            "agent bridge disconnected; press Ctrl+Q to exit".into(),
                        ));
                        app.partial_redraw_pending = false;
                        app.mark_dirty();
                        break;
                    }
                }
            }
            _ = tick.tick() => {

                if last_second.elapsed() >= one_second {
                    app.tick();
                    last_second = Instant::now();
                    app.partial_redraw_pending = false;
                    app.mark_dirty();
                }

                let before_msgs = app.chat_messages.len();
                let before_buf = app.streaming_buffer.len();
                app.drain_agent_events();
                let after_msgs = app.chat_messages.len();
                let after_buf = app.streaming_buffer.len();
                if after_msgs != before_msgs || after_buf != before_buf {
                    if app.pending_delta_started_at.is_none() {
                        app.pending_delta_started_at = Some(Instant::now());
                    }

                    let stream_only =
                        after_msgs == before_msgs && after_buf != before_buf;
                    if stream_only && app.active_tab == Tab::Chat {
                        app.partial_redraw_pending = true;
                        app.mark_dirty();
                    } else if !stream_only {
                        app.partial_redraw_pending = false;
                        app.mark_dirty();
                    }

                }
            }
        }
    }

    drop(input);

    Ok(())
}

async fn run_legacy_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    let tick_rate = Duration::from_secs(1);
    let poll_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| draw(f, app))?;
        crate::observability::tui_metrics::incr_tui_frame_draws();

        let timeout = if app.bridge.is_busy {
            poll_rate
        } else {
            tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or(Duration::from_millis(0))
        };

        if event::poll(timeout)? {
            let ev = event::read()?;
            handle_crossterm_event(app, terminal, ev)?;
            crate::observability::tui_metrics::incr_tui_input_events();
        }

        app.drain_agent_events();

        if last_tick.elapsed() >= tick_rate {
            app.tick();
            last_tick = Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn handle_crossterm_event(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ev: Event,
) -> anyhow::Result<()> {
    match ev {
        Event::Key(key) => app.handle_key(key),
        Event::Mouse(mouse) => match mouse.kind {
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                if mouse.row < 3 {
                    let tab_count = Tab::all().len() as u16;
                    if tab_count > 0 {
                        let tab_w = terminal.size()?.width / tab_count;
                        let idx = (mouse.column / tab_w.max(1)) as usize;
                        if idx < Tab::all().len() {
                            app.active_tab = Tab::all()[idx];
                        }
                    }
                }
            }
            crossterm::event::MouseEventKind::ScrollDown => {
                if app.active_tab == Tab::Chat && app.chat_scroll_offset > 0 {
                    app.chat_scroll_offset = app.chat_scroll_offset.saturating_sub(3);
                }
            }
            crossterm::event::MouseEventKind::ScrollUp => {
                if app.active_tab == Tab::Chat {
                    app.chat_scroll_offset = (app.chat_scroll_offset + 3)
                        .min(app.chat_messages.len().saturating_sub(1));
                }
            }
            _ => {}
        },
        Event::Resize(_, _) => {
            terminal.clear()?;
        }
        _ => {}
    }
    Ok(())
}
