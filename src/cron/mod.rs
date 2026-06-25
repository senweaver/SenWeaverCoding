// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::config::Config;
use crate::security::SecurityPolicy;
use anyhow::{Result, anyhow, bail};

mod legacy_import;
mod schedule;
mod store;
mod types;

pub mod scheduler;
pub use legacy_import::import_legacy_auto_dream;
pub use schedule::{
    next_run_for_schedule, normalize_expression, schedule_cron_expression, validate_schedule,
};
pub use store::{
    activity_jobs, add_agent_job, all_overdue_jobs, claim_job, due_jobs, finalize_run, get_job,
    list_jobs, list_runs, record_last_run, record_run, remove_job, reschedule_after_run,
    reset_running_runs, reset_stale_running, start_run, sync_declarative_jobs, update_job,
};
pub use types::{
    AgentJobOptions, CronJob, CronJobPatch, CronRun, DeliveryConfig, JobType, Schedule, SessionTarget,
    deserialize_maybe_stringified,
};

pub fn validate_shell_command(config: &Config, command: &str, approved: bool) -> Result<()> {
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);
    validate_shell_command_with_security(&security, command, approved)
}

pub(crate) fn validate_shell_command_with_security(
    security: &SecurityPolicy,
    command: &str,
    approved: bool,
) -> Result<()> {
    security
        .validate_command_execution(command, approved)
        .map(|_| ())
        .map_err(|reason| anyhow!("blocked by security policy: {reason}"))
}

pub(crate) fn validate_delivery_config(delivery: Option<&DeliveryConfig>) -> Result<()> {
    let Some(delivery) = delivery else {
        return Ok(());
    };

    if delivery.mode.eq_ignore_ascii_case("none") {
        return Ok(());
    }
    if !delivery.mode.eq_ignore_ascii_case("announce") {
        bail!("unsupported delivery mode: {}", delivery.mode);
    }

    let channel = delivery.channel.as_deref().map(str::trim);
    let Some(channel) = channel.filter(|value| !value.is_empty()) else {
        bail!("delivery.channel is required for announce mode");
    };
    match channel.to_ascii_lowercase().as_str() {
        "telegram" | "discord" | "slack" | "mattermost" | "signal" | "matrix" | "qq" => {}
        other => bail!("unsupported delivery channel: {other}"),
    }

    let has_target = delivery
        .to
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if !has_target {
        bail!("delivery.to is required for announce mode");
    }

    Ok(())
}

pub fn add_shell_job_with_approval(
    config: &Config,
    name: Option<String>,
    schedule: Schedule,
    command: &str,
    delivery: Option<DeliveryConfig>,
    approved: bool,
) -> Result<CronJob> {
    validate_shell_command(config, command, approved)?;
    validate_delivery_config(delivery.as_ref())?;
    store::add_shell_job(config, name, schedule, command, delivery)
}

pub fn update_shell_job_with_approval(
    config: &Config,
    job_id: &str,
    patch: CronJobPatch,
    approved: bool,
) -> Result<CronJob> {
    if let Some(command) = patch.command.as_deref() {
        validate_shell_command(config, command, approved)?;
    }
    update_job(config, job_id, patch)
}

pub fn add_once_validated(
    config: &Config,
    delay: &str,
    command: &str,
    approved: bool,
) -> Result<CronJob> {
    let duration = parse_delay(delay)?;
    let at = chrono::Utc::now() + duration;
    add_once_at_validated(config, at, command, approved)
}

pub fn add_once_at_validated(
    config: &Config,
    at: chrono::DateTime<chrono::Utc>,
    command: &str,
    approved: bool,
) -> Result<CronJob> {
    let schedule = Schedule::At { at };
    add_shell_job_with_approval(config, None, schedule, command, None, approved)
}

pub(crate) fn add_shell_job(
    config: &Config,
    name: Option<String>,
    schedule: Schedule,
    command: &str,
) -> Result<CronJob> {
    add_shell_job_with_approval(config, name, schedule, command, None, false)
}

