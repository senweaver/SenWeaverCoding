// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Duration;

use parking_lot::Mutex;
use std::sync::Arc;
use tracing::{info, warn};

use super::audit::SopAuditLogger;
use super::engine::{now_iso8601, SopEngine};
use super::types::{SopRunAction, SopStepResult, SopStepStatus};

const SOP_STEP_AGENT_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const SOP_STEP_CHAIN_CAP: u32 = 64;
const SOP_HEADLESS_MAX_PARALLEL: usize = 2;

fn inflight_runs() -> &'static Mutex<HashSet<String>> {
    static INFLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    INFLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

fn headless_slots() -> &'static tokio::sync::Semaphore {
    static SLOTS: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    SLOTS.get_or_init(|| tokio::sync::Semaphore::new(SOP_HEADLESS_MAX_PARALLEL))
}

fn try_claim_run(run_id: &str) -> bool {
    inflight_runs().lock().insert(run_id.to_string())
}

fn release_run(run_id: &str) {
    inflight_runs().lock().remove(run_id);
}

pub fn enqueue_action(
    engine: Arc<Mutex<SopEngine>>,
    audit: Arc<SopAuditLogger>,
    action: SopRunAction,
) {
    let run_id_for_mark = match &action {
        SopRunAction::ExecuteStep { run_id, .. }
        | SopRunAction::WaitApproval { run_id, .. }
        | SopRunAction::DeterministicStep { run_id, .. }
        | SopRunAction::CheckpointWait { run_id, .. } => Some(run_id.clone()),
        SopRunAction::Completed { .. } | SopRunAction::Failed { .. } => None,
    };
    if let Some(ref rid) = run_id_for_mark {
        engine.lock().mark_headless_driven(rid);
    }

    match action {
        SopRunAction::ExecuteStep {
            run_id,
            step,
            context,
        } => {
            if !try_claim_run(&run_id) {
                info!(
                    "SOP headless: run {run_id} already executing; skipping duplicate enqueue"
                );
                return;
            }
            crate::runtime::spawn_supervised("sop.headless_execute", async move {
                let _guard = RunClaimGuard {
                    run_id: run_id.clone(),
                };
                drive_execute_chain(engine, audit, run_id, step.number, context).await;
            });
        }
        SopRunAction::WaitApproval { run_id, step, .. } => {
            info!(
                "SOP headless: run {run_id} waiting for approval on step {} '{}'",
                step.number, step.title
            );
        }
        SopRunAction::DeterministicStep { run_id, step, .. } => {
            info!(
                "SOP headless: run {run_id} deterministic step {} '{}' requires external driver",
                step.number, step.title
            );
        }
        SopRunAction::CheckpointWait {
            run_id,
            step,
            state_file,
            ..
        } => {
            info!(
                "SOP headless: run {run_id} checkpoint at step {} '{}', state at {}",
                step.number,
                step.title,
                state_file.display()
            );
        }
        SopRunAction::Completed { run_id, sop_name } => {
            info!("SOP headless: run {run_id} ('{sop_name}') completed");
            audit_finished_run(&engine, &audit, &run_id);
        }
        SopRunAction::Failed {
            run_id,
            sop_name,
            reason,
        } => {
            warn!("SOP headless: run {run_id} ('{sop_name}') failed: {reason}");
            audit_finished_run(&engine, &audit, &run_id);
        }
    }
}

fn audit_finished_run(
    engine: &Arc<Mutex<SopEngine>>,
    audit: &Arc<SopAuditLogger>,
    run_id: &str,
) {
    let snapshot = {
        let eng = engine.lock();
        eng.finished_runs(None)
            .into_iter()
            .rev()
            .find(|r| r.run_id == run_id)
            .cloned()
    };
    if let Some(run) = snapshot {
        let audit = Arc::clone(audit);
        crate::runtime::spawn_supervised("sop.headless_audit_complete", async move {
            if let Err(e) = audit.log_run_complete(&run).await {
                warn!("SOP headless: audit log_run_complete failed: {e}");
            }
        });
    }
}

struct RunClaimGuard {
    run_id: String,
}

impl Drop for RunClaimGuard {
    fn drop(&mut self) {
        release_run(&self.run_id);
    }
}

async fn fail_and_enqueue(
    engine: Arc<Mutex<SopEngine>>,
    audit: Arc<SopAuditLogger>,
    run_id: &str,
    reason: String,
) {
    let action = {
        let mut eng = engine.lock();
        eng.fail_run(run_id, reason)
    };
    match action {
        Ok(action) => enqueue_action(engine, audit, action),
        Err(e) => warn!("SOP headless: fail_run({run_id}) failed: {e}"),
    }
}

