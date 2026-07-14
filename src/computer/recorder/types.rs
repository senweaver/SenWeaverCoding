// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone)]
pub enum RawInputEvent {
    Key {
        down: bool,
        vk: u16,
        scan: u32,
        ctrl: bool,
        alt: bool,
        shift: bool,
        win: bool,
        caps: bool,
    },
    MouseButton {
        button: MouseButton,
        down: bool,
        x: i32,
        y: i32,
    },
    MouseMove {
        x: i32,
        y: i32,
    },
    Wheel {
        delta: i32,
        horizontal: bool,
        x: i32,
        y: i32,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecordedStep {
    pub index: u32,
    pub action_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_norm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y_norm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_x_norm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_y_norm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_abs: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y_abs: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_x_abs: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_y_abs: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<i32>,
    #[serde(default)]
    pub delay_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitor: Option<crate::computer::coordinates::MonitorRect>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct RunConfig {
    #[serde(default)]
    pub loop_count: u32,
    #[serde(default)]
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecordingManifest {
    pub rec_id: String,
    pub task: String,
    pub created_at: String,
    pub display_w: i32,
    pub display_h: i32,
    pub steps: Vec<RecordedStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_config: Option<RunConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordingSummary {
    pub name: String,
    pub task: String,
    pub created_at: String,
    pub step_count: usize,
    pub has_skill: bool,
    pub has_trace: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecorderStatus {
    Recording,
    Stopped,
    Generating,
    Saved,
    Error,
    Idle,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecorderStepEvent {
    pub index: u32,
    pub action_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub screenshot_base64: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_x_norm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_y_norm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_x_norm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_y_norm: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecorderEvent {
    Status {
        status: RecorderStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    Step {
        #[serde(flatten)]
        step: RecorderStepEvent,
    },
    RecordingSaved {
        name: String,
    },
    SkillSaved {
        name: String,
    },
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
}

impl RecorderEvent {
    pub fn status(status: RecorderStatus, message: Option<String>) -> Self {
        RecorderEvent::Status {
            status,
            message,
            code: None,
        }
    }

    pub fn status_code(status: RecorderStatus, code: &str, message: impl Into<String>) -> Self {
        RecorderEvent::Status {
            status,
            message: Some(message.into()),
            code: Some(code.to_string()),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        RecorderEvent::Error {
            message: message.into(),
            code: None,
        }
    }

    pub fn error_code(code: &str, message: impl Into<String>) -> Self {
        RecorderEvent::Error {
            message: message.into(),
            code: Some(code.to_string()),
        }
    }
}
