// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Terminal User Interface (TUI) for SenWeaverCoding.
//!
//! Provides a rich terminal dashboard with multiple tabs:
//! - **Dashboard** - System status overview
//! - **Chat** - Interactive agent conversation
//! - **Memory** - Memory entries browser
//! - **Channels** - Channel status and management
//! - **Events** - Event bus monitor
//! - **Logs** - Real-time log viewer
//!
//! Enable with `--features tui` and run with `sen tui`.

pub mod agent_bridge;
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
use crate::services::ServiceContainer;

/// Non-panicking accessor for bootstrap state (returns None if not yet initialized).
fn try_get_bootstrap_state() -> Option<&'static BootstrapState> {
    std::panic::catch_unwind(crate::bootstrap::get_state).ok()
}

/// Non-panicking accessor for ServiceContainer (returns None if not yet initialized).
fn try_get_services() -> Option<&'static ServiceContainer> {
    std::panic::catch_unwind(crate::services::get_services).ok()
}

/// Active tab in the TUI.
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
    Events,
    Logs,
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
            Tab::Events,
            Tab::Logs,
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
            Tab::Events => "Events",
            Tab::Logs => "Logs",
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
            Tab::Events => 8,
            Tab::Logs => 9,
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
            8 => Tab::Events,
            9 => Tab::Logs,
            _ => Tab::Dashboard,
        }
    }
}

/// TUI application state.
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
}

/// Keyboard shortcut definitions for the help panel
pub struct KeyShortcut {
    pub key: &'static str,
    pub description: &'static str,
}

impl App {
    /// Get all available keyboard shortcuts
    pub fn get_shortcuts() -> Vec<KeyShortcut> {
        vec![
            KeyShortcut { key: "Ctrl+Q / Ctrl+C", description: "Quit application" },
            KeyShortcut { key: "Tab / Shift+Tab", description: "Next / Previous tab" },
            KeyShortcut { key: "F1-F10", description: "Switch to specific tab" },
            KeyShortcut { key: "?", description: "Toggle this help panel" },
            KeyShortcut { key: "j / k", description: "Navigate up / down" },
            KeyShortcut { key: "Enter", description: "Send message / Execute" },
            KeyShortcut { key: "Esc", description: "Cancel current operation" },
            KeyShortcut { key: "Ctrl+L", description: "Clear chat" },
        ]
    }
}

/// A chat message in the TUI.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

/// A memory entry for display.
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub key: String,
    pub category: String,
    pub preview: String,
}

/// System status information.
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

/// A background task entry for display.
#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub id: String,
    pub task_type: String,
    pub status: String,
    pub description: String,
    pub duration_ms: u64,
}

/// A tool entry for display.
#[derive(Debug, Clone)]
pub struct ToolEntry {
    pub name: String,
    pub category: String,
    pub call_count: u32,
    pub enabled: bool,
}

/// A slash command entry for display.
#[derive(Debug, Clone)]
pub struct CommandEntry {
    pub name: String,
    pub description: String,
    pub category: String,
    pub usage: String,
}

/// Detailed cost breakdown for the Cost tab.
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

/// Per-model cost breakdown.
#[derive(Debug, Clone)]
pub struct ModelCostEntry {
    pub model_name: String,
    pub cost_usd: f64,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl App {
    /// Create a new TUI app with the given config and agent bridge.
    pub fn new(config: Config, bridge: agent_bridge::AgentBridge) -> Self {
        let provider = config
            .default_provider
            .clone()
            .unwrap_or_else(|| "none".into());
        let model = config
            .default_model
            .clone()
            .unwrap_or_else(|| "none".into());

        Self {
            active_tab: Tab::Dashboard,
            should_quit: false,
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
        }
    }

    /// Handle keyboard input.
    fn handle_key(&mut self, key: event::KeyEvent) {
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
                // Clear chat history
                self.chat_messages.clear();
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
            _ => match self.active_tab {
                Tab::Chat => self.handle_chat_key(key),
                Tab::Memory => self.handle_memory_key(key),
                Tab::Tasks => self.handle_list_key(key, ListTarget::Tasks),
                Tab::Tools => self.handle_list_key(key, ListTarget::Tools),
                Tab::Commands => self.handle_list_key(key, ListTarget::Commands),
                _ => {}
            },
        }
    }

