// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod engine;
pub mod event_matcher;
pub use engine::{
    Routine, RoutineAction, RoutineDispatchResult, RoutinesEngine, load_routines,
    load_routines_from_file, save_routines, save_routines_to_file,
};
pub use event_matcher::{EventPattern, MatchStrategy, RoutineEvent, matches, matches_any};

use parking_lot::Mutex;
use std::sync::OnceLock;

static ENGINE: OnceLock<Mutex<RoutinesEngine>> = OnceLock::new();

fn routines_workspace_dir() -> std::path::PathBuf {
    crate::services::try_get_services()
        .map(|svc| svc.config().workspace_dir.clone())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

fn global_engine() -> &'static Mutex<RoutinesEngine> {
    ENGINE.get_or_init(|| {
        let routines = load_routines(&routines_workspace_dir());
        if !routines.is_empty() {
            tracing::info!(count = routines.len(), "Loaded routines");
        }
        Mutex::new(RoutinesEngine::new(routines))
    })
}

pub fn list_routines() -> Vec<Routine> {
    global_engine().lock().routines().to_vec()
}

pub fn add_routine(routine: Routine) -> anyhow::Result<()> {
    let mut engine = global_engine().lock();
    if engine.routines().iter().any(|r| r.name == routine.name) {
        anyhow::bail!("routine '{}' already exists", routine.name);
    }
    engine.add_routine(routine);
    save_routines(&routines_workspace_dir(), engine.routines())
}

pub fn remove_routine(name: &str) -> anyhow::Result<bool> {
    let mut engine = global_engine().lock();
    if !engine.remove_routine(name) {
        return Ok(false);
    }
    save_routines(&routines_workspace_dir(), engine.routines())?;
    Ok(true)
}

pub fn reload_routines() -> usize {
    let routines = load_routines(&routines_workspace_dir());
    let count = routines.len();
    *global_engine().lock() = RoutinesEngine::new(routines);
    count
}

pub fn reset_cooldowns() {
    global_engine().lock().reset_cooldowns();
}

pub fn dispatch_event(source: &str, topic: &str, payload: Option<String>) {
    let results = {
        let mut engine = global_engine().lock();
        if engine.is_empty() {
            return;
        }
        engine.dispatch(&RoutineEvent {
            source: source.to_string(),
            topic: topic.to_string(),
            payload,
            timestamp: chrono::Utc::now().to_rfc3339(),
        })
    };

    for result in results {
        if let RoutineDispatchResult::Fired {
            routine_name,
            action,
        } = result
        {
            spawn_routine_action(routine_name, action);
        }
    }
}

fn spawn_routine_action(routine_name: String, action: RoutineAction) {
    let Some(svc) = crate::services::try_get_services() else {
        tracing::warn!(
            routine = %routine_name,
            "routines: runtime services unavailable, skipping fired action"
        );
        return;
    };
    let config = (*svc.config()).clone();
    let temperature = config.default_temperature;
    let prompt = match &action {
        RoutineAction::Shell { command } => format!(
            "An automation routine named '{routine_name}' fired. Run the following shell command \
             and report the outcome concisely:\n\n{command}"
        ),
        RoutineAction::Message { channel, text } => format!(
            "An automation routine named '{routine_name}' fired. Send the following message to the \
             '{channel}' channel:\n\n{text}"
        ),
        RoutineAction::Sop { name } => format!(
            "An automation routine named '{routine_name}' fired. Execute the standard operating \
             procedure (SOP) named '{name}'."
        ),
        RoutineAction::CronJob { job_name } => format!(
            "An automation routine named '{routine_name}' fired. Trigger the cron job named \
             '{job_name}'."
        ),
    };

    crate::runtime::spawn_supervised("routines.action", async move {
        match Box::pin(crate::agent::run(
            config,
            Some(prompt),
            None,
            None,
            temperature,
            Vec::new(),
            false,
            None,
            None,
            None,
        ))
        .await
        {
            Ok(_) => tracing::info!(routine = %routine_name, "routine action completed"),
            Err(e) => tracing::warn!(routine = %routine_name, error = %e, "routine action failed"),
        }
    });
}
