// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Click,
    DoubleClick,
    RightClick,
    Type,
    KeyPress,
    Scroll,
    Drag,
    MoveMouse,
    Wait,
    Finished,
    CallUser,
}

impl ActionType {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionType::Click => "click",
            ActionType::DoubleClick => "double_click",
            ActionType::RightClick => "right_click",
            ActionType::Type => "type",
            ActionType::KeyPress => "key_press",
            ActionType::Scroll => "scroll",
            ActionType::Drag => "drag",
            ActionType::MoveMouse => "move_mouse",
            ActionType::Wait => "wait",
            ActionType::Finished => "finished",
            ActionType::CallUser => "call_user",
        }
    }

    pub fn needs_target(self) -> bool {
        matches!(
            self,
            ActionType::Click
                | ActionType::DoubleClick
                | ActionType::RightClick
                | ActionType::Scroll
                | ActionType::Drag
                | ActionType::MoveMouse
        )
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
            "click" | "left_click" | "left_single" | "tap" => Some(ActionType::Click),
            "double_click" | "left_double" | "doubleclick" => Some(ActionType::DoubleClick),
            "right_click" | "rightclick" | "context_click" => Some(ActionType::RightClick),
            "type" | "type_text" | "input" => Some(ActionType::Type),
            "key_press" | "hotkey" | "key" | "press" | "keypress" => Some(ActionType::KeyPress),
            "scroll" => Some(ActionType::Scroll),
            "drag" => Some(ActionType::Drag),
            "move_mouse" | "move" | "hover" => Some(ActionType::MoveMouse),
            "wait" | "sleep" => Some(ActionType::Wait),
            "finished" | "done" | "finish" | "complete" => Some(ActionType::Finished),
            "call_user" | "ask_user" | "request_user" => Some(ActionType::CallUser),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedAction {
    pub thought: String,
    pub action: ActionType,
    #[serde(default)]
    pub element_description: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub amount: Option<i32>,
    #[serde(default)]
    pub to_element_description: Option<String>,
    #[serde(default)]
    pub start_box: Option<Vec<f64>>,
    #[serde(default)]
    pub end_box: Option<Vec<f64>>,
}

pub fn parse_planned_action(raw_text: &str) -> Result<PlannedAction> {
    let json_str = extract_json_object(raw_text)
        .ok_or_else(|| anyhow!("planner response did not contain a JSON object: {raw_text}"))?;
    let value: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| anyhow!("failed to parse planner JSON: {e}; raw: {json_str}"))?;

    let action_raw = value
        .get("action")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("action_type").and_then(|v| v.as_str()))
        .ok_or_else(|| anyhow!("planner JSON missing 'action' field"))?;
    let action = ActionType::parse(action_raw)
        .ok_or_else(|| anyhow!("unknown action type: {action_raw}"))?;

    let thought = value
        .get("thought")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("reasoning").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();

    let element_description = value
        .get("element_description")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string);

    let value_field = value
        .get("value")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("text").and_then(|v| v.as_str()))
        .or_else(|| value.get("key").and_then(|v| v.as_str()))
        .or_else(|| value.get("direction").and_then(|v| v.as_str()))
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string);

    let amount = value
        .get("amount")
        .and_then(serde_json::Value::as_i64)
        .map(|n| n as i32);

    let to_element_description = value
        .get("to_element_description")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string);

    let start_box = parse_number_array(value.get("start_box"))
        .or_else(|| parse_number_array(value.get("box_2d")))
        .or_else(|| parse_number_array(value.get("point")));
    let end_box = parse_number_array(value.get("end_box"))
        .or_else(|| parse_number_array(value.get("to_box")));

    Ok(PlannedAction {
        thought,
        action,
        element_description,
        value: value_field,
        amount,
        to_element_description,
        start_box,
        end_box,
    })
}

fn parse_number_array(value: Option<&serde_json::Value>) -> Option<Vec<f64>> {
    let array = value?.as_array()?;
    let numbers: Vec<f64> = array
        .iter()
        .filter_map(serde_json::Value::as_f64)
        .collect();
    if numbers.len() == 2 || numbers.len() == 4 {
        Some(numbers)
    } else {
        None
    }
}

pub fn extract_json_object(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let without_fence = if let Some(stripped) = trimmed.strip_prefix("```") {
        let body = stripped
            .strip_prefix("json")
            .or_else(|| stripped.strip_prefix("JSON"))
            .unwrap_or(stripped);
        body.trim_end_matches("```").trim()
    } else {
        trimmed
    };

    let start = without_fence.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, ch) in without_fence[start..].char_indices() {
        match ch {
            '"' if !escaped => in_string = !in_string,
            '\\' if in_string => {
                escaped = !escaped;
                continue;
            }
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    let end = start + idx + 1;
                    return Some(without_fence[start..end].to_string());
                }
            }
            _ => {}
        }
        escaped = false;
    }
    None
}
