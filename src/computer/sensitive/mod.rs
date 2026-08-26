// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod detect;
pub mod frame_redact;
pub mod ocr;
pub mod scan;
pub mod secrets;

use std::path::Path;

pub use frame_redact::{FramePolicy, FrameRedactor, RedactStats};
pub use scan::{
    load_report, save_report, scan_recording, ScanOutcome, SensitiveFinding, SensitiveReport,
    TextRedactor,
};

const PRIVACY_SETTINGS_FILE: &str = "computer_privacy.json";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacySettings {
    #[serde(default = "default_true")]
    pub advanced_protection: bool,
}

fn default_true() -> bool {
    true
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            advanced_protection: true,
        }
    }
}

pub fn load_privacy_settings(workspace_dir: &Path) -> PrivacySettings {
    std::fs::read_to_string(workspace_dir.join(PRIVACY_SETTINGS_FILE))
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

pub fn save_privacy_settings(workspace_dir: &Path, settings: &PrivacySettings) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(settings)?;
    std::fs::write(workspace_dir.join(PRIVACY_SETTINGS_FILE), bytes)?;
    Ok(())
}

pub struct RedactionContext {
    pub text: TextRedactor,
    pub frame_policy: FramePolicy,
    pub frame_redactor: Option<FrameRedactor>,
    pub report: Option<SensitiveReport>,
}

impl RedactionContext {
    pub fn inactive() -> Self {
        Self {
            text: TextRedactor::passthrough(),
            frame_policy: FramePolicy::Inactive,
            frame_redactor: None,
            report: None,
        }
    }

    pub fn redact_text(&self, text: &str) -> String {
        self.text.redact(text)
    }
}

pub fn build_redaction(dir: &Path, session_id: &str, enabled: bool) -> RedactionContext {
    if !enabled {
        save_report(dir, None);
        return RedactionContext::inactive();
    }
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scan_recording(dir, session_id)
    }));
    match outcome {
        Ok(outcome) => {
            let frame_policy = if ocr::ocr_available() {
                FramePolicy::Redact
            } else {
                FramePolicy::Withhold
            };
            let frame_redactor = match frame_policy {
                FramePolicy::Redact => Some(FrameRedactor::new(outcome.values.clone())),
                _ => None,
            };
            RedactionContext {
                text: TextRedactor::new(outcome.values),
                frame_policy,
                frame_redactor,
                report: Some(outcome.report),
            }
        }
        Err(_) => {
            tracing::warn!("sensitive scan panicked; withholding frames for this analysis");
            RedactionContext {
                text: TextRedactor::passthrough(),
                frame_policy: FramePolicy::Withhold,
                frame_redactor: None,
                report: None,
            }
        }
    }
}