#[allow(clippy::needless_pass_by_value)]
pub fn handle_command(command: crate::CronCommands, config: &Config) -> Result<()> {
    match command {
        crate::CronCommands::List => {
            let jobs = list_jobs(config)?;
            if jobs.is_empty() {
                println!("No scheduled tasks yet.");
                println!("\nUsage:");
                println!("  sen cron add '0 9 * * *' 'agent -m \"Good morning!\"'");
                return Ok(());
            }

            println!("🕒 Scheduled jobs ({}):", jobs.len());
            for job in jobs {
                let last_run = job
                    .last_run
                    .map_or_else(|| "never".into(), |d| d.to_rfc3339());
                let last_status = job.last_status.unwrap_or_else(|| "n/a".into());
                println!(
                    "- {} | {:?} | next={} | last={} ({})",
                    job.id,
                    job.schedule,
                    job.next_run.to_rfc3339(),
                    last_run,
                    last_status,
                );
                if !job.command.is_empty() {
                    println!("    cmd: {}", job.command);
                }
                if let Some(prompt) = &job.prompt {
                    println!("    prompt: {prompt}");
                }
            }
            Ok(())
        }
        crate::CronCommands::Add {
            expression,
            tz,
            agent,
            allowed_tools,
            command,
        } => {
            let schedule = Schedule::Cron {
                expr: expression,
                tz,
            };
            if agent {
                let job = add_agent_job(
                    config,
                    None,
                    schedule,
                    &command,
                    AgentJobOptions {
                        session_target: SessionTarget::Isolated,
                        allowed_tools: if allowed_tools.is_empty() {
                            None
                        } else {
                            Some(allowed_tools)
                        },
                        ..Default::default()
                    },
                )?;
                println!("✅ Added agent cron job {}", job.id);
                println!("  Expr  : {}", job.expression);
                println!("  Next  : {}", job.next_run.to_rfc3339());
                println!("  Prompt: {}", job.prompt.as_deref().unwrap_or_default());
            } else {
                if !allowed_tools.is_empty() {
                    bail!("--allowed-tool is only supported with --agent cron jobs");
                }
                let job = add_shell_job(config, None, schedule, &command)?;
                println!("✅ Added cron job {}", job.id);
                println!("  Expr: {}", job.expression);
                println!("  Next: {}", job.next_run.to_rfc3339());
                println!("  Cmd : {}", job.command);
            }
            Ok(())
        }
        crate::CronCommands::AddAt {
            at,
            agent,
            allowed_tools,
            command,
        } => {
            let at = chrono::DateTime::parse_from_rfc3339(&at)
                .map_err(|e| anyhow::anyhow!("Invalid RFC3339 timestamp for --at: {e}"))?
                .with_timezone(&chrono::Utc);
            let schedule = Schedule::At { at };
            if agent {
                let job = add_agent_job(
                    config,
                    None,
                    schedule,
                    &command,
                    AgentJobOptions {
                        session_target: SessionTarget::Isolated,
                        delete_after_run: true,
                        allowed_tools: if allowed_tools.is_empty() {
                            None
                        } else {
                            Some(allowed_tools)
                        },
                        ..Default::default()
                    },
                )?;
                println!("✅ Added one-shot agent cron job {}", job.id);
                println!("  At    : {}", job.next_run.to_rfc3339());
                println!("  Prompt: {}", job.prompt.as_deref().unwrap_or_default());
            } else {
                if !allowed_tools.is_empty() {
                    bail!("--allowed-tool is only supported with --agent cron jobs");
                }
                let job = add_shell_job(config, None, schedule, &command)?;
                println!("✅ Added one-shot cron job {}", job.id);
                println!("  At  : {}", job.next_run.to_rfc3339());
                println!("  Cmd : {}", job.command);
            }
            Ok(())
        }
        crate::CronCommands::AddEvery {
            every_ms,
            agent,
            allowed_tools,
            command,
        } => {
            let schedule = Schedule::Every { every_ms };
            if agent {
                let job = add_agent_job(
                    config,
                    None,
                    schedule,
                    &command,
                    AgentJobOptions {
                        session_target: SessionTarget::Isolated,
                        allowed_tools: if allowed_tools.is_empty() {
                            None
                        } else {
                            Some(allowed_tools)
                        },
                        ..Default::default()
                    },
                )?;
                println!("✅ Added interval agent cron job {}", job.id);
                println!("  Every(ms): {every_ms}");
                println!("  Next     : {}", job.next_run.to_rfc3339());
                println!("  Prompt   : {}", job.prompt.as_deref().unwrap_or_default());
            } else {
                if !allowed_tools.is_empty() {
                    bail!("--allowed-tool is only supported with --agent cron jobs");
                }
                let job = add_shell_job(config, None, schedule, &command)?;
                println!("✅ Added interval cron job {}", job.id);
                println!("  Every(ms): {every_ms}");
                println!("  Next     : {}", job.next_run.to_rfc3339());
                println!("  Cmd      : {}", job.command);
            }
            Ok(())
        }
        crate::CronCommands::Once {
            delay,
            agent,
            allowed_tools,
            command,
        } => {
            if agent {
                let duration = parse_delay(&delay)?;
                let at = chrono::Utc::now() + duration;
                let schedule = Schedule::At { at };
                let job = add_agent_job(
                    config,
                    None,
                    schedule,
                    &command,
                    AgentJobOptions {
                        session_target: SessionTarget::Isolated,
                        delete_after_run: true,
                        allowed_tools: if allowed_tools.is_empty() {
                            None
                        } else {
                            Some(allowed_tools)
                        },
                        ..Default::default()
                    },
                )?;
                println!("✅ Added one-shot agent cron job {}", job.id);
                println!("  At    : {}", job.next_run.to_rfc3339());
                println!("  Prompt: {}", job.prompt.as_deref().unwrap_or_default());
            } else {
                if !allowed_tools.is_empty() {
                    bail!("--allowed-tool is only supported with --agent cron jobs");
                }
                let job = add_once(config, &delay, &command)?;
                println!("✅ Added one-shot cron job {}", job.id);
                println!("  At  : {}", job.next_run.to_rfc3339());
                println!("  Cmd : {}", job.command);
            }
            Ok(())
        }
        crate::CronCommands::Update {
            id,
            expression,
            tz,
            command,
            name,
            allowed_tools,
        } => {
            if expression.is_none()
                && tz.is_none()
                && command.is_none()
                && name.is_none()
                && allowed_tools.is_empty()
            {
                bail!(
                    "At least one of --expression, --tz, --command, --name, or --allowed-tool must be provided"
                );
            }

            let existing = if expression.is_some() || tz.is_some() || !allowed_tools.is_empty() {
                Some(get_job(config, &id)?)
            } else {
                None
            };

            let schedule = if expression.is_some() || tz.is_some() {
                let existing = existing
                    .as_ref()
                    .expect("existing job must be loaded when updating schedule");
                let (existing_expr, existing_tz) = match &existing.schedule {
                    Schedule::Cron {
                        expr,
                        tz: existing_tz,
                    } => (expr.clone(), existing_tz.clone()),
                    _ => bail!("Cannot update expression/tz on a non-cron schedule"),
                };
                Some(Schedule::Cron {
                    expr: expression.unwrap_or(existing_expr),
                    tz: tz.or(existing_tz),
                })
            } else {
                None
            };

            if !allowed_tools.is_empty() {
                let existing = existing
                    .as_ref()
                    .expect("existing job must be loaded when updating allowed tools");
                if existing.job_type != JobType::Agent {
                    bail!("--allowed-tool is only supported for agent cron jobs");
                }
            }

            let patch = CronJobPatch {
                schedule,
                command,
                name,
                allowed_tools: if allowed_tools.is_empty() {
                    None
                } else {
                    Some(allowed_tools)
                },
                ..CronJobPatch::default()
            };

            let job = update_shell_job_with_approval(config, &id, patch, false)?;
            println!("\u{2705} Updated cron job {}", job.id);
            println!("  Expr: {}", job.expression);
            println!("  Next: {}", job.next_run.to_rfc3339());
            println!("  Cmd : {}", job.command);
            Ok(())
        }
        crate::CronCommands::Remove { id } => remove_job(config, &id),
        crate::CronCommands::Pause { id } => {
            pause_job(config, &id)?;
            println!("⏸️  Paused cron job {id}");
            Ok(())
        }
        crate::CronCommands::Resume { id } => {
            resume_job(config, &id)?;
            println!("▶️  Resumed cron job {id}");
            Ok(())
        }
    }
}

