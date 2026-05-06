// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! FlowCanvas renderer.
//!
//! `FlowCanvas` turns a stream of [`SessionEvent`]s into a compact,
//! deterministic, text-based representation of the agent's execution
//! graph.  The same renderer is consumed by:
//!
//! * CLI: `sen session flow` command — prints the canvas to stdout.
//! * TUI: `ratatui::widgets::Paragraph` — shown in the "Flow" pane.
//! * GUI: `egui::TextEdit` — shown in the "Flow" panel.
//!
//! Keeping the renderer UI-framework-agnostic (it returns a `String`)
//! is what unlocks three-end parity: the same 20-line ASCII tree is
//! identical across CLI / TUI / GUI, so parity tests can assert
//! byte-equality.
//!
//! The canvas tracks three logical layers:
//!
//! ```text
//!   Turn N
//!     ├─ [plan]   <goal> (4 steps)
//!     │    ├─ 1. read_file        ✓
//!     │    ├─ 2. apply_diff       ✓
//!     │    ├─ 3. run_command      ✓
//!     │    └─ 4. verify           passed
//!     ├─ [tool]   <name> → ok
//!     └─ [diff]   apply 3 files   (then rolled back)
//! ```
//!
//! The renderer never allocates unboundedly — it caps the number of
//! turns kept at [`MAX_TURNS`] (default 50) so long sessions do not
//! leak memory.

use std::collections::VecDeque;

use crate::session::event::{SessionEvent, SessionEventKind};

pub const MAX_TURNS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowNode {
    Turn {
        index: u32,
        input: String,
    },
    Plan {
        goal: String,
        summary: String,
        steps_expected: u32,
    },
    Step {
        index: u32,
        label: String,
        status: StepStatus,
        summary: String,
    },
    Verify {
        status: String,
    },
    Tool {
        name: String,
        ok: bool,
    },
    DiffApplied {
        files: u32,
        hunks_exact: u32,
        hunks_fuzzy: u32,
    },
    DiffRolledBack {
        files: u32,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    Ok,
    Fail,
}

#[derive(Debug, Default)]
pub struct FlowCanvas {
    turns: VecDeque<TurnBuffer>,
}

#[derive(Debug)]
struct TurnBuffer {
    index: u32,
    input: String,
    children: Vec<FlowNode>,
}

impl FlowCanvas {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ingest(&mut self, event: &SessionEvent) {
        match &event.kind {
            SessionEventKind::TurnStarted { input } => {
                let index = self.turns.back().map_or(1, |t| t.index + 1);
                if self.turns.len() >= MAX_TURNS {
                    self.turns.pop_front();
                }
                self.turns.push_back(TurnBuffer {
                    index,
                    input: input.clone(),
                    children: Vec::new(),
                });
            }
            SessionEventKind::ToolCall { tool_name, .. } => {
                self.push_child(FlowNode::Tool {
                    name: tool_name.clone(),
                    ok: true,
                });
            }
            SessionEventKind::ToolResult { is_error, .. } => {
                if let Some(last) = self.last_mut() {
                    if let Some(FlowNode::Tool { ok, .. }) = last.children.last_mut() {
                        *ok = !is_error;
                    }
                }
            }
            SessionEventKind::WritePlanCreated {
                goal,
                summary,
                steps,
            } => {
                self.push_child(FlowNode::Plan {
                    goal: goal.clone(),
                    summary: summary.clone(),
                    steps_expected: *steps,
                });
            }
            SessionEventKind::WriteStepStarted { index, label } => {
                self.push_child(FlowNode::Step {
                    index: *index,
                    label: label.clone(),
                    status: StepStatus::Pending,
                    summary: String::new(),
                });
            }
            SessionEventKind::WriteStepFinished {
                index,
                label: _,
                ok,
                summary,
            } => {
                if let Some(last) = self.last_mut() {
                    for node in last.children.iter_mut().rev() {
                        if let FlowNode::Step {
                            index: existing_index,
                            status,
                            summary: existing_summary,
                            ..
                        } = node
                        {
                            if *existing_index == *index {
                                *status = if *ok {
                                    StepStatus::Ok
                                } else {
                                    StepStatus::Fail
                                };
                                *existing_summary = summary.clone();
                                return;
                            }
                        }
                    }
                }
            }
            SessionEventKind::WriteVerify { status } => {
                self.push_child(FlowNode::Verify {
                    status: status.clone(),
                });
            }
            SessionEventKind::DiffSessionApplied {
                files,
                hunks_exact,
                hunks_fuzzy,
            } => {
                self.push_child(FlowNode::DiffApplied {
                    files: *files,
                    hunks_exact: *hunks_exact,
                    hunks_fuzzy: *hunks_fuzzy,
                });
            }
            SessionEventKind::DiffSessionRolledBack { files } => {
                self.push_child(FlowNode::DiffRolledBack { files: *files });
            }
            SessionEventKind::Error { message } => {
                self.push_child(FlowNode::Error {
                    message: message.clone(),
                });
            }
            _ => {}
        }
    }

    fn last_mut(&mut self) -> Option<&mut TurnBuffer> {
        self.turns.back_mut()
    }

    fn push_child(&mut self, node: FlowNode) {
        if self.turns.is_empty() {
            self.turns.push_back(TurnBuffer {
                index: 1,
                input: String::from("(no turn)"),
                children: Vec::new(),
            });
        }
        self.last_mut().unwrap().children.push(node);
    }

    #[must_use]
    pub fn turns(&self) -> usize {
        self.turns.len()
    }

    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        for (turn_idx, turn) in self.turns.iter().enumerate() {
            if turn_idx > 0 {
                out.push('\n');
            }
            out.push_str(&format!("Turn {}: {}\n", turn.index, one_line(&turn.input)));
            let n = turn.children.len();
            for (i, child) in turn.children.iter().enumerate() {
                let branch = if i + 1 == n { "└─" } else { "├─" };
                render_child(&mut out, branch, child);
            }
        }
        out
    }
}

