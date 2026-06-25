// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::Serialize;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;

use super::action::{ActionType, PlannedAction};
use super::capture;
use super::coordinates;
use super::grounding;
use super::input::{self, ClickButton, ScrollDirection};
use super::planner;
use super::vision::VisionClient;
use crate::config::Config;

const MAX_CONSECUTIVE_ERRORS: u32 = 3;
const DEFAULT_WAIT_MS: u64 = 800;
const DEFAULT_SCROLL_AMOUNT: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Thinking,
    Finished,
    CallUser,
    Error,
    Stopped,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComputerStepEvent {
    pub index: u32,
    pub thought: String,
    pub action_type: String,
    pub element_description: Option<String>,
    pub value: Option<String>,
    pub screenshot_base64: String,
    pub target_x_norm: Option<f64>,
    pub target_y_norm: Option<f64>,
    pub to_x_norm: Option<f64>,
    pub to_y_norm: Option<f64>,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ComputerEvent {
    Status {
        status: RunStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    Step {
        #[serde(flatten)]
        step: ComputerStepEvent,
    },
    ActionResult {
        index: u32,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct RunParams {
    pub run_id: String,
    pub task: String,
    pub provider: String,
    pub model: String,
    pub max_steps: u32,
    pub step_delay_ms: u64,
}

pub async fn run_loop(
    params: RunParams,
    config: Config,
    cancel: CancellationToken,
    event_tx: UnboundedSender<ComputerEvent>,
    mut reply_rx: UnboundedReceiver<String>,
) {
    let emit = |event: ComputerEvent| {
        let _ = event_tx.send(event);
    };

    let client = match VisionClient::from_config(&config, &params.provider, &params.model) {
        Ok(client) => client,
        Err(err) => {
            emit(ComputerEvent::Error {
                message: format!("failed to initialize model '{}': {err}", params.model),
            });
            emit(ComputerEvent::Status {
                status: RunStatus::Error,
                message: None,
            });
            return;
        }
    };

    emit(ComputerEvent::Status {
        status: RunStatus::Running,
        message: None,
    });

    let (display_w, display_h) = match input::main_display_size().await {
        Ok((w, h)) if w > 0 && h > 0 => (w, h),
        _ => (0, 0),
    };

    let mut history: Vec<String> = Vec::new();
    let mut consecutive_errors = 0u32;
    let mut consecutive_action_failures = 0u32;
    let mut step: u32 = 0;

    while step < params.max_steps {
        if cancel.is_cancelled() {
            emit(ComputerEvent::Status {
                status: RunStatus::Stopped,
                message: None,
            });
            return;
        }

        let screen = match capture::capture_primary().await {
            Ok(screen) => screen,
            Err(err) => {
                emit(ComputerEvent::Error {
                    message: format!("screen capture failed: {err}"),
                });
                consecutive_errors += 1;
                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    emit(ComputerEvent::Status {
                        status: RunStatus::Error,
                        message: Some("aborted after repeated capture failures".into()),
                    });
                    return;
                }
                sleep_or_cancel(&cancel, 1000).await;
                continue;
            }
        };

        emit(ComputerEvent::Status {
            status: RunStatus::Thinking,
            message: None,
        });

        let data_uri = screen.data_uri();
        let planned = match planner::plan_next(&client, &data_uri, &params.task, &history).await {
            Ok(planned) => planned,
            Err(err) => {
                emit(ComputerEvent::Error {
                    message: format!("planning failed: {err}"),
                });
                consecutive_errors += 1;
                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    emit(ComputerEvent::Status {
                        status: RunStatus::Error,
                        message: Some("aborted after repeated planning failures".into()),
                    });
                    return;
                }
                sleep_or_cancel(&cancel, 800).await;
                continue;
            }
        };
        consecutive_errors = 0;

        let display_w = if display_w > 0 {
            display_w
        } else {
            i32::try_from(screen.width).unwrap_or(1)
        };
        let display_h = if display_h > 0 {
            display_h
        } else {
            i32::try_from(screen.height).unwrap_or(1)
        };

        let mut primary_target: Option<ResolvedTarget> = None;
        let mut secondary_target: Option<ResolvedTarget> = None;

        if planned.action.needs_target() {
            match resolve_target(
                &client,
                &data_uri,
                planned.start_box.as_deref(),
                planned.element_description.as_deref(),
                display_w,
                display_h,
            )
            .await
            {
                Ok(target) => primary_target = Some(target),
                Err(err) => emit(ComputerEvent::Error {
                    message: format!("could not locate target: {err}"),
                }),
            }
            if matches!(planned.action, ActionType::Drag) {
                match resolve_target(
                    &client,
                    &data_uri,
                    planned.end_box.as_deref(),
                    planned.to_element_description.as_deref(),
                    display_w,
                    display_h,
                )
                .await
                {
                    Ok(target) => secondary_target = Some(target),
                    Err(err) => emit(ComputerEvent::Error {
                        message: format!("could not locate drag destination: {err}"),
                    }),
                }
            }
        }

        emit(ComputerEvent::Step {
            step: ComputerStepEvent {
                index: step,
                thought: planned.thought.clone(),
                action_type: planned.action.as_str().to_string(),
                element_description: planned.element_description.clone(),
                value: planned.value.clone(),
                screenshot_base64: screen.png_base64.clone(),
                target_x_norm: primary_target.as_ref().map(|t| t.x_norm),
                target_y_norm: primary_target.as_ref().map(|t| t.y_norm),
                to_x_norm: secondary_target.as_ref().map(|t| t.x_norm),
                to_y_norm: secondary_target.as_ref().map(|t| t.y_norm),
                confidence: primary_target.as_ref().map(|t| t.confidence),
            },
        });

        match planned.action {
            ActionType::Finished => {
                emit(ComputerEvent::Status {
                    status: RunStatus::Finished,
                    message: Some(planned.thought.clone()),
                });
                return;
            }
            ActionType::CallUser => {
                emit(ComputerEvent::Status {
                    status: RunStatus::CallUser,
                    message: Some(planned.thought.clone()),
                });
                tokio::select! {
                    () = cancel.cancelled() => {
                        emit(ComputerEvent::Status { status: RunStatus::Stopped, message: None });
                        return;
                    }
                    reply = reply_rx.recv() => {
                        match reply {
                            Some(text) => {
                                history.push(format!("Asked the user for help; they replied: {text}"));
                                emit(ComputerEvent::Status { status: RunStatus::Running, message: None });
                                continue;
                            }
                            None => {
                                emit(ComputerEvent::Status { status: RunStatus::Stopped, message: None });
                                return;
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        if cancel.is_cancelled() {
            emit(ComputerEvent::Status {
                status: RunStatus::Stopped,
                message: None,
            });
            return;
        }

        let outcome = execute_action(
            &planned,
            display_w,
            display_h,
            primary_target,
            secondary_target,
        )
        .await;
        match outcome {
            Ok(summary) => {
                consecutive_action_failures = 0;
                emit(ComputerEvent::ActionResult {
                    index: step,
                    success: true,
                    message: None,
                });
                history.push(summary);
            }
            Err(err) => {
                consecutive_action_failures += 1;
                emit(ComputerEvent::ActionResult {
                    index: step,
                    success: false,
                    message: Some(err.to_string()),
                });
                history.push(format!(
                    "Attempted {} but it failed: {err}",
                    planned.action.as_str()
                ));
                if consecutive_action_failures >= MAX_CONSECUTIVE_ERRORS {
                    emit(ComputerEvent::Status {
                        status: RunStatus::Error,
                        message: Some(
                            "aborted after repeated action failures; the agent could not make progress"
                                .into(),
                        ),
                    });
                    return;
                }
            }
        }

        step += 1;
        sleep_or_cancel(&cancel, params.step_delay_ms).await;
    }

    emit(ComputerEvent::Status {
        status: RunStatus::Finished,
        message: Some(format!("reached the step limit ({})", params.max_steps)),
    });
}

#[derive(Debug, Clone, Copy)]
struct ResolvedTarget {
    x_norm: f64,
    y_norm: f64,
    confidence: f64,
    input_x: i32,
    input_y: i32,
}

async fn resolve_target(
    client: &VisionClient,
    data_uri: &str,
    coords: Option<&[f64]>,
    description: Option<&str>,
    display_w: i32,
    display_h: i32,
) -> anyhow::Result<ResolvedTarget> {
    if let Some(values) = coords {
        if let Some((x_norm, y_norm)) = coordinates::coords_to_center_norm(values) {
            return Ok(ResolvedTarget::from_norm(
                x_norm, y_norm, 100.0, display_w, display_h,
            ));
        }
    }
    let desc = description
        .ok_or_else(|| anyhow::anyhow!("no target coordinates or description provided"))?;
    let result = grounding::locate(client, data_uri, desc).await?;
    Ok(ResolvedTarget::from_norm(
        result.x_norm,
        result.y_norm,
        result.confidence,
        display_w,
        display_h,
    ))
}

impl ResolvedTarget {
    fn from_norm(
        x_norm: f64,
        y_norm: f64,
        confidence: f64,
        display_w: i32,
        display_h: i32,
    ) -> Self {
        let (input_x, input_y) =
            coordinates::normalized_to_input(x_norm, y_norm, display_w, display_h);
        Self {
            x_norm,
            y_norm,
            confidence,
            input_x,
            input_y,
        }
    }
}

async fn execute_action(
    planned: &PlannedAction,
    display_w: i32,
    display_h: i32,
    primary: Option<ResolvedTarget>,
    secondary: Option<ResolvedTarget>,
) -> anyhow::Result<String> {
    match planned.action {
        ActionType::Click => {
            let target = primary.ok_or_else(|| anyhow::anyhow!("no target located for click"))?;
            input::click(target.input_x, target.input_y, ClickButton::Left, 1).await?;
            Ok(format!(
                "Clicked {}",
                planned.element_description.as_deref().unwrap_or("element")
            ))
        }
        ActionType::DoubleClick => {
            let target =
                primary.ok_or_else(|| anyhow::anyhow!("no target located for double click"))?;
            input::click(target.input_x, target.input_y, ClickButton::Left, 2).await?;
            Ok(format!(
                "Double-clicked {}",
                planned.element_description.as_deref().unwrap_or("element")
            ))
        }
        ActionType::RightClick => {
            let target =
                primary.ok_or_else(|| anyhow::anyhow!("no target located for right click"))?;
            input::click(target.input_x, target.input_y, ClickButton::Right, 1).await?;
            Ok(format!(
                "Right-clicked {}",
                planned.element_description.as_deref().unwrap_or("element")
            ))
        }
        ActionType::MoveMouse => {
            let target =
                primary.ok_or_else(|| anyhow::anyhow!("no target located for move"))?;
            input::move_to(target.input_x, target.input_y).await?;
            Ok(format!(
                "Moved cursor to {}",
                planned.element_description.as_deref().unwrap_or("element")
            ))
        }
        ActionType::Type => {
            let text = planned
                .value
                .clone()
                .ok_or_else(|| anyhow::anyhow!("type action requires a value"))?;
            input::type_text(text.clone()).await?;
            Ok(format!("Typed \"{text}\""))
        }
        ActionType::KeyPress => {
            let combo = planned
                .value
                .clone()
                .ok_or_else(|| anyhow::anyhow!("key_press action requires a value"))?;
            input::key_combo(combo.clone()).await?;
            Ok(format!("Pressed {combo}"))
        }
        ActionType::Scroll => {
            let (x, y) = match primary {
                Some(target) => (target.input_x, target.input_y),
                None => (display_w / 2, display_h / 2),
            };
            let direction = parse_scroll_direction(planned.value.as_deref());
            let amount = planned.amount.unwrap_or(DEFAULT_SCROLL_AMOUNT);
            input::scroll(x, y, direction, amount).await?;
            Ok(format!("Scrolled {:?}", direction))
        }
        ActionType::Drag => {
            let from = primary.ok_or_else(|| anyhow::anyhow!("no source located for drag"))?;
            let to = secondary.ok_or_else(|| anyhow::anyhow!("no destination located for drag"))?;
            input::drag(from.input_x, from.input_y, to.input_x, to.input_y).await?;
            Ok("Performed drag".to_string())
        }
        ActionType::Wait => {
            let ms = planned
                .amount
                .map(|n| n.max(0) as u64)
                .unwrap_or(DEFAULT_WAIT_MS);
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            Ok(format!("Waited {ms}ms"))
        }
        ActionType::Finished | ActionType::CallUser => Ok(String::new()),
    }
}

fn parse_scroll_direction(value: Option<&str>) -> ScrollDirection {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("up") => ScrollDirection::Up,
        Some("left") => ScrollDirection::Left,
        Some("right") => ScrollDirection::Right,
        _ => ScrollDirection::Down,
    }
}

async fn sleep_or_cancel(cancel: &CancellationToken, ms: u64) {
    if ms == 0 {
        return;
    }
    tokio::select! {
        () = cancel.cancelled() => {}
        () = tokio::time::sleep(std::time::Duration::from_millis(ms)) => {}
    }
}
