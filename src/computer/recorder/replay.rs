// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use super::types::{RecordedStep, RecordingManifest};
use crate::computer::action::extract_json_object;
use crate::computer::capture;
use crate::computer::coordinates::{self, Box2d};
use crate::computer::input::{self, ClickButton, ScrollDirection};
use crate::computer::run::{ComputerEvent, ComputerStepEvent, RunStatus};
use crate::computer::vision::VisionClient;
use crate::config::Config;

const MAX_STEP_DELAY_MS: u64 = 10_000;
const DEFAULT_SCROLL_AMOUNT: i32 = 3;
const REPLAY_INITIAL_SETTLE_MS: u64 = 800;
const MAX_REPEAT_COUNT: u32 = 100;
const MAX_REPEAT_INTERVAL_MS: u64 = 3_600_000;
const SMART_DELAY_CAP_MS: u64 = 2_000;
const SMART_MAX_RECOVERIES_PER_STEP: u32 = 3;
const SMART_RECOVERY_SETTLE_MS: u64 = 700;
const SMART_MAX_RECOVERY_WAIT_MS: u64 = 5_000;
const SMART_GROUNDING_TIMEOUT_MS: u64 = 60_000;
const REFERENCE_CROP_PX: u32 = 480;
const SKILL_CONTEXT_MAX_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy)]
pub struct ReplayRepeat {
    pub count: u32,
    pub interval_ms: u64,
}

impl Default for ReplayRepeat {
    fn default() -> Self {
        Self {
            count: 1,
            interval_ms: 0,
        }
    }
}

impl ReplayRepeat {
    pub fn clamped(self) -> Self {
        Self {
            count: self.count.clamp(1, MAX_REPEAT_COUNT),
            interval_ms: self.interval_ms.min(MAX_REPEAT_INTERVAL_MS),
        }
    }
}

pub async fn replay_recording(
    manifest: RecordingManifest,
    repeat: ReplayRepeat,
    cancel: CancellationToken,
    event_tx: UnboundedSender<ComputerEvent>,
) {
    let emit = |event: ComputerEvent| {
        let _ = event_tx.send(event);
    };

    if manifest.steps.is_empty() {
        emit(ComputerEvent::status_code(
            RunStatus::Error,
            "recording_empty",
            "recording contains no steps",
        ));
        return;
    }

    let _input_lease =
        match crate::computer::input_lock::try_acquire(crate::computer::input_lock::InputActivity::Replay) {
            Ok(lease) => lease,
            Err(message) => {
                emit(ComputerEvent::error_code("busy", message));
                emit(ComputerEvent::status(RunStatus::Error, None));
                return;
            }
        };

    let repeat = repeat.clamped();
    let mut ui_index: u32 = 0;

    for iteration in 0..repeat.count {
        if repeat.count > 1 {
            emit(ComputerEvent::status_code(
                RunStatus::Running,
                "replay_iteration",
                format!("replay run {}/{}", iteration + 1, repeat.count),
            ));
        }
        if replay_steps_once(&manifest, &cancel, &emit, &mut ui_index)
            .await
            .is_err()
        {
            return;
        }
        if iteration + 1 < repeat.count && repeat.interval_ms > 0 {
            sleep_or_cancel(&cancel, repeat.interval_ms).await;
            if cancel.is_cancelled() {
                emit(ComputerEvent::status(RunStatus::Stopped, None));
                return;
            }
        }
    }

    emit(ComputerEvent::status_code(
        RunStatus::Finished,
        "replay_completed",
        "replay completed",
    ));
}

