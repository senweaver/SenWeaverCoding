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
const MAX_WAIT_MS: u64 = 15_000;
const DEFAULT_SCROLL_AMOUNT: i32 = 3;
const MAX_TOTAL_STEPS: u32 = 1_000;
const MAX_DISPLAY_SWITCHES: u32 = 8;

#[derive(Debug, Clone, Default)]
pub struct UserMessage {
    pub text: String,
    pub image_data_uris: Vec<String>,
}

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
    pub screenshot_mime: &'static str,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
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
    UserUpdate {
        index: u32,
        text: String,
    },
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
}

impl ComputerEvent {
    pub fn status(status: RunStatus, message: Option<String>) -> Self {
        ComputerEvent::Status {
            status,
            message,
            code: None,
        }
    }

    pub fn status_code(status: RunStatus, code: &str, message: impl Into<String>) -> Self {
        ComputerEvent::Status {
            status,
            message: Some(message.into()),
            code: Some(code.to_string()),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        ComputerEvent::Error {
            message: message.into(),
            code: None,
        }
    }

    pub fn error_code(code: &str, message: impl Into<String>) -> Self {
        ComputerEvent::Error {
            message: message.into(),
            code: Some(code.to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunParams {
    pub run_id: String,
    pub task: String,
    pub provider: String,
    pub model: String,
    pub max_steps: u32,
    pub step_delay_ms: u64,
    pub reference_images: Vec<String>,
    pub initial_history: Vec<String>,
}

enum SleepOutcome {
    Elapsed,
    Cancelled,
    User(UserMessage),
}

struct SteerState {
    history: Vec<String>,
    pending_images: Vec<String>,
    goal_steps: u32,
    consecutive_errors: u32,
    consecutive_action_failures: u32,
}

impl SteerState {
    fn apply_update(&mut self, msg: UserMessage, index: u32, emit: &impl Fn(ComputerEvent)) {
        let text = msg.text.trim().to_string();
        let has_images = !msg.image_data_uris.is_empty();
        if !text.is_empty() {
            self.history.push(format!("USER UPDATE (live instruction): {text}"));
        } else if has_images {
            self.history
                .push("USER UPDATE (live instruction): the user attached reference image(s); \
                       take them into account.".to_string());
        }
        emit(ComputerEvent::UserUpdate { index, text });
        self.pending_images.extend(msg.image_data_uris);
        self.goal_steps = 0;
        self.consecutive_errors = 0;
        self.consecutive_action_failures = 0;
    }

    fn drain_inbox(
        &mut self,
        user_rx: &mut UnboundedReceiver<UserMessage>,
        index: u32,
        emit: &impl Fn(ComputerEvent),
    ) -> bool {
        let mut any = false;
        while let Ok(msg) = user_rx.try_recv() {
            self.apply_update(msg, index, emit);
            any = true;
        }
        any
    }

    fn take_reference_images(&mut self, limit: usize) -> Vec<String> {
        if limit == 0 {
            self.pending_images.clear();
            return Vec::new();
        }
        let start = self.pending_images.len().saturating_sub(limit);
        let recent = self.pending_images.split_off(start);
        self.pending_images.clear();
        recent
    }
}

pub async fn run_loop(
    params: RunParams,
    config: Config,
    cancel: CancellationToken,
    event_tx: UnboundedSender<ComputerEvent>,
    mut user_rx: UnboundedReceiver<UserMessage>,
) {
    let emit = |event: ComputerEvent| {
        let _ = event_tx.send(event);
    };

    let _input_lease = match super::input_lock::try_acquire(super::input_lock::InputActivity::Agent)
    {
        Ok(lease) => lease,
        Err(message) => {
            emit(ComputerEvent::error_code("busy", message));
            emit(ComputerEvent::status(RunStatus::Error, None));
            return;
        }
    };

    let client = match VisionClient::from_config(&config, &params.provider, &params.model) {
        Ok(client) => client,
        Err(err) => {
            emit(ComputerEvent::error_code(
                "model_init_failed",
                format!("failed to initialize model '{}': {err}", params.model),
            ));
            emit(ComputerEvent::status(RunStatus::Error, None));
            return;
        }
    };

    emit(ComputerEvent::status(RunStatus::Running, None));

    let monitors = capture::list_monitors().await;
    let mut current_display = primary_display_index(&monitors);
    let mut display_switches: u32 = 0;

    let mut steer = SteerState {
        history: params.initial_history.clone(),
        pending_images: params.reference_images.clone(),
        goal_steps: 0,
        consecutive_errors: 0,
        consecutive_action_failures: 0,
    };
    let mut step: u32 = 0;
    let reference_limit = client.max_reference_images();

    while steer.goal_steps < params.max_steps && step < MAX_TOTAL_STEPS {
        if cancel.is_cancelled() {
            emit(ComputerEvent::status(RunStatus::Stopped, None));
            return;
        }

        steer.drain_inbox(&mut user_rx, step, &emit);

        let screen = match capture_display(&monitors, current_display).await {
            Ok(screen) => screen,
            Err(err) => {
                emit(ComputerEvent::error_code(
                    "capture_failed",
                    format!("screen capture failed: {err}"),
                ));
                steer.consecutive_errors += 1;
                if steer.consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    emit(ComputerEvent::status_code(
                        RunStatus::Error,
                        "capture_failed_repeated",
                        "aborted after repeated capture failures",
                    ));
                    return;
                }
                let backoff = (1000u64 * u64::from(steer.consecutive_errors)).min(4000);
                match sleep_user_or_cancel(&cancel, &mut user_rx, backoff).await {
                    SleepOutcome::Cancelled => {
                        emit(ComputerEvent::status(RunStatus::Stopped, None));
                        return;
                    }
                    SleepOutcome::User(msg) => steer.apply_update(msg, step, &emit),
                    SleepOutcome::Elapsed => {}
                }
                continue;
            }
        };

        emit(ComputerEvent::status(RunStatus::Thinking, None));

        let data_uri = screen.data_uri();
        let reference_images = steer.take_reference_images(reference_limit);
        let reference_refs: Vec<&str> = reference_images.iter().map(String::as_str).collect();
        let display_hint = describe_displays(&monitors, current_display);
        let plan_result = tokio::select! {
            () = cancel.cancelled() => {
                emit(ComputerEvent::status(RunStatus::Stopped, None));
                return;
            }
            msg = recv_user(&mut user_rx) => {
                steer.pending_images = reference_images;
                steer.apply_update(msg, step, &emit);
                continue;
            }
            result = planner::plan_next(
                &client,
                &data_uri,
                &params.task,
                &steer.history,
                &reference_refs,
                display_hint.as_deref(),
            ) => result,
        };
        let planned = match plan_result {
            Ok(planned) => planned,
            Err(err) => {
                emit(ComputerEvent::error_code(
                    "planning_failed",
                    format!("planning failed: {err}"),
                ));
                steer.consecutive_errors += 1;
                if steer.consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    emit(ComputerEvent::status_code(
                        RunStatus::Error,
                        "planning_failed_repeated",
                        "aborted after repeated planning failures",
                    ));
                    return;
                }
                let backoff = (800u64 * u64::from(steer.consecutive_errors)).min(4000);
                match sleep_user_or_cancel(&cancel, &mut user_rx, backoff).await {
                    SleepOutcome::Cancelled => {
                        emit(ComputerEvent::status(RunStatus::Stopped, None));
                        return;
                    }
                    SleepOutcome::User(msg) => steer.apply_update(msg, step, &emit),
                    SleepOutcome::Elapsed => {}
                }
                continue;
            }
        };
        steer.consecutive_errors = 0;

        if let Some(target_display) = planned.display {
            if target_display < monitors.len()
                && target_display != current_display
                && display_switches < MAX_DISPLAY_SWITCHES
            {
                current_display = target_display;
                display_switches += 1;
                steer
                    .history
                    .push(format!("Switched to display {target_display}."));
                continue;
            }
        }

        let monitor = screen.monitor;

        let mut primary_target: Option<ResolvedTarget> = None;
        let mut secondary_target: Option<ResolvedTarget> = None;

        if planned.action.needs_target() {
            let resolve_all = async {
                let mut primary: Option<ResolvedTarget> = None;
                let mut secondary: Option<ResolvedTarget> = None;
                match resolve_target(
                    &client,
                    &data_uri,
                    planned.start_box.as_deref(),
                    planned.element_description.as_deref(),
                    monitor,
                )
                .await
                {
                    Ok(target) => primary = Some(target),
                    Err(err) => emit(ComputerEvent::error_code(
                        "target_not_located",
                        format!("could not locate target: {err}"),
                    )),
                }
                if matches!(planned.action, ActionType::Drag) {
                    match resolve_target(
                        &client,
                        &data_uri,
                        planned.end_box.as_deref(),
                        planned.to_element_description.as_deref(),
                        monitor,
                    )
                    .await
                    {
                        Ok(target) => secondary = Some(target),
                        Err(err) => emit(ComputerEvent::error_code(
                            "drag_target_not_located",
                            format!("could not locate drag destination: {err}"),
                        )),
                    }
                }
                (primary, secondary)
            };
            let resolved = tokio::select! {
                () = cancel.cancelled() => {
                    emit(ComputerEvent::status(RunStatus::Stopped, None));
                    return;
                }
                msg = recv_user(&mut user_rx) => {
                    steer.apply_update(msg, step, &emit);
                    continue;
                }
                resolved = resolve_all => resolved,
            };
            primary_target = resolved.0;
            secondary_target = resolved.1;
        }

        if steer.drain_inbox(&mut user_rx, step, &emit) {
            continue;
        }

        emit(ComputerEvent::Step {
            step: ComputerStepEvent {
                index: step,
                thought: planned.thought.clone(),
                action_type: planned.action.as_str().to_string(),
                element_description: planned.element_description.clone(),
                value: planned.value.clone(),
                screenshot_base64: screen.display_jpeg_base64.clone(),
                screenshot_mime: "image/jpeg",
                target_x_norm: primary_target.as_ref().map(|t| t.x_norm),
                target_y_norm: primary_target.as_ref().map(|t| t.y_norm),
                to_x_norm: secondary_target.as_ref().map(|t| t.x_norm),
                to_y_norm: secondary_target.as_ref().map(|t| t.y_norm),
                confidence: primary_target.as_ref().map(|t| t.confidence),
            },
        });

        match planned.action {
            ActionType::Finished => {
                emit(ComputerEvent::status(
                    RunStatus::Finished,
                    Some(planned.thought.clone()),
                ));
                return;
            }
            ActionType::CallUser => {
                emit(ComputerEvent::status(
                    RunStatus::CallUser,
                    Some(planned.thought.clone()),
                ));
                tokio::select! {
                    () = cancel.cancelled() => {
                        emit(ComputerEvent::status(RunStatus::Stopped, None));
                        return;
                    }
                    reply = user_rx.recv() => {
                        match reply {
                            Some(msg) => {
                                let text = msg.text.trim().to_string();
                                if !text.is_empty() {
                                    steer.history.push(format!(
                                        "Asked the user for help; they replied: {text}"
                                    ));
                                    emit(ComputerEvent::UserUpdate {
                                        index: step,
                                        text,
                                    });
                                }
                                steer.pending_images.extend(msg.image_data_uris);
                                steer.goal_steps = 0;
                                steer.consecutive_errors = 0;
                                steer.consecutive_action_failures = 0;
                                emit(ComputerEvent::status(RunStatus::Running, None));
                                continue;
                            }
                            None => {
                                emit(ComputerEvent::status(RunStatus::Stopped, None));
                                return;
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        if cancel.is_cancelled() {
            emit(ComputerEvent::status(RunStatus::Stopped, None));
            return;
        }

        emit(ComputerEvent::status(RunStatus::Running, None));

        let outcome = if matches!(planned.action, ActionType::Wait) {
            let ms = planned
                .amount
                .map(|n| (n.max(0) as u64).min(MAX_WAIT_MS))
                .unwrap_or(DEFAULT_WAIT_MS);
            match sleep_user_or_cancel(&cancel, &mut user_rx, ms).await {
                SleepOutcome::Cancelled => {
                    emit(ComputerEvent::status(RunStatus::Stopped, None));
                    return;
                }
                SleepOutcome::User(msg) => {
                    steer.apply_update(msg, step, &emit);
                    continue;
                }
                SleepOutcome::Elapsed => Ok(format!("Waited {ms}ms")),
            }
        } else {
            execute_action(
                &planned,
                monitor,
                primary_target,
                secondary_target,
                &cancel,
            )
            .await
        };
        match outcome {
            Ok(summary) => {
                steer.consecutive_action_failures = 0;
                emit(ComputerEvent::ActionResult {
                    index: step,
                    success: true,
                    message: None,
                });
                steer.history.push(summary);
            }
            Err(err) => {
                steer.consecutive_action_failures += 1;
                emit(ComputerEvent::ActionResult {
                    index: step,
                    success: false,
                    message: Some(err.to_string()),
                });
                steer.history.push(format!(
                    "Attempted {} but it failed: {err}",
                    planned.action.as_str()
                ));
                if steer.consecutive_action_failures >= MAX_CONSECUTIVE_ERRORS {
                    emit(ComputerEvent::status_code(
                        RunStatus::Error,
                        "action_failed_repeated",
                        "aborted after repeated action failures; the agent could not make progress",
                    ));
                    return;
                }
            }
        }

        step += 1;
        steer.goal_steps += 1;
        match sleep_user_or_cancel(&cancel, &mut user_rx, params.step_delay_ms).await {
            SleepOutcome::Cancelled => {
                emit(ComputerEvent::status(RunStatus::Stopped, None));
                return;
            }
            SleepOutcome::User(msg) => steer.apply_update(msg, step, &emit),
            SleepOutcome::Elapsed => {}
        }
    }

    emit(ComputerEvent::status_code(
        RunStatus::Finished,
        "step_limit_reached",
        format!("reached the step limit ({})", params.max_steps),
    ));
}

fn primary_display_index(monitors: &[coordinates::MonitorRect]) -> usize {
    monitors
        .iter()
        .position(|m| m.x == 0 && m.y == 0)
        .unwrap_or(0)
}

fn describe_displays(monitors: &[coordinates::MonitorRect], current: usize) -> Option<String> {
    if monitors.len() <= 1 {
        return None;
    }
    let min_x = monitors.iter().map(|m| m.x).min().unwrap_or(0);
    let max_x = monitors.iter().map(|m| m.x).max().unwrap_or(0);
    let min_y = monitors.iter().map(|m| m.y).min().unwrap_or(0);
    let max_y = monitors.iter().map(|m| m.y).max().unwrap_or(0);
    let mut out = format!(
        "This computer has {} displays and you are currently viewing display {current} (0-based). \
         The screenshot shows ONLY that display. Displays are laid out on a shared desktop:\n",
        monitors.len()
    );
    for (idx, m) in monitors.iter().enumerate() {
        let mut position = String::new();
        if m.x == min_x && max_x != min_x {
            position.push_str("left");
        } else if m.x == max_x && max_x != min_x {
            position.push_str("right");
        }
        if m.y == min_y && max_y != min_y {
            if !position.is_empty() {
                position.push('-');
            }
            position.push_str("top");
        } else if m.y == max_y && max_y != min_y {
            if !position.is_empty() {
                position.push('-');
            }
            position.push_str("bottom");
        }
        if position.is_empty() {
            position.push_str("center");
        }
        let marker = if idx == current { " (current)" } else { "" };
        out.push_str(&format!(
            "- display {idx}: {}x{} at ({},{}), {position}{marker}\n",
            m.width, m.height, m.x, m.y
        ));
    }
    out.push_str(
        "If the target is on a different display, respond with \"display\": <0-based index> to \
         switch; a fresh screenshot of that display will be taken next.",
    );
    Some(out)
}

async fn capture_display(
    monitors: &[coordinates::MonitorRect],
    index: usize,
) -> anyhow::Result<capture::CapturedScreen> {
    match monitors.get(index) {
        Some(monitor) => capture::capture_monitor(monitor.id).await,
        None => capture::capture_primary().await,
    }
}

async fn recv_user(user_rx: &mut UnboundedReceiver<UserMessage>) -> UserMessage {
    match user_rx.recv().await {
        Some(msg) => msg,
        None => std::future::pending().await,
    }
}

async fn sleep_user_or_cancel(
    cancel: &CancellationToken,
    user_rx: &mut UnboundedReceiver<UserMessage>,
    ms: u64,
) -> SleepOutcome {
    if ms == 0 {
        return SleepOutcome::Elapsed;
    }
    tokio::select! {
        () = cancel.cancelled() => SleepOutcome::Cancelled,
        msg = recv_user(user_rx) => SleepOutcome::User(msg),
        () = tokio::time::sleep(std::time::Duration::from_millis(ms)) => SleepOutcome::Elapsed,
    }
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
    monitor: coordinates::MonitorRect,
) -> anyhow::Result<ResolvedTarget> {
    if let Some(values) = coords {
        if let Some((x_norm, y_norm)) = coordinates::coords_to_center_norm(values) {
            return Ok(ResolvedTarget::from_norm(x_norm, y_norm, 100.0, monitor));
        }
    }
    let desc = description
        .ok_or_else(|| anyhow::anyhow!("no target coordinates or description provided"))?;
    let result = grounding::locate(client, data_uri, desc).await?;
    Ok(ResolvedTarget::from_norm(
        result.x_norm,
        result.y_norm,
        result.confidence,
        monitor,
    ))
}

impl ResolvedTarget {
    fn from_norm(
        x_norm: f64,
        y_norm: f64,
        confidence: f64,
        monitor: coordinates::MonitorRect,
    ) -> Self {
        let (input_x, input_y) = monitor.denormalize(x_norm, y_norm);
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
    monitor: coordinates::MonitorRect,
    primary: Option<ResolvedTarget>,
    secondary: Option<ResolvedTarget>,
    cancel: &CancellationToken,
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
                None => (
                    monitor.x + monitor.width.max(1) / 2,
                    monitor.y + monitor.height.max(1) / 2,
                ),
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
                .map(|n| (n.max(0) as u64).min(MAX_WAIT_MS))
                .unwrap_or(DEFAULT_WAIT_MS);
            sleep_or_cancel(cancel, ms).await;
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
