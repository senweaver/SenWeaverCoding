// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
#[cfg(feature = "channel-matrix")]
use crate::channels::MatrixChannel;
#[cfg(feature = "whatsapp-web")]
use crate::channels::WhatsAppWebChannel;
use crate::channels::{
    Channel, DiscordChannel, MattermostChannel, QQChannel, SendMessage, SignalChannel,
    SlackChannel, TelegramChannel,
};
use crate::agent::coding_mode::CodingMode;
use crate::config::Config;
use crate::config::schema::{AutonomyConfig, CronJobDecl, CronScheduleDecl};
use crate::cron::{
    CronJob, CronJobPatch, DeliveryConfig, JobType, Schedule, SessionTarget, all_overdue_jobs,
    due_jobs, next_run_for_schedule, record_last_run, record_run, remove_job, reschedule_after_run,
    sync_declarative_jobs, update_job,
};
use crate::security::{AutonomyLevel, SecurityPolicy};
use anyhow::Result;
use chrono::{DateTime, Utc};
use futures_util::{StreamExt, stream};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::time::{self, Duration};

const MIN_POLL_SECONDS: u64 = 5;
const SHELL_JOB_TIMEOUT_SECS: u64 = 120;
const SCHEDULER_COMPONENT: &str = "scheduler";

fn apply_cron_permission_mode(autonomy: &mut AutonomyConfig, permission_mode: Option<&str>) {
    let Some(raw) = permission_mode.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    match raw {
        "bypassPermissions" => autonomy.level = AutonomyLevel::Full,
        "acceptEdits" => {
            autonomy.level = AutonomyLevel::Supervised;
            for t in ["file_write", "file_edit", "multi_edit", "glob_edit", "notebook_edit"] {
                let s = (*t).to_string();
                if !autonomy.auto_approve.iter().any(|x| x == &s) {
                    autonomy.auto_approve.push(s);
                }
            }
        }
        "default" | "askEveryTime" => autonomy.level = AutonomyLevel::Supervised,
        _ => {}
    }
}

fn resolved_cron_workspace_dir(config: &Config, job: &CronJob) -> PathBuf {
    let fp = match job.folder_path.as_deref() {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return config.workspace_dir.clone(),
    };
    let path = PathBuf::from(fp);
    if path.is_absolute() {
        path
    } else {
        config.workspace_dir.join(path)
    }
}

fn prepend_cron_run_meta(job: &CronJob, output: &str) -> String {
    let perm = job.permission_mode.as_deref().unwrap_or("-");
    let mode = job.coding_mode.as_deref().unwrap_or("-");
    let folder = job.folder_path.as_deref().unwrap_or("-");
    let uw = match job.use_worktree {
        Some(true) => "true",
        Some(false) => "false",
        None => "-",
    };
    format!("--- run meta: mode={mode} permission={perm} folder={folder} useWorktree={uw} ---\n{output}")
}

pub async fn run(config: Config) -> Result<()> {
    let poll_secs = config.reliability.scheduler_poll_secs.max(MIN_POLL_SECONDS);
    let mut interval = time::interval(Duration::from_secs(poll_secs));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));

    crate::health::mark_component_ok(SCHEDULER_COMPONENT);

    let mut jobs_with_builtin = config.cron.jobs.clone();
    if let Some(ref schedule_cron) = config.backup.schedule_cron {
        let backup_job = CronJobDecl {
            id: "__builtin_backup".to_string(),
            name: Some("Scheduled backup".to_string()),
            job_type: "shell".to_string(),
            schedule: CronScheduleDecl::Cron {
                expr: schedule_cron.clone(),
                tz: config.backup.schedule_timezone.clone(),
            },
            command: Some("backup create".to_string()),
            prompt: None,
            enabled: true,
            model: None,
            allowed_tools: None,
            session_target: None,
            delivery: None,
        };
        tracing::debug!(
            schedule = %schedule_cron,
            "Synthesizing builtin backup cron job from config.backup.schedule_cron"
        );
        jobs_with_builtin.push(backup_job);
    }

    match sync_declarative_jobs(&config, &jobs_with_builtin) {
        Ok(()) => {
            if !jobs_with_builtin.is_empty() {
                tracing::info!(
                    count = jobs_with_builtin.len(),
                    "Synced declarative cron jobs from config"
                );
            }
        }
        Err(e) => tracing::warn!("Failed to sync declarative cron jobs: {e}"),
    }

    if config.cron.catch_up_on_startup {
        catch_up_overdue_jobs(&config, &security).await;
    } else {
        tracing::info!("Scheduler startup: catch-up disabled by config");
    }

    loop {
        interval.tick().await;

        crate::health::mark_component_ok(SCHEDULER_COMPONENT);

        let jobs = match due_jobs(&config, Utc::now()) {
            Ok(jobs) => jobs,
            Err(e) => {
                crate::health::mark_component_error(SCHEDULER_COMPONENT, e.to_string());
                tracing::warn!("Scheduler query failed: {e}");
                continue;
            }
        };

        process_due_jobs(&config, &security, jobs, SCHEDULER_COMPONENT).await;
    }
}

