// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc::UnboundedSender;

use super::analysis::{
    load_analysis, parse_submission, save_analysis, Analysis, AnalysisFeedback, FeedbackEntry,
    FeedbackStepNote,
};
use super::instructions::{DESCRIBER_INSTRUCTIONS, KICKOFF_PROMPT, NUDGE_PROMPT};
use super::tools::run_tool;
use crate::computer::action::extract_json_object;
use crate::computer::sensitive;
use crate::computer::vision::VisionClient;
use crate::config::Config;
use crate::providers::traits::ChatMessage;

const MAX_TURNS: usize = 24;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnalyzeEvent {
    Progress {
        phase: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    Analysis {
        analysis: Analysis,
        #[serde(skip_serializing_if = "Option::is_none")]
        redacted_count: Option<usize>,
    },
    Error {
        message: String,
    },
}

fn emit(tx: &UnboundedSender<AnalyzeEvent>, phase: &str, message: Option<String>) {
    let _ = tx.send(AnalyzeEvent::Progress {
        phase: phase.to_string(),
        message,
    });
}

pub struct AnalyzeRequest {
    pub dir: PathBuf,
    pub session_id: String,
    pub provider: String,
    pub model: String,
    pub feedback: Option<AnalysisFeedback>,
}

pub async fn run_analyze(
    config: &Config,
    workspace_dir: &Path,
    request: AnalyzeRequest,
    tx: &UnboundedSender<AnalyzeEvent>,
) -> Result<Analysis> {
    let AnalyzeRequest {
        dir,
        session_id,
        provider,
        model,
        feedback,
    } = request;

    let refining = feedback
        .as_ref()
        .is_some_and(|f| f.overall.is_some() || !f.steps.is_empty());
    emit(
        tx,
        "start",
        Some(if refining {
            "Revising the analysis…".to_string()
        } else {
            "Reconstructing the session…".to_string()
        }),
    );

    if crate::computer::narration::transcribe::has_pending_transcription(&dir) {
        emit(tx, "working", Some("Transcribing narration…".to_string()));
        let started_epoch = crate::computer::activity::events::read_events(&dir)
            .first()
            .map(|e| e.epoch)
            .unwrap_or(0);
        if let Err(e) =
            crate::computer::narration::transcribe::transcribe_recording(config, &dir, started_epoch)
                .await
        {
            tracing::warn!("pre-analysis narration transcription failed: {e}");
        }
    }

    let advanced = sensitive::load_privacy_settings(workspace_dir).advanced_protection;
    emit(tx, "working", Some("Checking for sensitive details…".to_string()));
    let scan_dir = dir.clone();
    let scan_session = session_id.clone();
    let mut redaction = tokio::task::spawn_blocking(move || {
        sensitive::build_redaction(&scan_dir, &scan_session, advanced)
    })
    .await
    .map_err(|e| anyhow!("sensitive scan task failed: {e}"))?;

    let client = VisionClient::from_config(config, &provider, &model)
        .map_err(|e| anyhow!("failed to initialize model '{model}': {e}"))?;

    let started_epoch = crate::computer::activity::events::read_events(&dir)
        .first()
        .map(|e| e.epoch)
        .unwrap_or(0);

    let mut messages: Vec<ChatMessage> = vec![ChatMessage::system(DESCRIBER_INSTRUCTIONS)];
    let prior = load_analysis(&dir);
    let kickoff = match (&feedback, &prior) {
        (Some(fb), Some(prior)) if refining => render_feedback_prompt(fb, prior),
        _ => KICKOFF_PROMPT.to_string(),
    };
    messages.push(ChatMessage::user(kickoff));

    emit(tx, "working", Some("Thinking…".to_string()));
    let mut nudged = false;
    let mut turns = 0usize;

    loop {
        turns += 1;
        if turns > MAX_TURNS {
            return Err(anyhow!("analysis exceeded the maximum number of tool calls"));
        }

        let raw = client
            .complete_messages(&messages)
            .await
            .map_err(|e| anyhow!("model request failed: {e}"))?;
        messages.push(ChatMessage::assistant(raw.clone()));

        let Some(json) = extract_json_object(&raw) else {
            if !nudged {
                nudged = true;
                messages.push(ChatMessage::user(NUDGE_PROMPT.to_string()));
                continue;
            }
            return Err(anyhow!("model did not return a tool call"));
        };
        let call: serde_json::Value = serde_json::from_str(&json)
            .map_err(|e| anyhow!("failed to parse the model's tool call: {e}"))?;
        let tool = call
            .get("tool")
            .and_then(|v| v.as_str())
            .or_else(|| call.get("name").and_then(|v| v.as_str()))
            .unwrap_or("");
        let args = call
            .get("args")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));

        if tool == "submit_analysis" {
            match finalize(&dir, &session_id, &args, prior.as_ref(), feedback.as_ref()) {
                Ok(analysis) => {
                    let redacted_count = redaction
                        .report
                        .as_ref()
                        .map(|r| r.total_findings)
                        .filter(|n| *n > 0);
                    if let Some(report) = redaction.report.as_ref() {
                        let mut report = report.clone();
                        let stats = redaction
                            .frame_redactor
                            .as_ref()
                            .map(|r| r.stats())
                            .unwrap_or_default();
                        if stats.frames_blurred > 0 {
                            report.images = Some(sensitive::scan::SensitiveImagesSummary {
                                frames_blurred: stats.frames_blurred,
                                regions_blurred: stats.regions_blurred,
                            });
                        }
                        sensitive::save_report(&dir, Some(&report));
                    }
                    emit(tx, "done", Some("Analysis ready.".to_string()));
                    let _ = tx.send(AnalyzeEvent::Analysis {
                        analysis: analysis.clone(),
                        redacted_count,
                    });
                    return Ok(analysis);
                }
                Err(e) => {
                    messages.push(ChatMessage::user(format!(
                        "submit_analysis was rejected: {e}. Fix the issues and call submit_analysis again as a single JSON tool call."
                    )));
                    continue;
                }
            }
        }

        if tool.is_empty() {
            if !nudged {
                nudged = true;
                messages.push(ChatMessage::user(NUDGE_PROMPT.to_string()));
                continue;
            }
            return Err(anyhow!("model tool call was missing a 'tool' name"));
        }

        emit(tx, "working", Some(tool_progress(tool)));
        let output = run_tool(&dir, started_epoch, &mut redaction, tool, &args);
        let mut reply = ChatMessage::user(format!("Tool result ({tool}):\n{}", output.text));
        if !output.images.is_empty() {
            let mut content = reply.content.clone();
            for uri in output.images.iter().take(super::tools::MAX_IMAGES_PER_CALL) {
                content.push_str("\n\n[IMAGE:");
                content.push_str(uri);
                content.push(']');
            }
            reply.content = content;
        }
        messages.push(reply);
    }
}