async fn replay_steps_once(
    manifest: &RecordingManifest,
    cancel: &CancellationToken,
    emit: &impl Fn(ComputerEvent),
    ui_index: &mut u32,
) -> Result<(), ()> {
    emit(ComputerEvent::status_code(
        RunStatus::Running,
        "replaying_steps",
        format!("replaying {} recorded steps", manifest.steps.len()),
    ));

    let (display_w, display_h) = match input::main_display_size().await {
        Ok((w, h)) if w > 0 && h > 0 => (w, h),
        _ => (manifest.display_w.max(1), manifest.display_h.max(1)),
    };

    sleep_or_cancel(cancel, REPLAY_INITIAL_SETTLE_MS).await;

    for step in &manifest.steps {
        if cancel.is_cancelled() {
            emit(ComputerEvent::status(RunStatus::Stopped, None));
            return Err(());
        }

        sleep_or_cancel(cancel, step.delay_ms.min(MAX_STEP_DELAY_MS)).await;
        if cancel.is_cancelled() {
            emit(ComputerEvent::status(RunStatus::Stopped, None));
            return Err(());
        }

        let screenshot_base64 = capture::capture_preview_jpeg().await.unwrap_or_default();

        let index = *ui_index;
        emit(ComputerEvent::Step {
            step: ComputerStepEvent {
                index,
                thought: step
                    .element_description
                    .clone()
                    .unwrap_or_else(|| format!("Replay recorded {} action", step.action_type)),
                action_type: step.action_type.clone(),
                element_description: step.element_description.clone(),
                value: step.value.clone(),
                screenshot_base64,
                screenshot_mime: "image/jpeg",
                target_x_norm: step.x_norm,
                target_y_norm: step.y_norm,
                to_x_norm: step.to_x_norm,
                to_y_norm: step.to_y_norm,
                confidence: None,
            },
        });

        match execute_step(step, display_w, display_h, cancel).await {
            Ok(()) => {
                emit(ComputerEvent::ActionResult {
                    index,
                    success: true,
                    message: None,
                });
            }
            Err(err) => {
                emit(ComputerEvent::ActionResult {
                    index,
                    success: false,
                    message: Some(err.to_string()),
                });
                emit(ComputerEvent::status_code(
                    RunStatus::Error,
                    "replay_step_failed",
                    format!("replay stopped at step {}: {err}", step.index),
                ));
                return Err(());
            }
        }
        *ui_index += 1;
    }
    Ok(())
}