    fn handle_chat_key(&mut self, key: event::KeyEvent) {
        match key.code {
            KeyCode::Char(c) => self.chat_input.push(c),
            KeyCode::Backspace => {
                self.chat_input.pop();
            }
            KeyCode::Esc if self.bridge.is_busy => {
                let _ = self.bridge.send(agent_bridge::UserInput::Cancel);
            }
            KeyCode::Enter => {
                if !self.chat_input.is_empty() {
                    let msg = self.chat_input.clone();
                    self.chat_input.clear();

                    if let Some(cmd) = msg.strip_prefix('/') {
                        let parts: Vec<&str> = cmd.split_whitespace().collect();
                        let name = parts.first().unwrap_or(&"").to_string();
                        let args: Vec<String> =
                            parts.iter().skip(1).map(|s| s.to_string()).collect();
                        self.chat_messages.push(ChatMessage {
                            role: "system".into(),
                            content: format!("/{name} {}", args.join(" ")),
                            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                        });
                        let _ = self
                            .bridge
                            .send(agent_bridge::UserInput::SlashCommand { name, args });
                    } else {
                        self.chat_messages.push(ChatMessage {
                            role: "user".into(),
                            content: msg.clone(),
                            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                        });
                        let _ = self.bridge.send(agent_bridge::UserInput::Chat(msg));
                    }
                }
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
        self.sync_from_services();
    }

    /// Drain all pending agent events and convert them into chat messages.
    fn drain_agent_events(&mut self) {
        let events = self.bridge.poll_events();
        let ts = chrono::Local::now().format("%H:%M:%S").to_string();
        for ev in events {
            match ev {
                agent_bridge::AgentEvent::AssistantMessage(text) => {
                    self.event_entries
                        .push(format!("[{ts}] assistant: response received"));
                    self.chat_messages.push(ChatMessage {
                        role: "assistant".into(),
                        content: text,
                        timestamp: ts.clone(),
                    });
                }
                agent_bridge::AgentEvent::ToolUse { name, id } => {
                    self.event_entries.push(format!("[{ts}] tool_use: {name}"));
                    self.chat_messages.push(ChatMessage {
                        role: "tool".into(),
                        content: format!("▶ {name} ({id})"),
                        timestamp: ts.clone(),
                    });
                }
                agent_bridge::AgentEvent::ToolResult {
                    id,
                    output,
                    success,
                } => {
                    let prefix = if success { "✓" } else { "✗" };
                    self.event_entries
                        .push(format!("[{ts}] tool_result: {prefix} {id}"));
                    self.chat_messages.push(ChatMessage {
                        role: "tool".into(),
                        content: format!("{prefix} [{id}] {output}"),
                        timestamp: ts.clone(),
                    });
                }
                agent_bridge::AgentEvent::Thinking => {
                    self.event_entries.push(format!("[{ts}] thinking"));
                    self.chat_messages.push(ChatMessage {
                        role: "system".into(),
                        content: "Thinking…".into(),
                        timestamp: ts.clone(),
                    });
                }
                agent_bridge::AgentEvent::Done => {
                    self.event_entries.push(format!("[{ts}] done"));
                }
                agent_bridge::AgentEvent::Error(e) => {
                    self.event_entries.push(format!("[{ts}] error: {e}"));
                    self.chat_messages.push(ChatMessage {
                        role: "error".into(),
                        content: e,
                        timestamp: ts.clone(),
                    });
                }
                agent_bridge::AgentEvent::CommandOutput(output) => {
                    self.event_entries.push(format!("[{ts}] command_output"));
                    self.chat_messages.push(ChatMessage {
                        role: "system".into(),
                        content: output,
                        timestamp: ts.clone(),
                    });
                }
            }
        }
    }

    /// Pull live data from bootstrap state and ServiceContainer into TUI state.
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

            // Sync tool usage stats (refresh every 10 ticks)
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

        // Sync background sessions from filesystem (refresh every 5 ticks)
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

        // Sync memory entries from workspace memory files (refresh every 30 ticks)
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

/// Target for generic list key navigation.
enum ListTarget {
    Tasks,
    Tools,
    Commands,
}

/// Draw the TUI frame.
fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // tab bar
            Constraint::Min(0),    // content
            Constraint::Length(1), // status bar
        ])
        .split(f.area());

    draw_tabs(f, app, chunks[0]);

