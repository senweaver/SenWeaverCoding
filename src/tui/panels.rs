// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! TUI renderers for the multi-agent observability
//! panels.
//!
//! Both the GUI (egui) and this TUI (ratatui) consume the same
//! UI-framework-free view models that live in
//! [`crate::observability::views`]
//! ([`BudgetView`](crate::observability::views::BudgetView) and
//! [`ProviderHealthView`](crate::observability::views::ProviderHealthView)).
//! That keeps the CLI-friendly TUI at parity with the GUI without
//! duplicating the data-shaping logic.
//!
//! The renderers are deliberately read-only — a live copy of each
//! view model is passed by reference and drawn into the supplied
//! [`ratatui::layout::Rect`].  Update cadence and data sourcing are
//! owned by the TUI app shell (see `tui::mod.rs`).

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table};

use crate::observability::views::{BudgetRow, BudgetView, ProviderHealthRow, ProviderHealthView};

pub fn render_budget(f: &mut Frame, area: Rect, view: &BudgetView) {
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(3)])
        .split(area);

    let header = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Budget — {} ", view.header_line())),
        )
        .ratio((view.usage_ratio() as f64).clamp(0.0, 1.0))
        .gauge_style(Style::default().fg(Color::Green).bg(Color::Black))
        .label(format!(
            "{} used · {} remaining",
            view.used(),
            view.remaining()
        ));
    f.render_widget(header, chunks[0]);

    let rows: Vec<Row> = view
        .rows()
        .iter()
        .map(
            |BudgetRow {
                 segment,
                 reserved,
                 share,
             }| {
                let pct = (share * 100.0).round() as u32;
                let bar_width: usize = (share * 20.0).round() as usize;
                let bar_width = bar_width.clamp(0, 20);
                let bar: String = "█".repeat(bar_width);
                Row::new(vec![
                    Cell::from(segment.clone()),
                    Cell::from(format!("{}", reserved)),
                    Cell::from(Line::from(vec![
                        Span::styled(bar, Style::default().fg(Color::Cyan)),
                        Span::raw(format!(" {pct}%")),
                    ])),
                ])
            },
        )
        .collect();

    let widths = [
        Constraint::Percentage(30),
        Constraint::Length(10),
        Constraint::Percentage(60),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec!["segment", "reserved", "share"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Per-segment reservations "),
        );
    f.render_widget(table, chunks[1]);
}

pub fn render_provider_health(f: &mut Frame, area: Rect, view: &ProviderHealthView) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Provider health — {} ", view.header_line()));

    if view.row_count() == 0 {
        let para = Paragraph::new("No provider health signals yet.").block(block);
        f.render_widget(para, area);
        return;
    }

    let rows: Vec<Row> = view.rows().iter().map(|r| row_for_health(r)).collect();

    let widths = [
        Constraint::Length(14),
        Constraint::Length(22),
        Constraint::Length(9),
        Constraint::Length(8),
        Constraint::Length(10),
        Constraint::Length(10),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec![
                "provider", "model", "success", "p95 ms", "retries", "$/1k tok",
            ])
            .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(block);
    f.render_widget(table, area);
}

fn row_for_health(r: &ProviderHealthRow) -> Row<'static> {
    let style = if r.is_unhealthy() {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::Green)
    };
    Row::new(vec![
        Cell::from(r.provider.clone()).style(style),
        Cell::from(r.model.clone()),
        Cell::from(format!("{:.2}", r.success_rate)),
        Cell::from(format!("{}", r.p95_latency_ms)),
        Cell::from(format!("{:.2}", r.retries_per_req)),
        Cell::from(format!("{:.4}", r.cost_per_1k_tok)),
    ])
}