async fn drive_execute_chain(
    engine: Arc<Mutex<SopEngine>>,
    audit: Arc<SopAuditLogger>,
    run_id: String,
    mut step_number: u32,
    mut context: String,
) {
    let Some(services) = crate::services::try_get_services() else {
        warn!(
            "SOP headless: run {run_id} step {step_number} ready but services unavailable"
        );
        fail_and_enqueue(
            engine,
            audit,
            &run_id,
            "services unavailable for headless SOP execution".into(),
        )
        .await;
        return;
    };

    let config = (*services.shared_config.load()).clone();
    let security = crate::agent::cli_runtime::build_security(&config);
    if !security.can_act() {
        fail_and_enqueue(
            engine,
            audit,
            &run_id,
            "blocked by security policy: autonomy is read-only".into(),
        )
        .await;
        return;
    }
    if security.is_rate_limited() {
        fail_and_enqueue(
            engine,
            audit,
            &run_id,
            "blocked by security policy: rate limit exceeded".into(),
        )
        .await;
        return;
    }
    if !security.record_action() {
        fail_and_enqueue(
            engine,
            audit,
            &run_id,
            "blocked by security policy: action budget exhausted".into(),
        )
        .await;
        return;
    }

    let Ok(_slot) = headless_slots().acquire().await else {
        fail_and_enqueue(
            engine,
            audit,
            &run_id,
            "headless SOP execution slot closed".into(),
        )
        .await;
        return;
    };

    {
        let eng = engine.lock();
        if !eng.active_runs().contains_key(&run_id) {
            drop(eng);
            info!(
                "SOP headless: run {run_id} no longer active after waiting for execution slot"
            );
            audit_finished_run(&engine, &audit, &run_id);
            return;
        }
    }

    let mut chain = 0u32;
    loop {
        chain += 1;
        if chain > SOP_STEP_CHAIN_CAP {
            fail_and_enqueue(
                engine,
                audit,
                &run_id,
                format!("hit headless step chain cap ({SOP_STEP_CHAIN_CAP})"),
            )
            .await;
            return;
        }

        let allowed_tools = {
            let eng = engine.lock();
            let sop_name = eng
                .active_runs()
                .get(&run_id)
                .map(|r| r.sop_name.clone());
            match sop_name {
                Some(name) => eng
                    .sops()
                    .iter()
                    .find(|s| s.name == name)
                    .and_then(|sop| {
                        sop.steps
                            .iter()
                            .find(|s| s.number == step_number)
                            .map(|s| {
                                s.suggested_tools
                                    .iter()
                                    .filter(|t| !t.starts_with("sop_"))
                                    .cloned()
                                    .collect::<Vec<_>>()
                            })
                    })
                    .filter(|t| !t.is_empty()),
                None => {
                    drop(eng);
                    warn!("SOP headless: run {run_id} vanished before step {step_number}");
                    audit_finished_run(&engine, &audit, &run_id);
                    return;
                }
            }
        };

        let config = (*services.shared_config.load()).clone();
        let temperature = config.default_temperature;
        let prompt = format!(
            "[sop:{run_id} step {step_number}]\n{context}\n\n\
             Complete this SOP step, then summarize the result. Do not call sop_advance \
             or sop_approve; the headless driver records completion from your final reply."
        );

        info!("SOP headless: executing run {run_id} step {step_number} via agent");

        let started_at = now_iso8601();
        let deny = vec!["sop_".to_string()];
        let run_future = crate::agent::loop_::TOOL_DENY_PREFIXES.scope(deny, async {
            crate::agent::run(
                config,
                Some(prompt),
                None,
                None,
                temperature,
                vec![],
                false,
                None,
                allowed_tools,
                None,
            )
            .await
        });

        let step_result = match tokio::time::timeout(SOP_STEP_AGENT_TIMEOUT, run_future).await {
            Ok(Ok(output)) => SopStepResult {
                step_number,
                status: SopStepStatus::Completed,
                output: if output.trim().is_empty() {
                    "step completed".to_string()
                } else {
                    output
                },
                started_at,
                completed_at: Some(now_iso8601()),
            },
            Ok(Err(err)) => SopStepResult {
                step_number,
                status: SopStepStatus::Failed,
                output: format!("agent step failed: {err}"),
                started_at,
                completed_at: Some(now_iso8601()),
            },
            Err(_) => SopStepResult {
                step_number,
                status: SopStepStatus::Failed,
                output: format!(
                    "agent step timed out after {}s",
                    SOP_STEP_AGENT_TIMEOUT.as_secs()
                ),
                started_at,
                completed_at: Some(now_iso8601()),
            },
        };

        if let Err(e) = audit.log_step_result(&run_id, &step_result).await {
            warn!("SOP headless: audit log_step_result failed for {run_id}: {e}");
        }

        let already_recorded = {
            let eng = engine.lock();
            match eng.active_runs().get(&run_id) {
                Some(run) => run
                    .step_results
                    .iter()
                    .any(|s| s.step_number == step_number),
                None => {
                    info!(
                        "SOP headless: run {run_id} no longer active after step {step_number}"
                    );
                    drop(eng);
                    audit_finished_run(&engine, &audit, &run_id);
                    return;
                }
            }
        };

        let next = if already_recorded {
            info!(
                "SOP headless: run {run_id} step {step_number} already recorded by agent tools; \
                 resuming from engine state"
            );
            let resume = {
                let eng = engine.lock();
                eng.headless_resume_action(&run_id)
            };
            match resume {
                Some(action) => Ok(action),
                None => {
                    fail_and_enqueue(
                        Arc::clone(&engine),
                        Arc::clone(&audit),
                        &run_id,
                        "headless resume found no pending action after external step record"
                            .into(),
                    )
                    .await;
                    return;
                }
            }
        } else {
            let mut eng = engine.lock();
            eng.advance_step(&run_id, step_result)
        };

        match next {
            Ok(SopRunAction::ExecuteStep {
                run_id: next_id,
                step,
                context: next_ctx,
            }) => {
                if next_id != run_id {
                    warn!(
                        "SOP headless: unexpected run id change {run_id} -> {next_id}; stopping"
                    );
                    return;
                }
                step_number = step.number;
                context = next_ctx;
            }
            Ok(other) => {
                enqueue_action(engine, audit, other);
                return;
            }
            Err(e) => {
                warn!("SOP headless: advance/resume failed for run {run_id}: {e}");
                audit_finished_run(&engine, &audit, &run_id);
                return;
            }
        }
    }
}
