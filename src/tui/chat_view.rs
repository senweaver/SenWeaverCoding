// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::session::{ChatEntry, ChatEntryKind, SessionChatState};

pub struct ChatView<'a> {
    pub state: &'a SessionChatState,
    pub show_tools: bool,
    pub show_system: bool,
}

impl<'a> ChatView<'a> {
    pub fn new(state: &'a SessionChatState) -> Self {
        Self {
            state,
            show_tools: true,
            show_system: true,
        }
    }

    pub fn with_show_tools(mut self, show: bool) -> Self {
        self.show_tools = show;
        self
    }

    pub fn with_show_system(mut self, show: bool) -> Self {
        self.show_system = show;
        self
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, title: &str) {
        let entries = self.state.snapshot();
        if entries.is_empty() {
            let para = Paragraph::new("(no messages yet)")
                .block(Block::default().title(title).borders(Borders::ALL))
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(para, area);
            return;
        }

        let filtered: Vec<&ChatEntry> = entries
            .iter()
            .filter(|e| match e.kind {
                ChatEntryKind::ToolCall | ChatEntryKind::ToolResult | ChatEntryKind::ToolError => {
                    self.show_tools
                }
                ChatEntryKind::System => self.show_system,
                _ => true,
            })
            .collect();

        let items: Vec<ListItem<'_>> = filtered
            .iter()
            .flat_map(|entry| render_entry_lines(entry))
            .map(ListItem::new)
            .collect();

        let list = List::new(items).block(Block::default().title(title).borders(Borders::ALL));
        frame.render_widget(list, area);
    }
}

fn render_entry_lines(entry: &ChatEntry) -> Vec<Line<'static>> {
    let (prefix, color, bold) = match entry.kind {
        ChatEntryKind::User => ("▸ you", Color::Cyan, true),
        ChatEntryKind::Assistant => ("◂ sen", Color::Green, false),
        ChatEntryKind::ToolCall => ("⚒ tool", Color::Magenta, false),
        ChatEntryKind::ToolResult => ("✓ result", Color::LightGreen, false),
        ChatEntryKind::ToolError => ("✗ tool-err", Color::Red, true),
        ChatEntryKind::Error => ("✗ error", Color::Red, true),
        ChatEntryKind::System => ("· system", Color::DarkGray, false),
    };
    let mut header_style = Style::default().fg(color);
    if bold {
        header_style = header_style.add_modifier(Modifier::BOLD);
    }

    let header = Span::styled(prefix, header_style);
    let body = Span::raw(format!(" {}", entry.text.replace('\n', " ")));
    vec![Line::from(vec![header, body])]
}

pub fn render_status_strip(frame: &mut Frame<'_>, area: Rect, state: &SessionChatState) {
    let entries = state.snapshot();
    let counts = count_kinds(&entries);

    let line = format!(
        "user:{}  assistant:{}  tool:{}  err:{}  sys:{}  total:{}",
        counts.user,
        counts.assistant,
        counts.tool,
        counts.errors,
        counts.system,
        entries.len()
    );
    let p = Paragraph::new(line)
        .block(Block::default().borders(Borders::ALL).title("chat stats"))
        .wrap(Wrap { trim: true });
    frame.render_widget(p, area);
}

#[derive(Default, Debug, Clone, Copy)]
pub struct KindCounts {
    pub user: usize,
    pub assistant: usize,
    pub tool: usize,
    pub errors: usize,
    pub system: usize,
}

fn count_kinds(entries: &[ChatEntry]) -> KindCounts {
    let mut c = KindCounts::default();
    for e in entries {
        match e.kind {
            ChatEntryKind::User => c.user += 1,
            ChatEntryKind::Assistant => c.assistant += 1,
            ChatEntryKind::ToolCall | ChatEntryKind::ToolResult => c.tool += 1,
            ChatEntryKind::ToolError | ChatEntryKind::Error => c.errors += 1,
            ChatEntryKind::System => c.system += 1,
        }
    }
    c
}
