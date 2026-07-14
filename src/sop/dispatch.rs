// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::Mutex;
use std::sync::Arc;

use tracing::{debug, info, warn};

use super::audit::SopAuditLogger;
use super::engine::{SopEngine, now_iso8601};
use super::types::{SopEvent, SopRun, SopRunAction, SopTriggerSource};

#[derive(Debug, Clone)]
pub enum DispatchResult {

    Started {
        run_id: String,
        sop_name: String,
        action: Box<SopRunAction>,
    },

    Skipped { sop_name: String, reason: String },

    NoMatch,
}

fn extract_run_id_from_action(action: &SopRunAction) -> &str {
    match action {
        SopRunAction::ExecuteStep { run_id, .. }
        | SopRunAction::WaitApproval { run_id, .. }
        | SopRunAction::DeterministicStep { run_id, .. }
        | SopRunAction::CheckpointWait { run_id, .. }
        | SopRunAction::Completed { run_id, .. }
        | SopRunAction::Failed { run_id, .. } => run_id,
    }
}

fn action_label(action: &SopRunAction) -> &'static str {
    match action {
        SopRunAction::ExecuteStep { .. } => "ExecuteStep",
        SopRunAction::WaitApproval { .. } => "WaitApproval",
        SopRunAction::DeterministicStep { .. } => "DeterministicStep",
        SopRunAction::CheckpointWait { .. } => "CheckpointWait",
        SopRunAction::Completed { .. } => "Completed",
        SopRunAction::Failed { .. } => "Failed",
    }
}

pub async fn dispatch_sop_event(
    engine: &Arc<Mutex<SopEngine>>,
    audit: &SopAuditLogger,
    event: SopEvent,
) -> Vec<DispatchResult> {

    let matched_names: Vec<String> = {
        let eng = engine.lock();
        eng.match_trigger(&event)
            .iter()
            .map(|s| s.name.clone())
            .collect()
    };

    if matched_names.is_empty() {
        debug!("SOP dispatch: no match for event");
        return vec![DispatchResult::NoMatch];
    }

    info!(
        "SOP dispatch: {} SOP(s) matched: {:?}",
        matched_names.len(),
        matched_names
    );

    let mut results = Vec::new();
    let mut started_runs: Vec<SopRun> = Vec::new();

    {
        let mut eng = engine.lock();

        for sop_name in &matched_names {
            match eng.start_run(sop_name, event.clone()) {
                Ok(action) => {

                    let run_id = extract_run_id_from_action(&action).to_string();

                    if let Some(run) = eng.active_runs().get(&run_id) {
                        started_runs.push(run.clone());
                    }
                    info!(
                        "SOP dispatch: started '{}' run {run_id} (action: {})",
                        sop_name,
                        action_label(&action),
                    );
                    results.push(DispatchResult::Started {
                        run_id,
                        sop_name: sop_name.clone(),
                        action: Box::new(action),
                    });
                }
                Err(e) => {
                    info!("SOP dispatch: skipped '{}': {e}", sop_name);
                    results.push(DispatchResult::Skipped {
                        sop_name: sop_name.clone(),
                        reason: e.to_string(),
                    });
                }
            }
        }
    }

    for run in &started_runs {
        if let Err(e) = audit.log_run_start(run).await {
            warn!("SOP dispatch: audit log failed for run {}: {e}", run.run_id);
        }
    }

    crate::health::mark_component_ok("sop_dispatch");
    results
}

pub fn process_headless_results(
    engine: &Arc<Mutex<SopEngine>>,
    audit: &Arc<SopAuditLogger>,
    results: &[DispatchResult],
) {
    for result in results {
        match result {
            DispatchResult::Started {
                run_id: _,
                sop_name,
                action,
            } => {
                info!(
                    "SOP headless dispatch: '{sop_name}' -> {}",
                    action_label(action.as_ref())
                );
                super::runner::enqueue_action(
                    Arc::clone(engine),
                    Arc::clone(audit),
                    action.as_ref().clone(),
                );
            }
            DispatchResult::Skipped { sop_name, reason } => {
                info!("SOP headless dispatch: skipped '{sop_name}': {reason}");
            }
            DispatchResult::NoMatch => {}
        }
    }
}

pub async fn dispatch_peripheral_signal(
    engine: &Arc<Mutex<SopEngine>>,
    audit: &SopAuditLogger,
    board: &str,
    signal: &str,
    payload: Option<&str>,
) -> Vec<DispatchResult> {
    let event = SopEvent {
        source: SopTriggerSource::Peripheral,
        topic: Some(format!("{board}/{signal}")),
        payload: payload.map(String::from),
        timestamp: now_iso8601(),
    };
    dispatch_sop_event(engine, audit, event).await
}

#[derive(Clone)]
pub struct SopCronCache {

