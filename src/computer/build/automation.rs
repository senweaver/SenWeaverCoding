// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::skill::slugify_skill_name;
use super::values::{render_values, FixedValue};

pub const AUTOMATION_PLAN_FILE: &str = "automation-plan.json";
pub const BUILT_AUTOMATION_FILE: &str = "built-automation.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeOfDay {
    pub hour: u32,
    pub minute: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AutomationSchedule {
    #[serde(rename_all = "camelCase")]
    Single {
        #[serde(default)]
        natural_language: String,
        #[serde(default)]
        days: Vec<u32>,
        time: TimeOfDay,
    },
    #[serde(rename_all = "camelCase")]
    Interval {
        #[serde(default)]
        natural_language: String,
        #[serde(default)]
        days: Vec<u32>,
        #[serde(default = "default_interval_minutes")]
        interval_minutes: u32,
        anchor: TimeOfDay,
    },
    #[serde(rename_all = "camelCase")]
    Multi {
        #[serde(default)]
        natural_language: String,
        #[serde(default)]
        days: Vec<u32>,
        times: Vec<TimeOfDay>,
    },
}

fn default_interval_minutes() -> u32 {
    60
}

impl AutomationSchedule {
    pub fn primary_time(&self) -> TimeOfDay {
        match self {
            AutomationSchedule::Single { time, .. } => time.clone(),
            AutomationSchedule::Interval { anchor, .. } => anchor.clone(),
            AutomationSchedule::Multi { times, .. } => {
                times.first().cloned().unwrap_or(TimeOfDay { hour: 9, minute: 0 })
            }
        }
    }

    pub fn describe(&self) -> String {
        let nl = match self {
            AutomationSchedule::Single { natural_language, .. }
            | AutomationSchedule::Interval { natural_language, .. }
            | AutomationSchedule::Multi { natural_language, .. } => natural_language,
        };
        if !nl.trim().is_empty() {
            return nl.trim().to_string();
        }
        let t = self.primary_time();
        let time = format!("{:02}:{:02}", t.hour, t.minute);
        match self {
            AutomationSchedule::Single { .. } => format!("Once a day at {time}"),
            AutomationSchedule::Interval {
                interval_minutes, ..
            } => format!("Every {interval_minutes} min from {time}"),
            AutomationSchedule::Multi { times, .. } => format!("{}× a day", times.len()),
        }
    }

    pub fn to_cron_expr(&self) -> String {
        match self {
            AutomationSchedule::Single { days, time, .. } => {
                cron_expr(time.minute, time.hour, days)
            }
            AutomationSchedule::Interval { anchor, .. } => {
                cron_expr(anchor.minute, anchor.hour, &[])
            }
            AutomationSchedule::Multi { days, times, .. } => {
                let t = times.first().cloned().unwrap_or(TimeOfDay { hour: 9, minute: 0 });
                cron_expr(t.minute, t.hour, days)
            }
        }
    }
}

