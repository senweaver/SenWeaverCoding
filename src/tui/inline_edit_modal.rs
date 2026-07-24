// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;

use crossterm::event::{self, KeyCode};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalField {
    Path,
    Instruction,
}

#[derive(Debug)]
pub struct InlineEditModal {
    pub is_open: bool,
    pub path: String,
    pub instruction: String,
    pub focus: ModalField,
    pub status: Option<String>,
    pub submitting: bool,

    pub last_batch_id: Option<String>,
}

impl Default for InlineEditModal {
    fn default() -> Self {
        Self {
            is_open: false,
            path: String::new(),
            instruction: String::new(),
            focus: ModalField::Path,
            status: None,
            submitting: false,
            last_batch_id: None,
        }
    }
}

impl InlineEditModal {
    pub fn open_with_path(&mut self, path: Option<PathBuf>) {
        self.is_open = true;
        self.submitting = false;
        self.focus = if path.is_some() {
            ModalField::Instruction
        } else {
            ModalField::Path
        };
        if let Some(p) = path {
            self.path = p.display().to_string();
        }
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.submitting = false;
        self.instruction.clear();
        self.status = None;
    }
}

#[derive(Debug, Clone)]
pub enum ModalAction {
    Noop,
    Submit {
        path: PathBuf,
        instruction: String,
    },
    Close,
}

pub fn draw(f: &mut Frame, modal: &InlineEditModal, area: Rect) {
    if !modal.is_open {
        return;
    }
    let width = (area.width as i32 - 20).max(40) as u16;
    let height: u16 = 10;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect {
        x,
        y,
        width,
        height,
    };

    f.render_widget(Clear, rect);

    let path_style = if modal.focus == ModalField::Path {
        Style::default().fg(Color::Black).bg(Color::Cyan)
    } else {
        Style::default().fg(Color::Gray)
    };
    let instr_style = if modal.focus == ModalField::Instruction {
        Style::default().fg(Color::Black).bg(Color::Cyan)
    } else {
        Style::default().fg(Color::Gray)
    };
    let mut lines: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            "  Inline Edit (Ctrl+K)",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  path: ", Style::default().fg(Color::DarkGray)),
            Span::styled(modal.path.clone(), path_style),
        ]),
        Line::from(vec![
            Span::styled("  inst: ", Style::default().fg(Color::DarkGray)),
            Span::styled(modal.instruction.clone(), instr_style),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Tab: switch field   Enter: submit   Esc: cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    if let Some(status) = modal.status.as_deref() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {status}"),
            Style::default().fg(Color::Yellow),
        )));
    }
    if let Some(batch) = modal.last_batch_id.as_deref() {
        lines.push(Line::from(Span::styled(
            format!("  last batch: {batch}"),
            Style::default().fg(Color::Green),
        )));
    }
    let block = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Inline Edit "),
        );
    f.render_widget(block, rect);
}

pub fn handle_key(modal: &mut InlineEditModal, key: event::KeyEvent) -> ModalAction {
    if !modal.is_open {
        return ModalAction::Noop;
    }
    match key.code {
        KeyCode::Esc => {
            modal.close();
            ModalAction::Close
        }
        KeyCode::Tab => {
            modal.focus = match modal.focus {
                ModalField::Path => ModalField::Instruction,
                ModalField::Instruction => ModalField::Path,
            };
            ModalAction::Noop
        }
        KeyCode::Enter => {
            if modal.submitting {
                return ModalAction::Noop;
            }
            let path = modal.path.trim();
            let instruction = modal.instruction.trim();
            if path.is_empty() || instruction.is_empty() {
                modal.status = Some("path and instruction are required".into());
                return ModalAction::Noop;
            }
            ModalAction::Submit {
                path: PathBuf::from(path),
                instruction: instruction.to_string(),
            }
        }
        KeyCode::Backspace => {
            match modal.focus {
                ModalField::Path => {
                    super::pop_last_char(&mut modal.path);
                }
                ModalField::Instruction => {
                    super::pop_last_char(&mut modal.instruction);
                }
            }
            ModalAction::Noop
        }
        KeyCode::Char(c) => {
            match modal.focus {
                ModalField::Path => modal.path.push(c),
                ModalField::Instruction => modal.instruction.push(c),
            }
            ModalAction::Noop
        }
        _ => ModalAction::Noop,
    }
}

