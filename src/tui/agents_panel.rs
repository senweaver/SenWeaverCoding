// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};
use ratatui::{Frame, text::Line};

use crate::agent::multi_agent_runtime::{MultiAgentRuntime, RuntimeHealthSummary};

pub struct AgentsPanelState {
    pub summary: RuntimeHealthSummary,
    pub agents: Vec<AgentRow>,
    pub blackboard_peek: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AgentRow {
    pub id: String,
    pub name: String,
    pub role: String,
    pub state: String,
    pub tasks_completed: u64,
    pub tasks_failed: u64,
}

impl AgentsPanelState {

    pub fn from_global() -> Option<Self> {
        let rt = crate::agent::multi_agent_runtime::global_runtime()?;
        Some(Self::from_runtime(&rt))
    }

    pub fn from_runtime(rt: &MultiAgentRuntime) -> Self {
        let summary = rt.health_summary();
        let agents = rt
            .registry
            .all()
            .into_iter()
            .map(|a| AgentRow {
                id: a.id.clone(),
                name: a.name.clone(),
                role: a.role.clone(),
                state: format!("{:?}", a.state),
                tasks_completed: a.tasks_completed,
                tasks_failed: a.tasks_failed,
            })
            .collect();

        let mut blackboard_peek: Vec<String> = Vec::new();
        for ns in ["project", "task_results", "default"] {
            for k in rt.blackboard.inner().keys_in_namespace(ns) {
                blackboard_peek.push(format!("{}/{}", ns, k));
                if blackboard_peek.len() >= 10 {
                    break;
                }
            }
            if blackboard_peek.len() >= 10 {
                break;
            }
        }
        Self {
            summary,
            agents,
            blackboard_peek,
        }
    }
}

pub fn render(frame: &mut Frame<'_>, area: ratatui::layout::Rect, state: &AgentsPanelState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(5),
            Constraint::Length(6),
        ])
        .split(area);

    let summary_lines = vec![
        Line::from(format!(
            "Agents: total={} healthy={} unhealthy={}",
            state.summary.total_agents,
            state.summary.healthy_agents,
            state.summary.unhealthy_agents,
        )),
        Line::from(format!(
            "Tasks: pending={} running={}",
            state.summary.pending_tasks, state.summary.running_tasks,
        )),
        Line::from(format!(
            "Blackboard: {} entries",
            state.summary.blackboard_entries
        )),
    ];
    let summary = Paragraph::new(summary_lines).block(
        Block::default()
            .title("Multi-Agent Runtime")
            .borders(Borders::ALL),
    );
    frame.render_widget(summary, chunks[0]);

    let header = Row::new(vec!["ID", "Name", "Role", "State", "✓", "✗"])
        .style(Style::default().add_modifier(Modifier::BOLD));
    let rows: Vec<Row> = state
        .agents
        .iter()
        .map(|a| {
            Row::new(vec![
                a.id.clone(),
                a.name.clone(),
                a.role.clone(),
                a.state.clone(),
                a.tasks_completed.to_string(),
                a.tasks_failed.to_string(),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Length(20),
            Constraint::Length(14),
            Constraint::Length(12),
            Constraint::Length(5),
            Constraint::Length(5),
        ],
    )
    .header(header)
    .block(Block::default().title("Agents").borders(Borders::ALL));
    frame.render_widget(table, chunks[1]);

    let lines: Vec<Line> = if state.blackboard_peek.is_empty() {
        vec![Line::from("(no entries)")]
    } else {
        state
            .blackboard_peek
            .iter()
            .map(|k| Line::from(format!("• {k}")))
            .collect()
    };
    let bb = Paragraph::new(lines).block(
        Block::default()
            .title("Blackboard (latest keys)")
            .borders(Borders::ALL),
    );
    frame.render_widget(bb, chunks[2]);
}