fn cron_expr(minute: u32, hour: u32, days: &[u32]) -> String {
    let dow = if days.is_empty() {
        "*".to_string()
    } else {
        let mut d: Vec<String> = days.iter().map(|d| d.to_string()).collect();
        d.dedup();
        d.join(",")
    };
    format!("{minute} {hour} * * {dow}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationStep {
    #[serde(default)]
    pub label: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationPlan {
    pub architecture: String,
    pub name: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub generalization: String,
    pub trigger_type: String,
    pub schedule: AutomationSchedule,
    #[serde(default)]
    pub condition: String,
    #[serde(default)]
    pub values: Vec<FixedValue>,
    #[serde(default)]
    pub steps: Vec<AutomationStep>,
    #[serde(default)]
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltAutomation {
    pub version: u32,
    pub session_id: String,
    pub architecture: String,
    pub name: String,
    pub description: String,
    pub trigger_type: String,
    pub schedule: AutomationSchedule,
    #[serde(default)]
    pub condition: String,
    #[serde(default)]
    pub steps: Vec<AutomationStep>,
    #[serde(default)]
    pub values: Vec<FixedValue>,
    #[serde(default)]
    pub model: String,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exported_path: Option<String>,
}

fn parse_time(value: Option<&serde_json::Value>) -> TimeOfDay {
    let hour = value
        .and_then(|v| v.get("hour"))
        .and_then(|v| v.as_u64())
        .unwrap_or(9)
        .min(23) as u32;
    let minute = value
        .and_then(|v| v.get("minute"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .min(59) as u32;
    TimeOfDay { hour, minute }
}

fn parse_days(value: Option<&serde_json::Value>) -> Vec<u32> {
    value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_u64())
                .map(|d| (d % 7) as u32)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_schedule(value: Option<&serde_json::Value>) -> AutomationSchedule {
    let obj = value.cloned().unwrap_or_else(|| serde_json::json!({}));
    let kind = obj.get("kind").and_then(|v| v.as_str()).unwrap_or("single");
    let natural_language = obj
        .get("naturalLanguage")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let days = parse_days(obj.get("days"));
    match kind {
        "interval" => {
            let interval_minutes = obj
                .get("intervalMinutes")
                .and_then(|v| v.as_u64())
                .filter(|m| *m > 0 && *m <= 1440 && 1440 % *m == 0)
                .unwrap_or(60) as u32;
            AutomationSchedule::Interval {
                natural_language,
                days,
                interval_minutes,
                anchor: parse_time(obj.get("anchor")),
            }
        }
        "multi" => {
            let times = obj
                .get("times")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().map(|t| parse_time(Some(t))).collect::<Vec<_>>())
                .filter(|t: &Vec<TimeOfDay>| !t.is_empty())
                .unwrap_or_else(|| vec![TimeOfDay { hour: 9, minute: 0 }]);
            AutomationSchedule::Multi {
                natural_language,
                days,
                times,
            }
        }
        _ => AutomationSchedule::Single {
            natural_language,
            days,
            time: parse_time(obj.get("time")),
        },
    }
}

pub fn parse_plan(architecture: &str, args: &serde_json::Value) -> Result<AutomationPlan> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(slugify_skill_name)
        .ok_or_else(|| anyhow!("propose_automation_plan requires a 'name'"))?;
    let description = args
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or(&name)
        .trim()
        .to_string();
    let trigger = args
        .get("trigger")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let trigger_type = trigger
        .get("type")
        .and_then(|v| v.as_str())
        .filter(|t| *t == "condition")
        .unwrap_or("schedule")
        .to_string();
    let steps = args
        .get("steps")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|raw| {
                    let prompt = raw.get("prompt").and_then(|v| v.as_str())?;
                    if prompt.trim().is_empty() {
                        return None;
                    }
                    Some(AutomationStep {
                        label: raw
                            .get("label")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        prompt: prompt.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(AutomationPlan {
        architecture: architecture.to_string(),
        name,
        title,
        description,
        summary: args
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        generalization: args
            .get("generalization")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        trigger_type,
        schedule: parse_schedule(trigger.get("schedule")),
        condition: trigger
            .get("condition")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        values: super::values::parse_values(args.get("values")),
        steps,
        model: args
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

pub fn built_from_plan(session_id: &str, plan: &AutomationPlan) -> Result<BuiltAutomation> {
    if plan.steps.is_empty() {
        return Err(anyhow!("an automation needs at least one step"));
    }
    Ok(BuiltAutomation {
        version: 1,
        session_id: session_id.to_string(),
        architecture: plan.architecture.clone(),
        name: slugify_skill_name(&plan.name),
        description: plan.description.clone(),
        trigger_type: plan.trigger_type.clone(),
        schedule: plan.schedule.clone(),
        condition: plan.condition.clone(),
        steps: plan.steps.clone(),
        values: plan.values.clone(),
        model: plan.model.clone(),
        created_at: chrono::Utc::now().timestamp_millis(),
        exported_path: None,
    })
}

pub fn rendered_prompt(built: &BuiltAutomation) -> String {
    let mut lines = Vec::new();
    for step in &built.steps {
        let label = render_values(&step.label, &built.values);
        let prompt = render_values(&step.prompt, &built.values);
        if label.trim().is_empty() {
            lines.push(prompt);
        } else {
            lines.push(format!("{label}: {prompt}"));
        }
    }
    lines.join("\n\n")
}

pub fn render_automation_json(built: &BuiltAutomation) -> String {
    let steps: Vec<serde_json::Value> = built
        .steps
        .iter()
        .map(|s| {
            serde_json::json!({
                "label": render_values(&s.label, &built.values),
                "prompt": render_values(&s.prompt, &built.values),
            })
        })
        .collect();
    let obj = serde_json::json!({
        "name": slugify_skill_name(&built.name),
        "description": built.description,
        "triggerType": built.trigger_type,
        "schedule": built.schedule,
        "condition": built.condition,
        "steps": steps,
        "model": built.model,
    });
    serde_json::to_string_pretty(&obj).unwrap_or_default() + "\n"
}

pub fn save_plan(dir: &Path, plan: &AutomationPlan) {
    if let Ok(bytes) = serde_json::to_vec_pretty(plan) {
        let _ = std::fs::write(dir.join(AUTOMATION_PLAN_FILE), bytes);
    }
}

pub fn load_plan(dir: &Path) -> Option<AutomationPlan> {
    let content = std::fs::read_to_string(dir.join(AUTOMATION_PLAN_FILE)).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn persist_built(dir: &Path, built: &BuiltAutomation) {
    if let Ok(bytes) = serde_json::to_vec_pretty(built) {
        let _ = std::fs::write(dir.join(BUILT_AUTOMATION_FILE), bytes);
    }
}