async fn execute_step(
    step: &RecordedStep,
    display_w: i32,
    display_h: i32,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    let target = step
        .x_abs
        .zip(step.y_abs)
        .or_else(|| {
            step.x_norm.zip(step.y_norm).map(|(xn, yn)| {
                coordinates::normalized_to_input(xn, yn, display_w, display_h)
            })
        });
    let to_target = step
        .to_x_abs
        .zip(step.to_y_abs)
        .or_else(|| {
            step.to_x_norm.zip(step.to_y_norm).map(|(xn, yn)| {
                coordinates::normalized_to_input(xn, yn, display_w, display_h)
            })
        });
    execute_action_at(
        &step.action_type,
        target,
        to_target,
        step.value.as_deref(),
        step.amount,
        display_w,
        display_h,
        cancel,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_action_at(
    action_type: &str,
    target: Option<(i32, i32)>,
    to_target: Option<(i32, i32)>,
    value: Option<&str>,
    amount: Option<i32>,
    display_w: i32,
    display_h: i32,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    match action_type {
        "click" => {
            let (x, y) = target.ok_or_else(|| anyhow::anyhow!("click step missing target"))?;
            input::click(x, y, ClickButton::Left, 1).await
        }
        "double_click" => {
            let (x, y) =
                target.ok_or_else(|| anyhow::anyhow!("double_click step missing target"))?;
            input::click(x, y, ClickButton::Left, 2).await
        }
        "right_click" => {
            let (x, y) =
                target.ok_or_else(|| anyhow::anyhow!("right_click step missing target"))?;
            input::click(x, y, ClickButton::Right, 1).await
        }
        "type" => {
            let text = value
                .map(str::to_string)
                .ok_or_else(|| anyhow::anyhow!("type step missing text"))?;
            input::type_text(text).await
        }
        "key_press" => {
            let combo = value
                .map(str::to_string)
                .ok_or_else(|| anyhow::anyhow!("key_press step missing combo"))?;
            input::key_combo(combo).await
        }
        "scroll" => {
            let (x, y) = target.unwrap_or((display_w / 2, display_h / 2));
            let direction = match value {
                Some("up") => ScrollDirection::Up,
                Some("left") => ScrollDirection::Left,
                Some("right") => ScrollDirection::Right,
                _ => ScrollDirection::Down,
            };
            let amount = amount.unwrap_or(DEFAULT_SCROLL_AMOUNT);
            input::scroll(x, y, direction, amount).await
        }
        "drag" => {
            let (fx, fy) = target.ok_or_else(|| anyhow::anyhow!("drag step missing source"))?;
            let (tx, ty) =
                to_target.ok_or_else(|| anyhow::anyhow!("drag step missing destination"))?;
            input::drag(fx, fy, tx, ty).await
        }
        "move_mouse" => {
            let (x, y) = target.ok_or_else(|| anyhow::anyhow!("move step missing target"))?;
            input::move_to(x, y).await
        }
        "wait" => {
            let ms = amount
                .map(|n| (n.max(0) as u64).min(MAX_STEP_DELAY_MS))
                .unwrap_or(800);
            sleep_or_cancel(cancel, ms).await;
            Ok(())
        }
        other => anyhow::bail!("unsupported recorded action: {other}"),
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

const SMART_GROUNDING_SYSTEM: &str = "You are supervising the replay of one recorded desktop \
automation step on a live screen. You receive the CURRENT screenshot of the screen and, when \
available, a cropped REFERENCE image captured during the original recording, centered on the \
exact UI element the user interacted with. Since the recording, windows may have moved, the \
layout may differ, and other windows, dialogs or menus may now cover the target.\n\n\
Decide exactly one of:\n\
- \"found\": the target element is visible in the CURRENT screenshot. Return its bounding box.\n\
- \"obscured\": the target exists but is hidden behind another window, dialog, menu or overlay. \
Provide ONE recovery action that will reveal it, such as clicking the obstructing window's \
minimize or close button, pressing a key like \"escape\" or \"alt+tab\", scrolling, or dragging \
the obstructing window aside. Never use the obstructing window's close button if closing it \
could lose user data; prefer minimize or moving it.\n\
- \"not_found\": the target genuinely does not exist on this screen and no simple recovery \
would reveal it.\n\n\
Windows belonging to this assistant itself may appear in the REFERENCE image or the CURRENT \
screen: a small floating status card (a round orb with a brand name and a status line, usually \
in a corner), and the assistant's own control/recorder window (which may show a pink 'recording' \
banner, a step list, buttons like 'return to edit' / 'generate' / 'stop', or a task input). \
None of these are part of the user's task. Never click, drag, or target them, and never propose \
a recovery action that interacts with them (do not click 'return to edit', 'stop', or any of the \
assistant's own buttons). If the REFERENCE crop mostly shows the assistant's own interface, \
disregard it and locate the target on the CURRENT screen using the recorded action type and its \
normalized coordinates as the prior. If the only visible difference between reference and current \
is that the assistant's own window is shown or hidden, treat the target as unaffected and proceed \
by coordinates.\n\n\
All coordinates are normalized to 0-1000 relative to the CURRENT screenshot, boxes are \
[ymin, xmin, ymax, xmax] with origin at the top-left. Respond with raw JSON only, no markdown:\n\
{\"status\":\"found|obscured|not_found\",\"thought\":\"...\",\"box_2d\":[ymin,xmin,ymax,xmax],\
\"to_box_2d\":[ymin,xmin,ymax,xmax],\"confidence\":0-100,\"recovery\":{\"action\":\"click|\
double_click|right_click|key_press|scroll|drag|wait\",\"box_2d\":[ymin,xmin,ymax,xmax],\
\"to_box_2d\":[ymin,xmin,ymax,xmax],\"value\":\"...\",\"amount\":0}}\n\
Include box_2d only when status is found (and to_box_2d only when the step is a drag). Include \
recovery only when status is obscured.";

#[derive(Debug)]
struct SmartRecovery {
    action: String,
    target: Option<(f64, f64)>,
    to_target: Option<(f64, f64)>,
    value: Option<String>,
    amount: Option<i32>,
}

#[derive(Debug)]
struct SmartLocation {
    status: String,
    thought: String,
    target: Option<(f64, f64)>,
    to_target: Option<(f64, f64)>,
    confidence: Option<f64>,
    recovery: Option<SmartRecovery>,
}

fn parse_box_center(value: Option<&serde_json::Value>) -> Option<(f64, f64)> {
    let coords = value?.as_array()?;
    let numbers: Vec<f64> = coords
        .iter()
        .filter_map(serde_json::Value::as_f64)
        .collect();
    Box2d::from_slice(&numbers).map(|b| b.center_normalized())
}

fn parse_smart_location(raw: &str) -> anyhow::Result<SmartLocation> {
    let json_str = extract_json_object(raw)
        .ok_or_else(|| anyhow::anyhow!("grounding response missing JSON object: {raw}"))?;
    let value: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| anyhow::anyhow!("failed to parse grounding JSON: {e}; raw: {json_str}"))?;

    let status = value
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("not_found")
        .to_ascii_lowercase();
    let thought = value
        .get("thought")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let recovery = value.get("recovery").and_then(|r| {
        let action = r.get("action")?.as_str()?.to_ascii_lowercase();
        Some(SmartRecovery {
            action,
            target: parse_box_center(r.get("box_2d")),
            to_target: parse_box_center(r.get("to_box_2d")),
            value: r
                .get("value")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            amount: r
                .get("amount")
                .and_then(serde_json::Value::as_i64)
                .map(|n| n as i32),
        })
    });

    Ok(SmartLocation {
        status,
        thought,
        target: parse_box_center(value.get("box_2d")),
        to_target: parse_box_center(value.get("to_box_2d")),
        confidence: value.get("confidence").and_then(serde_json::Value::as_f64),
        recovery,
    })
}

async fn load_reference_crop(recording_dir: &Path, step: &RecordedStep) -> Option<String> {
    let file = step.screenshot_file.clone()?;
    let x_norm = step.x_norm?;
    let y_norm = step.y_norm?;
    let path = recording_dir.join(file);
    tokio::task::spawn_blocking(move || {
        let img = image::open(&path).ok()?.to_rgba8();
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return None;
        }
        let cx = (x_norm / 1000.0 * f64::from(w)).round() as i64;
        let cy = (y_norm / 1000.0 * f64::from(h)).round() as i64;
        let half = i64::from(REFERENCE_CROP_PX / 2);
        let x0 = (cx - half).clamp(0, i64::from(w).saturating_sub(1));
        let y0 = (cy - half).clamp(0, i64::from(h).saturating_sub(1));
        let x1 = (cx + half).clamp(x0 + 1, i64::from(w));
        let y1 = (cy + half).clamp(y0 + 1, i64::from(h));
        let crop = image::imageops::crop_imm(
            &img,
            x0 as u32,
            y0 as u32,
            (x1 - x0) as u32,
            (y1 - y0) as u32,
        )
        .to_image();
        let b64 = capture::encode_preview_jpeg_base64(&crop).ok()?;
        Some(format!("data:image/jpeg;base64,{b64}"))
    })
    .await
    .ok()
    .flatten()
}

async fn load_skill_context(recording_dir: &Path) -> Option<String> {
    let content = tokio::fs::read_to_string(recording_dir.join("SKILL.md"))
        .await
        .ok()?;
    let stripped = super::session::strip_frontmatter(&content);
    let trimmed = stripped.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(SKILL_CONTEXT_MAX_CHARS).collect())
}

fn step_needs_grounding(action_type: &str) -> bool {
    matches!(
        action_type,
        "click" | "double_click" | "right_click" | "drag" | "move_mouse"
    )
}

fn describe_step(step: &RecordedStep, total: usize, skill_context: Option<&str>) -> String {
    let mut text = format!(
        "Recorded step {} of {total}.\nAction: {}\n",
        step.index + 1,
        step.action_type
    );
    if let Some(desc) = &step.element_description {
        text.push_str(&format!("Target element (from the recording): {desc}\n"));
    }
    if let (Some(x), Some(y)) = (step.x_norm, step.y_norm) {
        text.push_str(&format!(
            "Recorded normalized position (0-1000): ({:.0}, {:.0}). The element may have moved.\n",
            x, y
        ));
    }
    if let (Some(tx), Some(ty)) = (step.to_x_norm, step.to_y_norm) {
        text.push_str(&format!(
            "Recorded drag destination (0-1000): ({:.0}, {:.0}).\n",
            tx, ty
        ));
    }
    if let Some(value) = &step.value {
        text.push_str(&format!("Associated value: \"{value}\"\n"));
    }
    if let Some(context) = skill_context {
        text.push_str(&format!(
            "\nWorkflow notes from the recorded skill document:\n{context}\n"
        ));
    }
    text
}

#[allow(clippy::too_many_arguments)]
pub async fn replay_recording_smart(
    manifest: RecordingManifest,
    recording_dir: PathBuf,
    config: Config,
    provider: String,
    model: String,
    repeat: ReplayRepeat,
    cancel: CancellationToken,
    event_tx: UnboundedSender<ComputerEvent>,
) {
    let emit = |event: ComputerEvent| {
        let _ = event_tx.send(event);
    };

    if manifest.steps.is_empty() {
        emit(ComputerEvent::status_code(
            RunStatus::Error,
            "recording_empty",
            "recording contains no steps",
        ));
        return;
    }

    let _input_lease =
        match crate::computer::input_lock::try_acquire(crate::computer::input_lock::InputActivity::Replay) {
            Ok(lease) => lease,
            Err(message) => {
                emit(ComputerEvent::error_code("busy", message));
                emit(ComputerEvent::status(RunStatus::Error, None));
                return;
            }
        };

    let client = match VisionClient::from_config(&config, &provider, &model) {
        Ok(client) => client,
        Err(err) => {
            emit(ComputerEvent::error_code(
                "model_init_failed",
                format!("failed to initialize vision model '{model}': {err}"),
            ));
            emit(ComputerEvent::status(RunStatus::Error, None));
            return;
        }
    };

    let skill_context = load_skill_context(&recording_dir).await;
    let repeat = repeat.clamped();
    let mut ui_index: u32 = 0;

    for iteration in 0..repeat.count {
        if repeat.count > 1 {
            emit(ComputerEvent::status_code(
                RunStatus::Running,
                "replay_iteration",
                format!("replay run {}/{}", iteration + 1, repeat.count),
            ));
        }
        if replay_smart_steps_once(
            &manifest,
            &recording_dir,
            &client,
            skill_context.as_deref(),
            &cancel,
            &emit,
            &mut ui_index,
        )
        .await
        .is_err()
        {
            return;
        }
        if iteration + 1 < repeat.count && repeat.interval_ms > 0 {
            sleep_or_cancel(&cancel, repeat.interval_ms).await;
            if cancel.is_cancelled() {
                emit(ComputerEvent::status(RunStatus::Stopped, None));
                return;
            }
        }
    }

    emit(ComputerEvent::status_code(
        RunStatus::Finished,
        "smart_replay_completed",
        "smart replay completed",
    ));
}

#[allow(clippy::too_many_arguments)]
async fn replay_smart_steps_once(
    manifest: &RecordingManifest,
    recording_dir: &Path,
    client: &VisionClient,
    skill_context: Option<&str>,
    cancel: &CancellationToken,
    emit: &impl Fn(ComputerEvent),
    ui_index: &mut u32,
) -> Result<(), ()> {
    let total = manifest.steps.len();
    let call_budget = (total as u32) * 4 + 8;
    let mut calls_used: u32 = 0;

    emit(ComputerEvent::status_code(
        RunStatus::Running,
        "smart_replaying_steps",
        format!("smart replaying {total} recorded steps"),
    ));

    let (display_w, display_h) = match input::main_display_size().await {
        Ok((w, h)) if w > 0 && h > 0 => (w, h),
        _ => (manifest.display_w.max(1), manifest.display_h.max(1)),
    };

    sleep_or_cancel(cancel, REPLAY_INITIAL_SETTLE_MS).await;

    for step in &manifest.steps {
        if cancel.is_cancelled() {
            emit(ComputerEvent::status(RunStatus::Stopped, None));
            return Err(());
        }

        sleep_or_cancel(cancel, step.delay_ms.min(SMART_DELAY_CAP_MS)).await;
        if cancel.is_cancelled() {
            emit(ComputerEvent::status(RunStatus::Stopped, None));
            return Err(());
        }

        if !step_needs_grounding(&step.action_type) {
            let screenshot_base64 = capture::capture_primary()
                .await
                .map(|s| s.display_jpeg_base64)
                .unwrap_or_default();
            emit(ComputerEvent::Step {
                step: ComputerStepEvent {
                    index: *ui_index,
                    thought: format!(
                        "Replaying recorded {} action (step {} of {total})",
                        step.action_type,
                        step.index + 1
                    ),
                    action_type: step.action_type.clone(),
                    element_description: step.element_description.clone(),
                    value: step.value.clone(),
                    screenshot_base64,
                    screenshot_mime: "image/jpeg",
                    target_x_norm: step.x_norm,
                    target_y_norm: step.y_norm,
                    to_x_norm: step.to_x_norm,
                    to_y_norm: step.to_y_norm,
                    confidence: None,
                },
            });
            match execute_step(step, display_w, display_h, cancel).await {
                Ok(()) => {
                    emit(ComputerEvent::ActionResult {
                        index: *ui_index,
                        success: true,
                        message: None,
                    });
                }
                Err(err) => {
                    emit(ComputerEvent::ActionResult {
                        index: *ui_index,
                        success: false,
                        message: Some(err.to_string()),
                    });
                    emit(ComputerEvent::status_code(
                        RunStatus::Error,
                        "smart_replay_step_failed",
                        format!(
                            "smart replay stopped at recorded step {}: {err}",
                            step.index + 1
                        ),
                    ));
                    return Err(());
                }
            }
            *ui_index += 1;
            continue;
        }

        let reference = load_reference_crop(recording_dir, step).await;
        let step_text = describe_step(step, total, skill_context);
        let step_monitor_id = step.monitor.map(|m| m.id);
        let mut recoveries: u32 = 0;

        loop {
            if cancel.is_cancelled() {
                emit(ComputerEvent::status(RunStatus::Stopped, None));
                return Err(());
            }
            if calls_used >= call_budget {
                emit(ComputerEvent::status_code(
                    RunStatus::Error,
                    "smart_replay_budget_exhausted",
                    format!(
                        "smart replay aborted at recorded step {}: vision call budget exhausted",
                        step.index + 1
                    ),
                ));
                return Err(());
            }

            let capture_result = match step_monitor_id {
                Some(id) => capture::capture_monitor(id).await,
                None => capture::capture_primary().await,
            };
            let screen = match capture_result {
                Ok(screen) => screen,
                Err(err) => {
                    emit(ComputerEvent::status_code(
                        RunStatus::Error,
                        "capture_failed",
                        format!("screen capture failed: {err}"),
                    ));
                    return Err(());
                }
            };
            let monitor = screen.monitor;
            let screen_uri = screen.data_uri();

            emit(ComputerEvent::status_code(
                RunStatus::Thinking,
                "smart_replay_locating",
                format!("locating the target for step {}/{total}", step.index + 1),
            ));

            let mut user_text = step_text.clone();
            if reference.is_some() {
                user_text.push_str(
                    "\nThe first image is the CURRENT screen. The second image is the recorded \
                     REFERENCE crop centered on the target element.",
                );
            } else {
                user_text.push_str("\nThe image is the CURRENT screen.");
            }
            let mut images: Vec<&str> = vec![screen_uri.as_str()];
            if let Some(reference) = reference.as_deref() {
                images.push(reference);
            }

            calls_used += 1;
            let locate_call =
                client.complete_with_images(SMART_GROUNDING_SYSTEM, &user_text, &images);
            let locate_result = tokio::select! {
                () = cancel.cancelled() => {
                    emit(ComputerEvent::status(RunStatus::Stopped, None));
                    return Err(());
                }
                outcome = tokio::time::timeout(
                    std::time::Duration::from_millis(SMART_GROUNDING_TIMEOUT_MS),
                    locate_call,
                ) => match outcome {
                    Ok(result) => result,
                    Err(_) => Err(anyhow::anyhow!(
                        "vision model did not respond within {}s",
                        SMART_GROUNDING_TIMEOUT_MS / 1000
                    )),
                },
            };
            let location = match locate_result.and_then(|raw| parse_smart_location(&raw)) {
                Ok(location) => location,
                Err(err) => {
                    if recoveries >= SMART_MAX_RECOVERIES_PER_STEP {
                        emit(ComputerEvent::status_code(
                            RunStatus::Error,
                            "smart_replay_grounding_failed",
                            format!(
                                "smart replay aborted at recorded step {}: {err}",
                                step.index + 1
                            ),
                        ));
                        return Err(());
                    }
                    recoveries += 1;
                    sleep_or_cancel(cancel, SMART_RECOVERY_SETTLE_MS).await;
                    continue;
                }
            };

            emit(ComputerEvent::status(RunStatus::Running, None));

            match location.status.as_str() {
                "found" => {
                    let Some((x_norm, y_norm)) = location.target else {
                        if recoveries >= SMART_MAX_RECOVERIES_PER_STEP {
                            emit(ComputerEvent::status_code(
                                RunStatus::Error,
                                "smart_replay_no_coords",
                                format!(
                                    "smart replay aborted at recorded step {}: model reported \
                                     the target as found but returned no coordinates",
                                    step.index + 1
                                ),
                            ));
                            return Err(());
                        }
                        recoveries += 1;
                        continue;
                    };
                    let to_norm = if step.action_type == "drag" {
                        location
                            .to_target
                            .or(step.to_x_norm.zip(step.to_y_norm))
                    } else {
                        None
                    };
                    let thought = if location.thought.is_empty() {
                        format!(
                            "Located the recorded target on the current screen (step {} of {total})",
                            step.index + 1
                        )
                    } else {
                        location.thought.clone()
                    };
                    emit(ComputerEvent::Step {
                        step: ComputerStepEvent {
                            index: *ui_index,
                            thought,
                            action_type: step.action_type.clone(),
                            element_description: step.element_description.clone(),
                            value: step.value.clone(),
                            screenshot_base64: screen.display_jpeg_base64.clone(),
                            screenshot_mime: "image/jpeg",
                            target_x_norm: Some(x_norm),
                            target_y_norm: Some(y_norm),
                            to_x_norm: to_norm.map(|t| t.0),
                            to_y_norm: to_norm.map(|t| t.1),
                            confidence: location.confidence,
                        },
                    });
                    let target = monitor.denormalize(x_norm, y_norm);
                    let to_target = to_norm.map(|(tx, ty)| monitor.denormalize(tx, ty));
                    let outcome = execute_action_at(
                        &step.action_type,
                        Some(target),
                        to_target,
                        step.value.as_deref(),
                        step.amount,
                        display_w,
                        display_h,
                        cancel,
                    )
                    .await;
                    match outcome {
                        Ok(()) => {
                            emit(ComputerEvent::ActionResult {
                                index: *ui_index,
                                success: true,
                                message: None,
                            });
                            *ui_index += 1;
                            break;
                        }
                        Err(err) => {
                            emit(ComputerEvent::ActionResult {
                                index: *ui_index,
                                success: false,
                                message: Some(err.to_string()),
                            });
                            emit(ComputerEvent::status_code(
                                RunStatus::Error,
                                "smart_replay_step_failed",
                                format!(
                                    "smart replay stopped at recorded step {}: {err}",
                                    step.index + 1
                                ),
                            ));
                            return Err(());
                        }
                    }
                }
                "obscured" => {
                    if recoveries >= SMART_MAX_RECOVERIES_PER_STEP {
                        emit(ComputerEvent::status_code(
                            RunStatus::Error,
                            "smart_replay_still_obscured",
                            format!(
                                "smart replay aborted at recorded step {}: the target remained \
                                 obscured after {SMART_MAX_RECOVERIES_PER_STEP} recovery attempts",
                                step.index + 1
                            ),
                        ));
                        return Err(());
                    }
                    recoveries += 1;
                    let Some(recovery) = location.recovery else {
                        sleep_or_cancel(cancel, SMART_RECOVERY_SETTLE_MS).await;
                        continue;
                    };
                    let thought = if location.thought.is_empty() {
                        "Clearing an obstruction covering the recorded target".to_string()
                    } else {
                        format!("Clearing obstruction: {}", location.thought)
                    };
                    emit(ComputerEvent::Step {
                        step: ComputerStepEvent {
                            index: *ui_index,
                            thought,
                            action_type: recovery.action.clone(),
                            element_description: None,
                            value: recovery.value.clone(),
                            screenshot_base64: screen.display_jpeg_base64.clone(),
                            screenshot_mime: "image/jpeg",
                            target_x_norm: recovery.target.map(|t| t.0),
                            target_y_norm: recovery.target.map(|t| t.1),
                            to_x_norm: recovery.to_target.map(|t| t.0),
                            to_y_norm: recovery.to_target.map(|t| t.1),
                            confidence: location.confidence,
                        },
                    });
                    let target = recovery.target.map(|(x, y)| monitor.denormalize(x, y));
                    let to_target = recovery.to_target.map(|(x, y)| monitor.denormalize(x, y));
                    let amount = if recovery.action == "wait" {
                        Some(
                            recovery
                                .amount
                                .map(|n| n.clamp(0, SMART_MAX_RECOVERY_WAIT_MS as i32))
                                .unwrap_or(800),
                        )
                    } else {
                        recovery.amount
                    };
                    let outcome = execute_action_at(
                        &recovery.action,
                        target,
                        to_target,
                        recovery.value.as_deref(),
                        amount,
                        display_w,
                        display_h,
                        cancel,
                    )
                    .await;
                    match outcome {
                        Ok(()) => {
                            emit(ComputerEvent::ActionResult {
                                index: *ui_index,
                                success: true,
                                message: None,
                            });
                        }
                        Err(err) => {
                            emit(ComputerEvent::ActionResult {
                                index: *ui_index,
                                success: false,
                                message: Some(err.to_string()),
                            });
                        }
                    }
                    *ui_index += 1;
                    sleep_or_cancel(cancel, SMART_RECOVERY_SETTLE_MS).await;
                    continue;
                }
                _ => {
                    if recoveries >= SMART_MAX_RECOVERIES_PER_STEP {
                        let reason = if location.thought.is_empty() {
                            "the target could not be found on the current screen".to_string()
                        } else {
                            location.thought.clone()
                        };
                        emit(ComputerEvent::status_code(
                            RunStatus::Error,
                            "smart_replay_not_found",
                            format!(
                                "smart replay aborted at recorded step {}: {reason}",
                                step.index + 1
                            ),
                        ));
                        return Err(());
                    }
                    recoveries += 1;
                    sleep_or_cancel(cancel, SMART_RECOVERY_SETTLE_MS).await;
                    continue;
                }
            }
        }
    }
    Ok(())
}