fn render_child(out: &mut String, branch: &str, node: &FlowNode) {
    match node {
        FlowNode::Turn { .. } => {}
        FlowNode::Plan {
            goal,
            summary,
            steps_expected,
        } => {
            out.push_str(&format!(
                "  {branch} [plan] {}  ({steps} steps, {sum})\n",
                one_line(goal),
                steps = steps_expected,
                sum = summary
            ));
        }
        FlowNode::Step {
            index,
            label,
            status,
            summary,
        } => {
            let mark = match status {
                StepStatus::Ok => "ok",
                StepStatus::Fail => "FAIL",
                StepStatus::Pending => "…",
            };
            out.push_str(&format!(
                "     {branch} {}. {:<12} {mark}{}\n",
                index,
                label,
                if summary.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", one_line(summary))
                }
            ));
        }
        FlowNode::Verify { status } => {
            out.push_str(&format!("     {branch} verify       {status}\n"));
        }
        FlowNode::Tool { name, ok } => {
            out.push_str(&format!(
                "  {branch} [tool] {name} → {}\n",
                if *ok { "ok" } else { "FAIL" }
            ));
        }
        FlowNode::DiffApplied {
            files,
            hunks_exact,
            hunks_fuzzy,
        } => {
            out.push_str(&format!(
                "  {branch} [diff] applied {files} files ({hunks_exact} exact, {hunks_fuzzy} fuzzy)\n",
            ));
        }
        FlowNode::DiffRolledBack { files } => {
            out.push_str(&format!("  {branch} [diff] rolled back {files} files\n"));
        }
        FlowNode::Error { message } => {
            out.push_str(&format!("  {branch} [err]  {}\n", one_line(message)));
        }
    }
}

fn one_line(s: &str) -> String {
    let trimmed: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    if trimmed.chars().count() > 80 {
        let truncated: String = trimmed.chars().take(77).collect();
        format!("{truncated}...")
    } else {
        trimmed
    }
}