async fn catch_up_overdue_jobs(config: &Config, security: &Arc<SecurityPolicy>) {
    let now = Utc::now();
    let jobs = match all_overdue_jobs(config, now) {
        Ok(jobs) => jobs,
        Err(e) => {
            tracing::warn!("Startup catch-up query failed: {e}");
            return;
        }
    };

    if jobs.is_empty() {
        tracing::info!("Scheduler startup: no overdue jobs to catch up");
        return;
    }

    tracing::info!(
        count = jobs.len(),
        "Scheduler startup: catching up overdue jobs"
    );

    process_due_jobs(config, security, jobs, SCHEDULER_COMPONENT).await;

    tracing::info!("Scheduler startup: catch-up complete");
}

pub async fn execute_job_now(config: &Config, job: &CronJob) -> (bool, String) {
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);
    Box::pin(execute_job_with_retry(config, &security, job)).await
}

pub async fn execute_job_now_and_record(config: &Config, job: &CronJob) -> (bool, String) {
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let started_at = Utc::now();
    let run_id = match crate::cron::start_run(config, &job.id, started_at) {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::warn!("Failed to insert running cron record for {}: {e}", job.id);
            None
        }
    };

    let (success, output) = Box::pin(execute_job_with_retry(config, &security, job)).await;
    let finished_at = Utc::now();
    let success = Box::pin(persist_job_result(
        config,
        job,
        success,
        &output,
        started_at,
        finished_at,
        run_id,
    ))
    .await;

    (success, output)
}

async fn execute_job_with_retry(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (bool, String) {
    let mut last_output = String::new();
    let retries = config.reliability.scheduler_retries;
    let mut backoff_ms = config.reliability.provider_backoff_ms.max(200);

    for attempt in 0..=retries {
        let (success, output) = match job.job_type {
            JobType::Shell => run_job_command(config, security, job).await,
            JobType::Agent => Box::pin(run_agent_job(config, security, job)).await,
        };
        last_output = output;

        if success {
            return (true, last_output);
        }

        if last_output.starts_with("blocked by security policy:") {

            return (false, last_output);
        }

        if attempt < retries {
            let jitter_ms = u64::from(Utc::now().timestamp_subsec_millis() % 250);
            time::sleep(Duration::from_millis(backoff_ms + jitter_ms)).await;
            backoff_ms = (backoff_ms.saturating_mul(2)).min(30_000);
        }
    }

    (false, last_output)
}

async fn process_due_jobs(
    config: &Config,
    security: &Arc<SecurityPolicy>,
    jobs: Vec<CronJob>,
    component: &str,
) {

    crate::health::mark_component_ok(component);

    let max_concurrent = config.scheduler.max_concurrent.max(1);
    let mut in_flight = stream::iter(jobs.into_iter().map(|job| {
        let config = config.clone();
        let security = Arc::clone(security);
        let component = component.to_owned();
        async move {
            Box::pin(execute_and_persist_job(
                &config,
                security.as_ref(),
                &job,
                &component,
            ))
            .await
        }
    }))
    .buffer_unordered(max_concurrent);

    while let Some((job_id, success, output)) = in_flight.next().await {
        if !success {
            tracing::warn!("Scheduler job '{job_id}' failed: {output}");
        }
    }
}

async fn execute_and_persist_job(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
    component: &str,
) -> (String, bool, String) {
    crate::health::mark_component_ok(component);
    warn_if_high_frequency_agent_job(job);

    let started_at = Utc::now();
    let run_id = match crate::cron::start_run(config, &job.id, started_at) {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::warn!("Failed to insert running cron record for {}: {e}", job.id);
            None
        }
    };

    let (success, output) = Box::pin(execute_job_with_retry(config, security, job)).await;
    let finished_at = Utc::now();
    let success = Box::pin(persist_job_result(
        config,
        job,
        success,
        &output,
        started_at,
        finished_at,
        run_id,
    ))
    .await;

    (job.id.clone(), success, output)
}

