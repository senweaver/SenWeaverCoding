// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlaybookStep {

    pub action: String,

    pub description: String,

    #[serde(default)]
    pub requires_approval: bool,

    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_timeout_secs() -> u64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Playbook {

    pub name: String,

    pub description: String,

    pub steps: Vec<PlaybookStep>,

    #[serde(default = "default_severity_filter")]
    pub severity_filter: String,

    #[serde(default)]
    pub auto_approve_steps: Vec<usize>,
}

fn default_severity_filter() -> String {
    "medium".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepExecutionResult {
    pub step_index: usize,
    pub action: String,
    pub status: StepStatus,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepStatus {

    Completed,

    PendingApproval,

    Skipped,

    Failed,
}

impl std::fmt::Display for StepStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Completed => write!(f, "completed"),
            Self::PendingApproval => write!(f, "pending_approval"),
            Self::Skipped => write!(f, "skipped"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

pub fn load_playbooks(dir: &Path) -> Vec<Playbook> {
    let mut playbooks = Vec::new();

    if !dir.exists() || !dir.is_dir() {
        return builtin_playbooks();
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "json") {
                match std::fs::read_to_string(&path) {
                    Ok(contents) => match serde_json::from_str::<Playbook>(&contents) {
                        Ok(pb) => playbooks.push(pb),
                        Err(e) => {
                            tracing::warn!("Failed to parse playbook {}: {e}", path.display());
                        }
                    },
                    Err(e) => {
                        tracing::warn!("Failed to read playbook {}: {e}", path.display());
                    }
                }
            }
        }
    }

    for builtin in builtin_playbooks() {
        if !playbooks.iter().any(|p| p.name == builtin.name) {
            playbooks.push(builtin);
        }
    }

    playbooks
}

pub fn severity_level(severity: &str) -> u8 {
    match severity.to_lowercase().as_str() {
        "low" => 1,
        "medium" => 2,
        "high" => 3,
        "critical" => 4,

        _ => u8::MAX,
    }
}

pub fn can_auto_approve(
    playbook: &Playbook,
    step_index: usize,
    alert_severity: &str,
    max_auto_severity: &str,
) -> bool {

    if severity_level(alert_severity) > severity_level(max_auto_severity) {
        return false;
    }

    playbook.auto_approve_steps.contains(&step_index)
}

pub fn evaluate_step(
    playbook: &Playbook,
    step_index: usize,
    alert_severity: &str,
    max_auto_severity: &str,
    require_approval: bool,
) -> StepExecutionResult {
    let step = match playbook.steps.get(step_index) {
        Some(s) => s,
        None => {
            return StepExecutionResult {
                step_index,
                action: "unknown".into(),
                status: StepStatus::Failed,
                message: format!("Step index {step_index} out of range"),
            };
        }
    };

    if step.requires_approval
        && (!require_approval
            || !can_auto_approve(playbook, step_index, alert_severity, max_auto_severity))
    {
        return StepExecutionResult {
            step_index,
            action: step.action.clone(),
            status: StepStatus::PendingApproval,
            message: format!(
                "Step '{}' requires human approval (severity: {alert_severity})",
                step.description
            ),
        };
    }

    StepExecutionResult {
        step_index,
        action: step.action.clone(),
        status: StepStatus::Completed,
        message: format!("Executed: {}", step.description),
    }
}

pub fn builtin_playbooks() -> Vec<Playbook> {
    vec![
        Playbook {
            name: "suspicious_login".into(),
            description: "Respond to suspicious login activity detected by SIEM".into(),
            steps: vec![
                PlaybookStep {
                    action: "gather_login_context".into(),
                    description: "Collect login metadata: IP, geo, device fingerprint, time".into(),
                    requires_approval: false,
                    timeout_secs: 60,
                },
                PlaybookStep {
                    action: "check_threat_intel".into(),
                    description: "Query threat intelligence for source IP reputation".into(),
                    requires_approval: false,
                    timeout_secs: 30,
                },
                PlaybookStep {
                    action: "notify_user".into(),
                    description: "Send verification notification to account owner".into(),
                    requires_approval: true,
                    timeout_secs: 300,
                },
                PlaybookStep {
                    action: "force_password_reset".into(),
                    description: "Force password reset if login confirmed unauthorized".into(),
                    requires_approval: true,
                    timeout_secs: 120,
                },
            ],
            severity_filter: "medium".into(),
            auto_approve_steps: vec![0, 1],
        },
        Playbook {
            name: "malware_detected".into(),
            description: "Respond to malware detection on endpoint".into(),
            steps: vec![
                PlaybookStep {
                    action: "isolate_endpoint".into(),
                    description: "Network-isolate the affected endpoint".into(),
                    requires_approval: true,
                    timeout_secs: 60,
                },
                PlaybookStep {
                    action: "collect_forensics".into(),
                    description: "Capture memory dump and disk image for analysis".into(),
                    requires_approval: false,
                    timeout_secs: 600,
                },
                PlaybookStep {
                    action: "scan_lateral_movement".into(),
                    description: "Check for lateral movement indicators on adjacent hosts".into(),
                    requires_approval: false,
                    timeout_secs: 300,
                },
                PlaybookStep {
                    action: "remediate_endpoint".into(),
                    description: "Remove malware and restore endpoint to clean state".into(),
                    requires_approval: true,
                    timeout_secs: 600,
                },
            ],
            severity_filter: "high".into(),
            auto_approve_steps: vec![1, 2],
        },
        Playbook {
            name: "data_exfiltration_attempt".into(),
            description: "Respond to suspected data exfiltration".into(),
            steps: vec![
                PlaybookStep {
                    action: "block_egress".into(),
                    description: "Block suspicious outbound connections".into(),
                    requires_approval: true,
                    timeout_secs: 30,
                },
                PlaybookStep {
                    action: "identify_data_scope".into(),
                    description: "Determine what data may have been accessed or transferred".into(),
                    requires_approval: false,
                    timeout_secs: 300,
                },
                PlaybookStep {
                    action: "preserve_evidence".into(),
                    description: "Preserve network logs and access records".into(),
                    requires_approval: false,
                    timeout_secs: 120,
                },
                PlaybookStep {
                    action: "escalate_to_legal".into(),
                    description: "Notify legal and compliance teams".into(),
                    requires_approval: true,
                    timeout_secs: 60,
                },
            ],
            severity_filter: "critical".into(),
            auto_approve_steps: vec![1, 2],
        },
        Playbook {
            name: "brute_force".into(),
            description: "Respond to brute force authentication attempts".into(),
            steps: vec![
                PlaybookStep {
                    action: "block_source_ip".into(),
                    description: "Block the attacking source IP at firewall".into(),
                    requires_approval: true,
                    timeout_secs: 30,
                },
                PlaybookStep {
                    action: "check_compromised_accounts".into(),
                    description: "Check if any accounts were successfully compromised".into(),
                    requires_approval: false,
                    timeout_secs: 120,
                },
                PlaybookStep {
                    action: "enable_rate_limiting".into(),
                    description: "Enable enhanced rate limiting on auth endpoints".into(),
                    requires_approval: true,
                    timeout_secs: 60,
                },
            ],
            severity_filter: "medium".into(),
            auto_approve_steps: vec![1],
        },
    ]
}