    schedules: Vec<(String, String, cron::Schedule)>,
}

impl SopCronCache {

    pub fn from_engine(engine: &Arc<Mutex<SopEngine>>) -> Self {
        let mut schedules = Vec::new();
        let eng = engine.lock();

        for sop in eng.sops() {
            for trigger in &sop.triggers {
                if let super::types::SopTrigger::Cron { expression } = trigger {

                    let normalized = match crate::cron::normalize_expression(expression) {
                        Ok(n) => n,
                        Err(e) => {
                            warn!(
                                "SopCronCache: invalid cron expression '{}' in SOP '{}': {e}",
                                expression, sop.name
                            );
                            continue;
                        }
                    };
                    match normalized.parse::<cron::Schedule>() {
                        Ok(schedule) => {
                            schedules.push((sop.name.clone(), expression.clone(), schedule));
                        }
                        Err(e) => {
                            warn!(
                                "SopCronCache: failed to parse cron schedule '{}' for SOP '{}': {e}",
                                normalized, sop.name
                            );
                        }
                    }
                }
            }
        }

        debug!("SopCronCache: cached {} cron schedule(s)", schedules.len());
        Self { schedules }
    }

}

const SOP_MAINTENANCE_INTERVAL_SECS: u64 = 30;

static SOP_MAINTENANCE_STARTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn maintenance_audit_slot() -> &'static parking_lot::Mutex<Option<Arc<SopAuditLogger>>> {
    static SLOT: std::sync::OnceLock<parking_lot::Mutex<Option<Arc<SopAuditLogger>>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| parking_lot::Mutex::new(None))
}

fn resolve_maintenance_audit() -> Arc<SopAuditLogger> {
    maintenance_audit_slot()
        .lock()
        .clone()
        .unwrap_or_else(|| {
            Arc::new(SopAuditLogger::new(Arc::new(
                crate::memory::none::NoneMemory::new(),
            )))
        })
}

pub fn ensure_sop_maintenance(
    engine: Arc<Mutex<SopEngine>>,
    audit: Option<Arc<SopAuditLogger>>,
    workspace_dir: std::path::PathBuf,
) {
    if let Some(a) = audit {
        *maintenance_audit_slot().lock() = Some(a);
    }
    if SOP_MAINTENANCE_STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    crate::runtime::spawn_supervised("sop.maintenance", async move {
        {
            let mut eng = engine.lock();
            if eng.sops().is_empty() {
                eng.reload(&workspace_dir);
            }
        }
        let mut last_cron_check = chrono::Utc::now();
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(
            SOP_MAINTENANCE_INTERVAL_SECS,
        ));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let audit = resolve_maintenance_audit();

            let timeout_actions = engine.lock().check_approval_timeouts();
            for action in timeout_actions {
                info!(
                    "SOP maintenance: approval timeout advanced run {} ({})",
                    extract_run_id_from_action(&action),
                    action_label(&action),
                );
                super::runner::enqueue_action(Arc::clone(&engine), Arc::clone(&audit), action);
            }

            let reaped = engine
                .lock()
                .reap_stale_runs(super::engine::SopEngine::MAX_RUN_LIFETIME_SECS);
            for action in reaped {
                warn!(
                    "SOP maintenance: reaped stale run {} ({})",
                    extract_run_id_from_action(&action),
                    action_label(&action),
                );
                super::runner::enqueue_action(Arc::clone(&engine), Arc::clone(&audit), action);
            }

            let cache = SopCronCache::from_engine(&engine);
            if cache.schedules.is_empty() {
                continue;
            }
            let results =
                check_sop_cron_triggers(&engine, &audit, &cache, &mut last_cron_check).await;
            process_headless_results(&engine, &audit, &results);
        }
    });
}

pub async fn check_sop_cron_triggers(
    engine: &Arc<Mutex<SopEngine>>,
    audit: &SopAuditLogger,
    cache: &SopCronCache,
    last_check: &mut chrono::DateTime<chrono::Utc>,
) -> Vec<DispatchResult> {
    let now = chrono::Utc::now();
    let mut all_results = Vec::new();
    let mut fired_expressions = std::collections::HashSet::new();

    for (_sop_name, expression, schedule) in &cache.schedules {
        let mut upcoming = schedule.after(last_check);
        if let Some(next) = upcoming.next() {
            if next <= now {
                if !fired_expressions.insert(expression.clone()) {
                    continue;
                }
                let event = SopEvent {
                    source: SopTriggerSource::Cron,
                    topic: Some(expression.clone()),
                    payload: None,
                    timestamp: now_iso8601(),
                };
                let results = dispatch_sop_event(engine, audit, event).await;
                all_results.extend(results);
            }
        }
    }

    *last_check = now;
    all_results
}