async fn run_agent_job(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (bool, String) {
    if !security.can_act() {
        return (
            false,
            "blocked by security policy: autonomy is read-only".to_string(),
        );
    }

    if security.is_rate_limited() {
        return (
            false,
            "blocked by security policy: rate limit exceeded".to_string(),
        );
    }

    if !security.record_action() {
        return (
            false,
            "blocked by security policy: action budget exhausted".to_string(),
        );
    }
    let name = job.name.clone().unwrap_or_else(|| "cron-job".to_string());
    let prompt = job.prompt.clone().unwrap_or_default();
    let prefixed_prompt = format!("[cron:{} {name}] {prompt}", job.id);
    let model_override = job.model.clone();

    let mut effective_config = config.clone();
    apply_cron_permission_mode(&mut effective_config.autonomy, job.permission_mode.as_deref());
    effective_config.workspace_dir = resolved_cron_workspace_dir(config, job);

    let coding_override = job
        .coding_mode
        .as_deref()
        .and_then(CodingMode::from_str_loose);

    let run_result = match job.session_target {
        SessionTarget::Main | SessionTarget::Isolated => {
            Box::pin(crate::agent::run(
                effective_config,
                Some(prefixed_prompt),
                None,
                model_override,
                config.default_temperature,
                vec![],
                false,
                None,
                job.allowed_tools.clone(),
                coding_override,
            ))
            .await
        }
    };

    match run_result {
        Ok(response) => (
            true,
            if response.trim().is_empty() {
                "agent job executed".to_string()
            } else {
                response
            },
        ),
        Err(e) => (false, format!("agent job failed: {e}")),
    }
}

async fn persist_job_result(
    config: &Config,
    job: &CronJob,
    mut success: bool,
    output: &str,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    run_id: Option<i64>,
) -> bool {
    let duration_ms = (finished_at - started_at).num_milliseconds();

    if let Err(e) = deliver_if_configured(config, job, output).await {
        if job.delivery.best_effort {
            tracing::warn!("Cron delivery failed (best_effort): {e}");
        } else {
            success = false;
            tracing::warn!("Cron delivery failed: {e}");
        }
    }

    let stored_output = prepend_cron_run_meta(job, output);
    let status = if success { "ok" } else { "error" };

    if let Some(rid) = run_id {
        let _ = crate::cron::finalize_run(
            config,
            rid,
            finished_at,
            status,
            Some(&stored_output),
            duration_ms,
        );
    } else {
        let _ = record_run(
            config,
            &job.id,
            started_at,
            finished_at,
            status,
            Some(&stored_output),
            duration_ms,
        );
    }

    if is_one_shot_auto_delete(job) {
        if success {
            if let Err(e) = remove_job(config, &job.id) {
                tracing::warn!("Failed to remove one-shot cron job after success: {e}");

                let _ = update_job(
                    config,
                    &job.id,
                    CronJobPatch {
                        enabled: Some(false),
                        ..CronJobPatch::default()
                    },
                );
            }
        } else {
            let _ = record_last_run(config, &job.id, finished_at, false, output);
            if let Err(e) = update_job(
                config,
                &job.id,
                CronJobPatch {
                    enabled: Some(false),
                    ..CronJobPatch::default()
                },
            ) {
                tracing::warn!("Failed to disable failed one-shot cron job: {e}");
            }
        }
        return success;
    }

    if let Err(e) = reschedule_after_run(config, job, success, output) {
        tracing::warn!("Failed to persist scheduler run result: {e}");
    }

    success
}

fn is_one_shot_auto_delete(job: &CronJob) -> bool {
    job.delete_after_run && matches!(job.schedule, Schedule::At { .. })
}

fn warn_if_high_frequency_agent_job(job: &CronJob) {
    if !matches!(job.job_type, JobType::Agent) {
        return;
    }
    let too_frequent = match &job.schedule {
        Schedule::Every { every_ms } => *every_ms < 5 * 60 * 1000,
        Schedule::Cron { .. } => {
            let now = Utc::now();
            match (
                next_run_for_schedule(&job.schedule, now),
                next_run_for_schedule(&job.schedule, now + chrono::Duration::seconds(1)),
            ) {
                (Ok(a), Ok(b)) => (b - a).num_minutes() < 5,
                _ => false,
            }
        }
        Schedule::At { .. } => false,
    };

    if too_frequent {
        tracing::warn!(
            "Cron agent job '{}' is scheduled more frequently than every 5 minutes",
            job.id
        );
    }
}

fn resolve_matrix_delivery_room(configured_room_id: &str, target: &str) -> String {
    let target = target.trim();
    if target.is_empty() {
        configured_room_id.trim().to_string()
    } else {
        target.to_string()
    }
}

async fn deliver_if_configured(config: &Config, job: &CronJob, output: &str) -> Result<()> {
    let delivery: &DeliveryConfig = &job.delivery;
    if !delivery.mode.eq_ignore_ascii_case("announce") {
        return Ok(());
    }

    let channel = delivery
        .channel
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("delivery.channel is required for announce mode"))?;
    let target = delivery
        .to
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("delivery.to is required for announce mode"))?;

    deliver_announcement(config, channel, target, output).await
}

