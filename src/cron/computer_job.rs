// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::types::{ComputerJobMode, ComputerJobSpec, CronJob};
use crate::computer::recorder::ReplayRepeat;
use crate::computer::run::{ComputerEvent, RunParams, RunStatus, UserMessage};
use crate::config::Config;

const DEFAULT_JOB_TIMEOUT_MS: u64 = 1_800_000;
const SHUTDOWN_GRACE_MS: u64 = 10_000;
const DEFAULT_MAX_STEPS: u32 = 40;
const MAX_ALLOWED_STEPS: u32 = 200;
const DEFAULT_STEP_DELAY_MS: u64 = 600;
const MAX_STEP_DELAY_MS: u64 = 10_000;

#[derive(Debug, Default)]
struct RunSummary {
    final_status: Option<RunStatus>,
    final_message: Option<String>,
    steps: u32,
    failures: u32,
    errors: Vec<String>,
}

impl RunSummary {
    fn describe(&self) -> String {
        let status = self
            .final_status
            .map(|s| format!("{s:?}").to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());
        let mut out = format!(
            "status={status}; steps={}; failures={}",
            self.steps, self.failures
        );
        if let Some(message) = &self.final_message {
            if !message.is_empty() {
                out.push_str(&format!("; message={message}"));
            }
        }
        if !self.errors.is_empty() {
            out.push_str(&format!("; errors={}", self.errors.join(" | ")));
        }
        out
    }
}

async fn collect_events(
    mut event_rx: mpsc::UnboundedReceiver<ComputerEvent>,
) -> RunSummary {
    let mut summary = RunSummary::default();
    while let Some(event) = event_rx.recv().await {
        match event {
            ComputerEvent::Status {
                status, message, ..
            } => {
                if matches!(
                    status,
                    RunStatus::Finished | RunStatus::Error | RunStatus::Stopped
                ) {
                    summary.final_status = Some(status);
                    summary.final_message = message;
                }
            }
            ComputerEvent::Step { .. } => summary.steps += 1,
            ComputerEvent::ActionResult { success, .. } => {
                if !success {
                    summary.failures += 1;
                }
            }
            ComputerEvent::Error { message, .. } => {
                if summary.errors.len() < 10 {
                    summary.errors.push(message);
                }
            }
            ComputerEvent::UserUpdate { .. } => {}
        }
    }
    summary
}

fn resolve_route(spec: &ComputerJobSpec, config: &Config) -> Option<(String, String)> {
    let explicit_provider = spec.provider.clone().filter(|s| !s.trim().is_empty());
    let explicit_model = spec.model.clone().filter(|s| !s.trim().is_empty());
    if let (Some(provider), Some(model)) = (explicit_provider, explicit_model) {
        return Some((provider, model));
    }
    if let (Some(provider), Some(model)) = (
        config.multimodal.vision_provider.as_deref(),
        config.multimodal.vision_model.as_deref(),
    ) {
        let provider = provider.trim();
        let model = model.trim();
        if !provider.is_empty() && !model.is_empty() {
            return Some((provider.to_string(), model.to_string()));
        }
    }
    let models = crate::computer::list_vision_models(config);
    let pick = models
        .iter()
        .find(|m| m.recommended)
        .or_else(|| models.first())?;
    Some((pick.provider.clone(), pick.model.clone()))
}

pub async fn run_computer_job(config: &Config, job: &CronJob) -> (bool, String) {
    let spec = match ComputerJobSpec::parse(&job.command) {
        Ok(spec) => spec,
        Err(e) => return (false, e),
    };

    let cancel = CancellationToken::new();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<ComputerEvent>();
    let collector = tokio::spawn(collect_events(event_rx));

    let run_task = match spec.mode {
        ComputerJobMode::Replay => {
            let Some(recording) = spec
                .recording
                .clone()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
            else {
                return (false, "computer replay job missing recording name".to_string());
            };
            let manifest = match crate::computer::recorder::load_recording(
                &config.workspace_dir,
                &recording,
            )
            .await
            {
                Ok(manifest) => manifest,
                Err(e) => return (false, format!("failed to load recording: {e}")),
            };
            let manifest_repeat = manifest.run_config.map(|rc| ReplayRepeat {
                count: rc.loop_count.max(1),
                interval_ms: rc.interval_ms,
            });
            let repeat = ReplayRepeat {
                count: spec
                    .loop_count
                    .unwrap_or_else(|| manifest_repeat.map_or(1, |r| r.count)),
                interval_ms: spec
                    .interval_ms
                    .unwrap_or_else(|| manifest_repeat.map_or(0, |r| r.interval_ms)),
            }
            .clamped();
            if spec.smart {
                let Some((provider, model)) = resolve_route(&spec, config) else {
                    return (
                        false,
                        "no vision provider/model configured for smart replay".to_string(),
                    );
                };
                let recording_dir = config.workspace_dir.join("skills").join(&recording);
                tokio::spawn(crate::computer::recorder::replay_recording_smart(
                    manifest,
                    recording_dir,
                    config.clone(),
                    provider,
                    model,
                    repeat,
                    cancel.clone(),
                    event_tx,
                ))
            } else {
                tokio::spawn(crate::computer::recorder::replay_recording(
                    manifest,
                    repeat,
                    cancel.clone(),
                    event_tx,
                ))
            }
        }
        ComputerJobMode::Agent => {
            let Some(task) = spec
                .task
                .clone()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
            else {
                return (false, "computer agent job missing task".to_string());
            };
            let Some((provider, model)) = resolve_route(&spec, config) else {
                return (
                    false,
                    "no vision provider/model configured for computer agent job".to_string(),
                );
            };
            let params = RunParams {
                run_id: format!("cron-{}", job.id),
                task,
                provider,
                model,
                max_steps: spec
                    .max_steps
                    .unwrap_or(DEFAULT_MAX_STEPS)
                    .clamp(1, MAX_ALLOWED_STEPS),
                step_delay_ms: spec
                    .step_delay_ms
                    .unwrap_or(DEFAULT_STEP_DELAY_MS)
                    .min(MAX_STEP_DELAY_MS),
                reference_images: Vec::new(),
                initial_history: Vec::new(),
            };
            let (_user_tx, user_rx) = mpsc::unbounded_channel::<UserMessage>();
            tokio::spawn(crate::computer::run::run_loop(
                params,
                config.clone(),
                cancel.clone(),
                event_tx,
                user_rx,
            ))
        }
    };

    let timeout_ms = job.max_duration_ms.unwrap_or(DEFAULT_JOB_TIMEOUT_MS).max(1_000);
    let mut run_task = run_task;
    let mut timed_out = false;
    tokio::select! {
        _ = &mut run_task => {}
        () = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)) => {
            timed_out = true;
            cancel.cancel();
            let _ = tokio::time::timeout(
                std::time::Duration::from_millis(SHUTDOWN_GRACE_MS),
                &mut run_task,
            )
            .await;
        }
    }

    let summary = collector.await.unwrap_or_default();
    let description = summary.describe();
    if timed_out {
        return (
            false,
            format!("computer job timed out after {timeout_ms}ms; {description}"),
        );
    }
    let success = matches!(summary.final_status, Some(RunStatus::Finished));
    (success, description)
}