fn tool_progress(tool: &str) -> String {
    match tool {
        "get_timeline" => "Reading the timeline…".to_string(),
        "get_events" => "Reading events…".to_string(),
        "get_narration" => "Reading narration…".to_string(),
        "list_frames" => "Listing keyframes…".to_string(),
        "get_frames" => "Looking at the screen…".to_string(),
        other => format!("Running {other}…"),
    }
}

fn finalize(
    dir: &Path,
    session_id: &str,
    args: &serde_json::Value,
    prior: Option<&Analysis>,
    feedback: Option<&AnalysisFeedback>,
) -> Result<Analysis> {
    let (title, intent, confidence, rationale, steps) = parse_submission(args)?;
    let revision = prior.map(|p| p.revision + 1).unwrap_or(1);
    let mut feedback_log = prior.map(|p| p.feedback_log.clone()).unwrap_or_default();
    if let Some(fb) = feedback {
        if fb.overall.is_some() || !fb.steps.is_empty() {
            feedback_log.push(FeedbackEntry {
                revision,
                at: chrono::Utc::now().timestamp_millis(),
                overall: fb.overall.clone(),
                steps: fb
                    .steps
                    .iter()
                    .map(|s| FeedbackStepNote {
                        step_id: s.step_id.clone(),
                        note: s.note.clone(),
                    })
                    .collect(),
            });
        }
    }
    let narration_source_updated_at = crate::computer::narration::transcript_updated_at(dir);
    let analysis = Analysis {
        version: 1,
        session_id: session_id.to_string(),
        revision,
        created_at: chrono::Utc::now().timestamp_millis(),
        narration_source_updated_at,
        title,
        intent,
        intent_confidence: confidence,
        intent_rationale: rationale,
        steps,
        feedback_log,
        approved: prior.map(|p| p.approved).unwrap_or(false),
        approved_at: prior.and_then(|p| p.approved_at),
    };
    save_analysis(dir, &analysis)?;
    Ok(analysis)
}

fn render_feedback_prompt(feedback: &AnalysisFeedback, prior: &Analysis) -> String {
    let mut lines = vec![
        "The user reviewed your analysis and left feedback. Revise the ENTIRE analysis and call"
            .to_string(),
        "submit_analysis again. Keep step ids stable where a step is unchanged.".to_string(),
        String::new(),
        format!("Current intent: {}", prior.intent),
    ];
    if !prior.steps.is_empty() {
        lines.push("Current steps:".to_string());
        for step in &prior.steps {
            lines.push(format!("- {} ({}): {}", step.id, step.title, step.detail));
        }
    }
    lines.push(String::new());
    if let Some(overall) = &feedback.overall {
        lines.push(format!("Overall feedback: {overall}"));
    }
    for note in &feedback.steps {
        lines.push(format!("Feedback on step {}: {}", note.step_id, note.note));
    }
    lines.join("\n")
}