pub(crate) struct RedactedOutput(String);

impl RedactedOutput {

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

fn scan_and_redact_output(channel: &str, target: &str, output: &str) -> RedactedOutput {
    let leak_detector = crate::security::LeakDetector::new();
    let leak_check = leak_detector.scan(output);

    match leak_check {
        crate::security::LeakResult::Detected { patterns, redacted } => {
            tracing::warn!(
                channel = %channel,
                target = %target,
                patterns = ?patterns,
                "Credential leak detected in cron job output; redacting before delivery"
            );
            RedactedOutput(redacted)
        }
        crate::security::LeakResult::Clean => RedactedOutput(output.to_string()),
    }
}

pub(crate) async fn deliver_announcement(
    config: &Config,
    channel: &str,
    target: &str,
    output: &str,
) -> Result<()> {

    let safe_output = scan_and_redact_output(channel, target, output);

    match channel.to_ascii_lowercase().as_str() {
        "telegram" => {
            let tg = config
                .channels_config
                .telegram
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("telegram channel not configured"))?;
            let channel = TelegramChannel::new(
                tg.bot_token.clone(),
                tg.allowed_users.clone(),
                tg.mention_only,
            );
            channel
                .send(&SendMessage::new(safe_output.as_str(), target))
                .await?;
        }
        "discord" => {
            let dc = config
                .channels_config
                .discord
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("discord channel not configured"))?;
            let channel = DiscordChannel::new(
                dc.bot_token.clone(),
                dc.guild_id.clone(),
                dc.allowed_users.clone(),
                dc.listen_to_bots,
                dc.mention_only,
            );
            channel
                .send(&SendMessage::new(safe_output.as_str(), target))
                .await?;
        }
        "slack" => {
            let sl = config
                .channels_config
                .slack
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("slack channel not configured"))?;
            let channel = SlackChannel::new(
                sl.bot_token.clone(),
                sl.app_token.clone(),
                sl.channel_id.clone(),
                Vec::new(),
                sl.allowed_users.clone(),
            )
            .with_workspace_dir(config.workspace_dir.clone());
            channel
                .send(&SendMessage::new(safe_output.as_str(), target))
                .await?;
        }
        "mattermost" => {
            let mm = config
                .channels_config
                .mattermost
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("mattermost channel not configured"))?;
            let channel = MattermostChannel::new(
                mm.url.clone(),
                mm.bot_token.clone(),
                mm.channel_id.clone(),
                mm.allowed_users.clone(),
                mm.thread_replies.unwrap_or(true),
                mm.mention_only.unwrap_or(false),
            );
            channel
                .send(&SendMessage::new(safe_output.as_str(), target))
                .await?;
        }
        "signal" => {
            let sg = config
                .channels_config
                .signal
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("signal channel not configured"))?;
            let channel = SignalChannel::new(
                sg.http_url.clone(),
                sg.account.clone(),
                sg.group_id.clone(),
                sg.allowed_from.clone(),
                sg.ignore_attachments,
                sg.ignore_stories,
            );
            channel
                .send(&SendMessage::new(safe_output.as_str(), target))
                .await?;
        }
        "matrix" => {
            #[cfg(feature = "channel-matrix")]
            {
                let mx = config
                    .channels_config
                    .matrix
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("matrix channel not configured"))?;
                let room_id = resolve_matrix_delivery_room(&mx.room_id, target);
                let channel = MatrixChannel::new_with_session_hint_and_sen_dir(
                    mx.homeserver.clone(),
                    mx.access_token.clone(),
                    room_id,
                    mx.allowed_users.clone(),
                    mx.user_id.clone(),
                    mx.device_id.clone(),
                    config.config_path.parent().map(|path| path.to_path_buf()),
                );
                channel
                    .send(&SendMessage::new(safe_output.as_str(), target))
                    .await?;
            }
            #[cfg(not(feature = "channel-matrix"))]
            {
                anyhow::bail!("matrix delivery channel requires `channel-matrix` feature");
            }
        }
        "whatsapp" | "whatsapp-web" | "whatsapp_web" => {
            #[cfg(feature = "whatsapp-web")]
            {
                let wa = config
                    .channels_config
                    .whatsapp
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("whatsapp channel not configured"))?;
                if !wa.is_web_config() {
                    anyhow::bail!(
                        "whatsapp cron delivery requires Web mode (session_path must be set)"
                    );
                }
                let channel = WhatsAppWebChannel::new(
                    wa.session_path.clone().unwrap_or_default(),
                    wa.pair_phone.clone(),
                    wa.pair_code.clone(),
                    wa.allowed_numbers.clone(),
                    wa.mode.clone(),
                    wa.dm_policy.clone(),
                    wa.group_policy.clone(),
                    wa.self_chat_mode,
                );
                channel
                    .send(&SendMessage::new(safe_output.as_str(), target))
                    .await?;
            }
            #[cfg(not(feature = "whatsapp-web"))]
            {
                anyhow::bail!("whatsapp delivery channel requires `whatsapp-web` feature");
            }
        }
        "qq" => {
            let qq = config
                .channels_config
                .qq
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("qq channel not configured"))?;
            let channel = QQChannel::new(
                qq.app_id.clone(),
                qq.app_secret.clone(),
                qq.allowed_users.clone(),
            );
            channel
                .send(&SendMessage::new(safe_output.as_str(), target))
                .await?;
        }
        other => anyhow::bail!("unsupported delivery channel: {other}"),
    }

    Ok(())
}