pub(crate) fn add_once(config: &Config, delay: &str, command: &str) -> Result<CronJob> {
    add_once_validated(config, delay, command, false)
}

pub fn pause_job(config: &Config, id: &str) -> Result<CronJob> {
    update_job(
        config,
        id,
        CronJobPatch {
            enabled: Some(false),
            ..CronJobPatch::default()
        },
    )
}

pub fn resume_job(config: &Config, id: &str) -> Result<CronJob> {
    update_job(
        config,
        id,
        CronJobPatch {
            enabled: Some(true),
            ..CronJobPatch::default()
        },
    )
}

fn parse_delay(input: &str) -> Result<chrono::Duration> {
    let input = input.trim();
    if input.is_empty() {
        anyhow::bail!("delay must not be empty");
    }
    let split = input
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(input.len());
    let (num, unit) = input.split_at(split);
    let amount: i64 = num.parse()?;
    let unit = if unit.is_empty() { "m" } else { unit };
    let duration = match unit {
        "s" => chrono::Duration::seconds(amount),
        "m" => chrono::Duration::minutes(amount),
        "h" => chrono::Duration::hours(amount),
        "d" => chrono::Duration::days(amount),
        _ => anyhow::bail!("unsupported delay unit '{unit}', use s/m/h/d"),
    };
    Ok(duration)
}