pub fn build_agent_prompt(path: &PathBuf, instruction: &str) -> String {
    format!(
        "请对文件 `{path}` 执行以下内联修改：\n\n{instruction}\n\n\
         请使用 file_edit / multi_edit 等写入工具真实落盘，\
         完成后简要说明修改了哪些位置。",
        path = path.display()
    )
}

#[derive(Debug, Clone)]
pub struct RunnerSubmitOutcome {
    pub path: PathBuf,
    pub diff: String,
    pub additions: i32,
    pub deletions: i32,
    pub validator_issues: Vec<String>,
    pub checkpoint_id: Option<String>,
    pub edit_batch_id: Option<String>,
}

pub async fn run_through_runner(
    runner: &crate::inline_edit::InlineEditRunner,
    workspace_dir: PathBuf,
    path: PathBuf,
    instruction: String,
) -> Result<RunnerSubmitOutcome, anyhow::Error> {
    let source = match tokio::fs::read_to_string(&path).await {
        Ok(s) => s,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "failed to read source file {}: {e}",
                path.display()
            ));
        }
    };
    let len = source.len();
    let description = instruction.chars().take(120).collect::<String>();
    let req = crate::inline_edit::InlineEditRequest {
        file_path: path.clone(),
        selection: source.clone(),
        selection_bytes: (0, len),
        instruction,
        context_lines: None,
        request_id: uuid::Uuid::new_v4(),
    };
    let outcome = runner
        .run(&source, req)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let (additions, deletions) = count_diff_lines(&outcome.diff);

    let history = crate::tools::edit_history::EditHistory::shared_for_workspace(&workspace_dir);
    let snapshot_recorded = {
        let history = history.clone();
        let snap_path = path.clone();
        tokio::task::spawn_blocking(move || {
            history.snapshot_before_write(&snap_path, "inline_edit", &description)
        })
        .await
        .map(|r| r.is_ok())
        .unwrap_or(false)
    };
    if !snapshot_recorded {
        tracing::warn!(
            path = %path.display(),
            "inline-edit: failed to record edit-history snapshot before write"
        );
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent).await.ok();
    }

    let batch = crate::apply_model::edit_op::EditBatch::new(
        crate::apply_model::edit_op::EditOrigin::InlineEdit,
    )
    .with_op(crate::apply_model::edit_op::EditOp::Replace {
        path: path.clone(),
        byte_range: 0..len,
        old_text: source.clone(),
        new_text: outcome.applied.clone(),
        anchor: None,
    });
    let applier =
        crate::apply_model::ops_applier::OpsApplier::locked_for_workspace(workspace_dir.clone());
    let batch_outcome = applier.apply_batch(batch).await.map_err(|e| {
        anyhow::anyhow!(
            "inline-edit apply failed for {} (file may have changed on disk): {e}",
            path.display()
        )
    })?;

    Ok(RunnerSubmitOutcome {
        path,
        diff: outcome.diff,
        additions,
        deletions,
        validator_issues: outcome.validator_issues,
        checkpoint_id: outcome.checkpoint_id,
        edit_batch_id: Some(batch_outcome.batch_id),
    })
}

fn count_diff_lines(diff: &str) -> (i32, i32) {
    let mut additions = 0i32;
    let mut deletions = 0i32;
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if let Some(rest) = line.strip_prefix('+') {

            if !rest.starts_with('+') {
                additions += 1;
            }
        } else if let Some(rest) = line.strip_prefix('-')
            && !rest.starts_with('-')
        {
            deletions += 1;
        }
    }
    (additions, deletions)
}