async fn run_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (bool, String) {
    run_job_command_with_timeout(
        config,
        security,
        job,
        Duration::from_secs(SHELL_JOB_TIMEOUT_SECS),
    )
    .await
}

async fn run_job_command_with_timeout(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
    timeout: Duration,
) -> (bool, String) {
    if !security.can_act() {
        return (
            false,
            "blocked by security policy: autonomy is read-only".to_string(),
        );
    }

    if security.is_rate_limited() {
        return (
            false,
            "blocked by security policy: rate limit exceeded".to_string(),
        );
    }

    let approved = false;
    if let Err(error) =
        crate::cron::validate_shell_command_with_security(security, &job.command, approved)
    {
        return (false, error.to_string());
    }

    if let Some(path) = security.forbidden_path_argument(&job.command) {
        return (
            false,
            format!("blocked by security policy: forbidden path argument: {path}"),
        );
    }

    if !security.record_action() {
        return (
            false,
            "blocked by security policy: action budget exhausted".to_string(),
        );
    }

    let child = match build_cron_shell_command(&job.command, &config.workspace_dir) {
        Ok(mut cmd) => match cmd.spawn() {
            Ok(child) => child,
            Err(e) => return (false, format!("spawn error: {e}")),
        },
        Err(e) => return (false, format!("shell setup error: {e}")),
    };

    match time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!(
                "status={}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                stdout.trim(),
                stderr.trim()
            );
            (output.status.success(), combined)
        }
        Ok(Err(e)) => (false, format!("spawn error: {e}")),
        Err(_) => (
            false,
            format!("job timed out after {}s", timeout.as_secs_f64()),
        ),
    }
}

fn build_cron_shell_command(
    command: &str,
    workspace_dir: &std::path::Path,
) -> anyhow::Result<Command> {
    let mut cmd = crate::util::hidden_async_command("sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(workspace_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    Ok(cmd)
}