    match app.active_tab {
        Tab::Dashboard => draw_dashboard(f, app, chunks[1]),
        Tab::Chat => draw_chat(f, app, chunks[1]),
        Tab::Memory => draw_memory(f, app, chunks[1]),
        Tab::Channels => draw_channels(f, app, chunks[1]),
        Tab::Tasks => draw_tasks(f, app, chunks[1]),
        Tab::Tools => draw_tools(f, app, chunks[1]),
        Tab::Commands => draw_commands(f, app, chunks[1]),
        Tab::Cost => draw_cost(f, app, chunks[1]),
        Tab::Events => draw_events(f, app, chunks[1]),
        Tab::Logs => draw_logs(f, app, chunks[1]),
    }

    draw_status_bar(f, app, chunks[2]);
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

    // System Info
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

    // Cost tracking
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

    // Channels
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

    // Recent activity
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

fn draw_chat(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let mut messages: Vec<Line> = app
        .chat_messages
        .iter()
        .map(|m| {
            Line::from(vec![
                Span::styled(format!("[{}] ", m.timestamp), theme::dim()),
                Span::styled(format!("{}: ", m.role), theme::style_for_role(&m.role)),
                Span::styled(&m.content, theme::normal()),
            ])
        })
        .collect();

    if app.bridge.is_busy {
        let spinner_idx = (app.tick_count as usize) % theme::SPINNER_FRAMES.len();
        let spinner = theme::SPINNER_FRAMES[spinner_idx];
        messages.push(Line::from(Span::styled(
            format!("{spinner} Agent is thinking… (Esc to cancel)"),
            theme::thinking_style(),
        )));
    }

    let messages_block = Paragraph::new(messages)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Conversation ")
                .title_style(theme::title()),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(messages_block, chunks[0]);

    let input_title = if app.bridge.is_busy {
        " Input (Esc to cancel) "
    } else {
        " Input (Enter to send, / for commands) "
    };
    let input = Paragraph::new(Line::from(vec![
        Span::styled("> ", theme::info_style()),
        Span::styled(&app.chat_input, theme::normal()),
        Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
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

    // Summary section
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

    // Per-model breakdown
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

fn draw_status_bar(f: &mut Frame, _app: &App, area: Rect) {
    let bar = Paragraph::new(Line::from(vec![
        Span::styled(" SenWeaverCoding ", theme::title()),
        Span::styled("| ", theme::dim()),
        Span::styled("Tab/Shift+Tab: switch ", theme::dim()),
        Span::styled("| ", theme::dim()),
        Span::styled("F1-F10: jump ", theme::dim()),
        Span::styled("| ", theme::dim()),
        Span::styled("Ctrl+Q: quit ", theme::dim()),
    ]));
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

/// Run the TUI application.
///
/// This takes over the terminal and displays the interactive dashboard.
/// Press Ctrl+Q or Ctrl+C to exit.
pub async fn run_tui(config: Config) -> anyhow::Result<()> {
    let bridge = agent_bridge::spawn_agent_task(config.clone());
    run_tui_inner(config, bridge).await
}

/// Alternate entrypoint that loads config internally to avoid binary/lib
/// crate type mismatches when called from `main.rs`.
pub async fn run_tui_standalone() -> anyhow::Result<()> {
    let config = crate::Config::load_or_init().await?;

    // Initialize bootstrap state and services so slash commands work
    let cwd = std::env::current_dir().unwrap_or_default();
    crate::bootstrap::init_state(cwd);

    if let Ok(svc_cfg) =
        std::panic::catch_unwind(|| crate::services::container::ServiceContainerConfig::default())
    {
        let _ = std::panic::catch_unwind(|| {
            crate::services::init_services(svc_cfg);
        });
    }

    let bridge = agent_bridge::spawn_agent_task(config.clone());
    run_tui_inner(config, bridge).await
}

async fn run_tui_inner(config: Config, bridge: agent_bridge::AgentBridge) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config, bridge);
    let tick_rate = Duration::from_secs(1);
    let poll_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| draw(f, &mut app))?;

        let timeout = if app.bridge.is_busy {
            poll_rate
        } else {
            tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or(Duration::from_millis(0))
        };

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
            }
        }

        // Always drain agent events between frames
        app.drain_agent_events();

        if last_tick.elapsed() >= tick_rate {
            app.tick();
            last_tick = Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
