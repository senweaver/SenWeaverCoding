// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::config::Config;
use crate::cron::{
    AgentJobOptions, CronJob, CronJobPatch, CronRun, DeliveryConfig, JobType, Schedule, SessionTarget,
    next_run_for_schedule, schedule_cron_expression, validate_delivery_config, validate_schedule,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::types::{FromSqlResult, ValueRef};
use rusqlite::{Connection, params};
use uuid::Uuid;

const MAX_CRON_OUTPUT_BYTES: usize = 16 * 1024;
const TRUNCATED_OUTPUT_MARKER: &str = "\n...[truncated]";

impl rusqlite::types::FromSql for JobType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = value.as_str()?;
        JobType::try_from(text).map_err(|e| rusqlite::types::FromSqlError::Other(e.into()))
    }
}

pub fn add_job(config: &Config, expression: &str, command: &str) -> Result<CronJob> {
    let schedule = Schedule::Cron {
        expr: expression.to_string(),
        tz: None,
    };
    add_shell_job(config, None, schedule, command, None)
}

pub fn add_shell_job(
    config: &Config,
    name: Option<String>,
    schedule: Schedule,
    command: &str,
    delivery: Option<DeliveryConfig>,
) -> Result<CronJob> {
    let now = Utc::now();
    validate_schedule(&schedule, now)?;
    validate_delivery_config(delivery.as_ref())?;
    let next_run = next_run_for_schedule(&schedule, now)?;
    let id = Uuid::new_v4().to_string();
    let expression = schedule_cron_expression(&schedule).unwrap_or_default();
    let schedule_json = serde_json::to_string(&schedule)?;
    let delivery = delivery.unwrap_or_default();

    let delete_after_run = matches!(schedule, Schedule::At { .. });

    with_connection(config, |conn| {
        conn.execute(
            "INSERT INTO cron_jobs (
                id, expression, command, schedule, job_type, prompt, name, session_target, model,
                enabled, delivery, delete_after_run, created_at, next_run
             ) VALUES (?1, ?2, ?3, ?4, 'shell', NULL, ?5, 'isolated', NULL, 1, ?6, ?7, ?8, ?9)",
            params![
                id,
                expression,
                command,
                schedule_json,
                name,
                serde_json::to_string(&delivery)?,
                i32::from(delete_after_run),
                now.to_rfc3339(),
                next_run.to_rfc3339(),
            ],
        )
        .context("Failed to insert cron shell job")?;
        Ok(())
    })?;

    get_job(config, &id)
}

pub fn add_agent_job(
    config: &Config,
    name: Option<String>,
    schedule: Schedule,
    prompt: &str,
    opts: AgentJobOptions,
) -> Result<CronJob> {
    let AgentJobOptions {
        session_target,
        model,
        delivery,
        delete_after_run,
        allowed_tools,
        permission_mode,
        coding_mode,
        folder_path,
        use_worktree,
        notification,
        task_description,
    } = opts;

    let now = Utc::now();
    validate_schedule(&schedule, now)?;
    validate_delivery_config(delivery.as_ref())?;
    let next_run = next_run_for_schedule(&schedule, now)?;
    let id = Uuid::new_v4().to_string();
    let expression = schedule_cron_expression(&schedule).unwrap_or_default();
    let schedule_json = serde_json::to_string(&schedule)?;
    let delivery = delivery.unwrap_or_default();
    let notification_json =
        encode_optional_json_value(notification.as_ref()).context("notification JSON")?;

    with_connection(config, |conn| {
        conn.execute(
            "INSERT INTO cron_jobs (
                id, expression, command, schedule, job_type, prompt, name, session_target, model,
                enabled, delivery, delete_after_run, allowed_tools,
                permission_mode, coding_mode, folder_path, use_worktree, notification, task_description,
                created_at, next_run
             ) VALUES (?1, ?2, '', ?3, 'agent', ?4, ?5, ?6, ?7, 1, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                id,
                expression,
                schedule_json,
                prompt,
                name,
                session_target.as_str(),
                model,
                serde_json::to_string(&delivery)?,
                i32::from(delete_after_run),
                encode_allowed_tools(allowed_tools.as_ref())?,
                permission_mode,
                coding_mode,
                folder_path,
                use_worktree.map(|b| i32::from(b)),
                notification_json,
                task_description,
                now.to_rfc3339(),
                next_run.to_rfc3339(),
            ],
        )
        .context("Failed to insert cron agent job")?;
        Ok(())
    })?;

    get_job(config, &id)
}

pub fn list_jobs(config: &Config) -> Result<Vec<CronJob>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, expression, command, schedule, job_type, prompt, name, session_target, model,
                    enabled, delivery, delete_after_run, created_at, next_run, last_run, last_status, last_output,
                    allowed_tools, source, permission_mode, coding_mode, folder_path, use_worktree, notification,
                    task_description
             FROM cron_jobs ORDER BY next_run ASC",
        )?;

        let rows = stmt.query_map([], map_cron_job_row)?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    })
}

pub fn get_job(config: &Config, job_id: &str) -> Result<CronJob> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, expression, command, schedule, job_type, prompt, name, session_target, model,
                    enabled, delivery, delete_after_run, created_at, next_run, last_run, last_status, last_output,
                    allowed_tools, source, permission_mode, coding_mode, folder_path, use_worktree, notification,
                    task_description
             FROM cron_jobs WHERE id = ?1",
        )?;

        let mut rows = stmt.query(params![job_id])?;
        if let Some(row) = rows.next()? {
            map_cron_job_row(row).map_err(Into::into)
        } else {
            anyhow::bail!("Cron job '{job_id}' not found")
        }
    })
}

pub fn remove_job(config: &Config, id: &str) -> Result<()> {
    let changed = with_connection(config, |conn| {
        conn.execute("DELETE FROM cron_jobs WHERE id = ?1", params![id])
            .context("Failed to delete cron job")
    })?;

    if changed == 0 {
        anyhow::bail!("Cron job '{id}' not found");
    }

    println!("✅ Removed cron job {id}");
    Ok(())
}

pub fn due_jobs(config: &Config, now: DateTime<Utc>) -> Result<Vec<CronJob>> {
    let lim = i64::try_from(config.scheduler.max_tasks.max(1))
        .context("Scheduler max_tasks overflows i64")?;
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, expression, command, schedule, job_type, prompt, name, session_target, model,
                    enabled, delivery, delete_after_run, created_at, next_run, last_run, last_status, last_output,
                    allowed_tools, source, permission_mode, coding_mode, folder_path, use_worktree, notification,
                    task_description
             FROM cron_jobs
             WHERE enabled = 1 AND next_run <= ?1
             ORDER BY next_run ASC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![now.to_rfc3339(), lim], map_cron_job_row)?;

        let mut jobs = Vec::new();
        for row in rows {
            match row {
                Ok(job) => jobs.push(job),
                Err(e) => tracing::warn!("Skipping cron job with unparseable row data: {e}"),
            }
        }
        Ok(jobs)
    })
}

pub fn all_overdue_jobs(config: &Config, now: DateTime<Utc>) -> Result<Vec<CronJob>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, expression, command, schedule, job_type, prompt, name, session_target, model,
                    enabled, delivery, delete_after_run, created_at, next_run, last_run, last_status, last_output,
                    allowed_tools, source, permission_mode, coding_mode, folder_path, use_worktree, notification,
                    task_description
             FROM cron_jobs
             WHERE enabled = 1 AND next_run <= ?1
             ORDER BY next_run ASC",
        )?;

        let rows = stmt.query_map(params![now.to_rfc3339()], map_cron_job_row)?;

        let mut jobs = Vec::new();
        for row in rows {
            match row {
                Ok(job) => jobs.push(job),
                Err(e) => tracing::warn!("Skipping cron job with unparseable row data: {e}"),
            }
        }
        Ok(jobs)
    })
}

pub fn update_job(config: &Config, job_id: &str, patch: CronJobPatch) -> Result<CronJob> {
    let mut job = get_job(config, job_id)?;
    let mut schedule_changed = false;

    if let Some(schedule) = patch.schedule {
        validate_schedule(&schedule, Utc::now())?;
        job.schedule = schedule;
        job.expression = schedule_cron_expression(&job.schedule).unwrap_or_default();
        schedule_changed = true;
    }
    if let Some(command) = patch.command {
        job.command = command;
    }
    if let Some(prompt) = patch.prompt {
        job.prompt = Some(prompt);
    }
    if let Some(name) = patch.name {
        job.name = Some(name);
    }
    if let Some(enabled) = patch.enabled {
        job.enabled = enabled;
    }
    if let Some(delivery) = patch.delivery {
        job.delivery = delivery;
    }
    if let Some(model) = patch.model {
        job.model = Some(model);
    }
    if let Some(target) = patch.session_target {
        job.session_target = target;
    }
    if let Some(delete_after_run) = patch.delete_after_run {
        job.delete_after_run = delete_after_run;
    }
    if let Some(allowed_tools) = patch.allowed_tools {

        if allowed_tools.is_empty() {
            job.allowed_tools = None;
        } else {
            job.allowed_tools = Some(allowed_tools);
        }
    }

    if let Some(pm) = patch.permission_mode.clone() {
        job.permission_mode = (!pm.trim().is_empty()).then_some(pm);
    }
    if let Some(cm) = patch.coding_mode.clone() {
        job.coding_mode = (!cm.trim().is_empty()).then_some(cm);
    }
    if let Some(fp) = patch.folder_path.clone() {
        job.folder_path = (!fp.trim().is_empty()).then_some(fp);
    }
    if let Some(wt) = patch.use_worktree {
        job.use_worktree = Some(wt);
    }
    if let Some(n) = patch.notification.clone() {
        job.notification = if n.is_null() { None } else { Some(n) };
    }
    if let Some(td) = patch.task_description.clone() {
        job.task_description = (!td.trim().is_empty()).then_some(td);
    }

    if schedule_changed {
        job.next_run = next_run_for_schedule(&job.schedule, Utc::now())?;
    }

    let notification_db =
        encode_optional_json_value(job.notification.as_ref()).context("notification JSON")?;

    with_connection(config, |conn| {
        conn.execute(
            "UPDATE cron_jobs
             SET expression = ?1, command = ?2, schedule = ?3, job_type = ?4, prompt = ?5, name = ?6,
                 session_target = ?7, model = ?8, enabled = ?9, delivery = ?10, delete_after_run = ?11,
                 allowed_tools = ?12,
                 permission_mode = ?13, coding_mode = ?14, folder_path = ?15, use_worktree = ?16,
                 notification = ?17, task_description = ?18,
                 next_run = ?19
             WHERE id = ?20",
            params![
                job.expression,
                job.command,
                serde_json::to_string(&job.schedule)?,
                <JobType as Into<&str>>::into(job.job_type).to_string(),
                job.prompt,
                job.name,
                job.session_target.as_str(),
                job.model,
                i32::from(job.enabled),
                serde_json::to_string(&job.delivery)?,
                i32::from(job.delete_after_run),
                encode_allowed_tools(job.allowed_tools.as_ref())?,
                job.permission_mode,
                job.coding_mode,
                job.folder_path,
                job.use_worktree.map(|b| i32::from(b)),
                notification_db,
                job.task_description,
                job.next_run.to_rfc3339(),
                job.id,
            ],
        )
        .context("Failed to update cron job")?;
        Ok(())
    })?;

    get_job(config, job_id)
}

pub fn record_last_run(
    config: &Config,
    job_id: &str,
    finished_at: DateTime<Utc>,
    success: bool,
    output: &str,
) -> Result<()> {
    let status = if success { "ok" } else { "error" };
    let bounded_output = truncate_cron_output(output);
    with_connection(config, |conn| {
        conn.execute(
            "UPDATE cron_jobs
             SET last_run = ?1, last_status = ?2, last_output = ?3
             WHERE id = ?4",
            params![finished_at.to_rfc3339(), status, bounded_output, job_id],
        )
        .context("Failed to update cron last run fields")?;
        Ok(())
    })
}

pub fn reschedule_after_run(
    config: &Config,
    job: &CronJob,
    success: bool,
    output: &str,
) -> Result<()> {
    let now = Utc::now();
    let status = if success { "ok" } else { "error" };
    let bounded_output = truncate_cron_output(output);

    if matches!(job.schedule, Schedule::At { .. }) {
        with_connection(config, |conn| {
            conn.execute(
                "UPDATE cron_jobs
                 SET enabled = 0, last_run = ?1, last_status = ?2, last_output = ?3
                 WHERE id = ?4",
                params![now.to_rfc3339(), status, bounded_output, job.id],
            )
            .context("Failed to disable completed one-shot cron job")?;
            Ok(())
        })
    } else {
        let next_run = next_run_for_schedule(&job.schedule, now)?;
        with_connection(config, |conn| {
            conn.execute(
                "UPDATE cron_jobs
                 SET next_run = ?1, last_run = ?2, last_status = ?3, last_output = ?4
                 WHERE id = ?5",
                params![
                    next_run.to_rfc3339(),
                    now.to_rfc3339(),
                    status,
                    bounded_output,
                    job.id
                ],
            )
            .context("Failed to update cron job run state")?;
            Ok(())
        })
    }
}

pub fn start_run(config: &Config, job_id: &str, started_at: DateTime<Utc>) -> Result<i64> {
    with_connection(config, |conn| {
        conn.execute(
            "INSERT INTO cron_runs (job_id, started_at, finished_at, status, output, duration_ms)
             VALUES (?1, ?2, ?2, 'running', NULL, NULL)",
            params![job_id, started_at.to_rfc3339()],
        )
        .context("Failed to insert cron running record")?;
        Ok(conn.last_insert_rowid())
    })
}

pub fn finalize_run(
    config: &Config,
    run_id: i64,
    finished_at: DateTime<Utc>,
    status: &str,
    output: Option<&str>,
    duration_ms: i64,
) -> Result<()> {
    let bounded_output = output.map(truncate_cron_output);
    with_connection(config, |conn| {
        let tx = conn.unchecked_transaction()?;

        tx.execute(
            "UPDATE cron_runs
             SET finished_at = ?1, status = ?2, output = ?3, duration_ms = ?4
             WHERE id = ?5",
            params![
                finished_at.to_rfc3339(),
                status,
                bounded_output.as_deref(),
                duration_ms,
                run_id,
            ],
        )
        .context("Failed to finalize cron run")?;

        let keep = i64::from(config.cron.max_run_history.max(1));
        tx.execute(
            "DELETE FROM cron_runs
             WHERE job_id = (SELECT job_id FROM cron_runs WHERE id = ?1)
               AND id NOT IN (
                 SELECT id FROM cron_runs
                 WHERE job_id = (SELECT job_id FROM cron_runs WHERE id = ?1)
                 ORDER BY started_at DESC, id DESC
                 LIMIT ?2
               )",
            params![run_id, keep],
        )
        .context("Failed to prune cron run history")?;

        tx.commit()
            .context("Failed to commit cron run finalize transaction")?;
        Ok(())
    })
}

pub fn record_run(
    config: &Config,
    job_id: &str,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    status: &str,
    output: Option<&str>,
    duration_ms: i64,
) -> Result<()> {
    let bounded_output = output.map(truncate_cron_output);
    with_connection(config, |conn| {

        let tx = conn.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO cron_runs (job_id, started_at, finished_at, status, output, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                job_id,
                started_at.to_rfc3339(),
                finished_at.to_rfc3339(),
                status,
                bounded_output.as_deref(),
                duration_ms,
            ],
        )
        .context("Failed to insert cron run")?;

        let keep = i64::from(config.cron.max_run_history.max(1));
        tx.execute(
            "DELETE FROM cron_runs
             WHERE job_id = ?1
               AND id NOT IN (
                 SELECT id FROM cron_runs
                 WHERE job_id = ?1
                 ORDER BY started_at DESC, id DESC
                 LIMIT ?2
               )",
            params![job_id, keep],
        )
        .context("Failed to prune cron run history")?;

        tx.commit()
            .context("Failed to commit cron run transaction")?;
        Ok(())
    })
}

fn truncate_cron_output(output: &str) -> String {
    if output.len() <= MAX_CRON_OUTPUT_BYTES {
        return output.to_string();
    }

    if MAX_CRON_OUTPUT_BYTES <= TRUNCATED_OUTPUT_MARKER.len() {
        return TRUNCATED_OUTPUT_MARKER.to_string();
    }

    let mut cutoff = MAX_CRON_OUTPUT_BYTES - TRUNCATED_OUTPUT_MARKER.len();
    while cutoff > 0 && !output.is_char_boundary(cutoff) {
        cutoff -= 1;
    }

    let mut truncated = output[..cutoff].to_string();
    truncated.push_str(TRUNCATED_OUTPUT_MARKER);
    truncated
}

pub fn list_runs(config: &Config, job_id: &str, limit: usize) -> Result<Vec<CronRun>> {
    with_connection(config, |conn| {
        let lim = i64::try_from(limit.max(1)).context("Run history limit overflow")?;
        let mut stmt = conn.prepare(
            "SELECT id, job_id, started_at, finished_at, status, output, duration_ms
             FROM cron_runs
             WHERE job_id = ?1
             ORDER BY started_at DESC, id DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![job_id, lim], |row| {
            Ok(CronRun {
                id: row.get(0)?,
                job_id: row.get(1)?,
                started_at: parse_rfc3339(&row.get::<_, String>(2)?)
                    .map_err(sql_conversion_error)?,
                finished_at: parse_rfc3339(&row.get::<_, String>(3)?)
                    .map_err(sql_conversion_error)?,
                status: row.get(4)?,
                output: row.get(5)?,
                duration_ms: row.get(6)?,
            })
        })?;

        let mut runs = Vec::new();
        for row in rows {
            runs.push(row?);
        }
        Ok(runs)
    })
}

fn parse_rfc3339(raw: &str) -> Result<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(raw)
        .with_context(|| format!("Invalid RFC3339 timestamp in cron DB: {raw}"))?;
    Ok(parsed.with_timezone(&Utc))
}

fn sql_conversion_error(err: anyhow::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(err.into())
}

fn map_cron_job_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CronJob> {
    let expression: String = row.get(1)?;
    let schedule_raw: Option<String> = row.get(3)?;
    let schedule =
        decode_schedule(schedule_raw.as_deref(), &expression).map_err(sql_conversion_error)?;

    let delivery_raw: Option<String> = row.get(10)?;
    let delivery = decode_delivery(delivery_raw.as_deref()).map_err(sql_conversion_error)?;

    let next_run_raw: String = row.get(13)?;
    let last_run_raw: Option<String> = row.get(14)?;
    let created_at_raw: String = row.get(12)?;
    let allowed_tools_raw: Option<String> = row.get(17)?;
    let source: Option<String> = row.get(18)?;
    let permission_mode: Option<String> = row.get(19)?;
    let coding_mode: Option<String> = row.get(20)?;
    let folder_path: Option<String> = row.get(21)?;
    let use_worktree_raw: Option<i64> = row.get(22)?;
    let use_worktree = use_worktree_raw.map(|x| x != 0);
    let notification_raw: Option<String> = row.get(23)?;
    let notification = decode_optional_json(notification_raw.as_deref()).map_err(sql_conversion_error)?;
    let task_description: Option<String> = row.get(24)?;

    Ok(CronJob {
        id: row.get(0)?,
        expression,
        schedule,
        command: row.get(2)?,
        job_type: row.get(4)?,
        prompt: row.get(5)?,
        name: row.get(6)?,
        session_target: SessionTarget::parse(&row.get::<_, String>(7)?),
        model: row.get(8)?,
        enabled: row.get::<_, i64>(9)? != 0,
        delivery,
        delete_after_run: row.get::<_, i64>(11)? != 0,
        source: source.unwrap_or_else(|| "imperative".to_string()),
        created_at: parse_rfc3339(&created_at_raw).map_err(sql_conversion_error)?,
        next_run: parse_rfc3339(&next_run_raw).map_err(sql_conversion_error)?,
        last_run: match last_run_raw {
            Some(raw) => Some(parse_rfc3339(&raw).map_err(sql_conversion_error)?),
            None => None,
        },
        last_status: row.get(15)?,
        last_output: row.get(16)?,
        allowed_tools: decode_allowed_tools(allowed_tools_raw.as_deref())
            .map_err(sql_conversion_error)?,
        permission_mode,
        coding_mode,
        folder_path,
        use_worktree,
        notification,
        task_description,
    })
}

fn encode_optional_json_value(opt: Option<&serde_json::Value>) -> Result<Option<String>> {
    Ok(match opt {
        None => None,
        Some(v) => Some(serde_json::to_string(v).context("Failed to serialize JSON column")?),
    })
}

fn decode_optional_json(raw: Option<&str>) -> Result<Option<serde_json::Value>> {
    let Some(trimmed) = raw.map(str::trim) else {
        return Ok(None);
    };
    if trimmed.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(trimmed)
        .map(Some)
        .with_context(|| format!("Failed to parse JSON column value: {trimmed}"))
}

fn decode_schedule(schedule_raw: Option<&str>, expression: &str) -> Result<Schedule> {
    if let Some(raw) = schedule_raw {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return serde_json::from_str(trimmed)
                .with_context(|| format!("Failed to parse cron schedule JSON: {trimmed}"));
        }
    }

    if expression.trim().is_empty() {
        anyhow::bail!("Missing schedule and legacy expression for cron job")
    }

    Ok(Schedule::Cron {
        expr: expression.to_string(),
        tz: None,
    })
}

fn decode_delivery(delivery_raw: Option<&str>) -> Result<DeliveryConfig> {
    if let Some(raw) = delivery_raw {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return serde_json::from_str(trimmed)
                .with_context(|| format!("Failed to parse cron delivery JSON: {trimmed}"));
        }
    }
    Ok(DeliveryConfig::default())
}

fn encode_allowed_tools(allowed_tools: Option<&Vec<String>>) -> Result<Option<String>> {
    allowed_tools
        .map(serde_json::to_string)
        .transpose()
        .context("Failed to serialize cron allowed_tools")
}

fn decode_allowed_tools(raw: Option<&str>) -> Result<Option<Vec<String>>> {
    if let Some(raw) = raw {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return serde_json::from_str(trimmed)
                .map(Some)
                .with_context(|| format!("Failed to parse cron allowed_tools JSON: {trimmed}"));
        }
    }
    Ok(None)
}

pub fn sync_declarative_jobs(
    config: &Config,
    decls: &[crate::config::schema::CronJobDecl],
) -> Result<()> {
    use crate::config::schema::CronScheduleDecl;

    if decls.is_empty() {

        with_connection(config, |conn| {
            let deleted = conn
                .execute("DELETE FROM cron_jobs WHERE source = 'declarative'", [])
                .context("Failed to remove stale declarative cron jobs")?;
            if deleted > 0 {
                tracing::info!(
                    count = deleted,
                    "Removed declarative cron jobs no longer in config"
                );
            }
            Ok(())
        })?;
        return Ok(());
    }

    for decl in decls {
        validate_decl(decl)?;
    }

    let now = Utc::now();

    with_connection(config, |conn| {

        let config_ids: std::collections::HashSet<&str> =
            decls.iter().map(|d| d.id.as_str()).collect();

        {
            let mut stmt = conn.prepare("SELECT id FROM cron_jobs WHERE source = 'declarative'")?;
            let db_ids: Vec<String> = stmt
                .query_map([], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();

            for db_id in &db_ids {
                if !config_ids.contains(db_id.as_str()) {
                    conn.execute("DELETE FROM cron_jobs WHERE id = ?1", params![db_id])
                        .with_context(|| {
                            format!("Failed to remove stale declarative cron job '{db_id}'")
                        })?;
                    tracing::info!(
                        job_id = %db_id,
                        "Removed declarative cron job no longer in config"
                    );
                }
            }
        }

        for decl in decls {
            let schedule = convert_schedule_decl(&decl.schedule)?;
            let expression = schedule_cron_expression(&schedule).unwrap_or_default();
            let schedule_json = serde_json::to_string(&schedule)?;
            let job_type = &decl.job_type;
            let session_target = decl.session_target.as_deref().unwrap_or("isolated");
            let delivery = match &decl.delivery {
                Some(d) => convert_delivery_decl(d),
                None => DeliveryConfig::default(),
            };
            let delivery_json = serde_json::to_string(&delivery)?;
            let allowed_tools_json = encode_allowed_tools(decl.allowed_tools.as_ref())?;
            let command = decl.command.as_deref().unwrap_or("");
            let delete_after_run = matches!(decl.schedule, CronScheduleDecl::At { .. });

            let exists: bool = conn
                .prepare("SELECT COUNT(*) FROM cron_jobs WHERE id = ?1")?
                .query_row(params![decl.id], |row| row.get::<_, i64>(0))
                .map(|c| c > 0)
                .unwrap_or(false);

            if exists {

                let current_schedule_raw: Option<String> = conn
                    .prepare("SELECT schedule FROM cron_jobs WHERE id = ?1")?
                    .query_row(params![decl.id], |row| row.get(0))
                    .ok();

                let schedule_changed = current_schedule_raw.as_deref() != Some(&schedule_json);

                if schedule_changed {
                    let next_run = next_run_for_schedule(&schedule, now)?;
                    conn.execute(
                        "UPDATE cron_jobs
                         SET expression = ?1, command = ?2, schedule = ?3, job_type = ?4,
                             prompt = ?5, name = ?6, session_target = ?7, model = ?8,
                             enabled = ?9, delivery = ?10, delete_after_run = ?11,
                             allowed_tools = ?12, source = 'declarative', next_run = ?13
                         WHERE id = ?14",
                        params![
                            expression,
                            command,
                            schedule_json,
                            job_type,
                            decl.prompt,
                            decl.name,
                            session_target,
                            decl.model,
                            i32::from(decl.enabled),
                            delivery_json,
                            i32::from(delete_after_run),
                            allowed_tools_json,
                            next_run.to_rfc3339(),
                            decl.id,
                        ],
                    )
                    .with_context(|| {
                        format!("Failed to update declarative cron job '{}'", decl.id)
                    })?;
                } else {
                    conn.execute(
                        "UPDATE cron_jobs
                         SET expression = ?1, command = ?2, schedule = ?3, job_type = ?4,
                             prompt = ?5, name = ?6, session_target = ?7, model = ?8,
                             enabled = ?9, delivery = ?10, delete_after_run = ?11,
                             allowed_tools = ?12, source = 'declarative'
                         WHERE id = ?13",
                        params![
                            expression,
                            command,
                            schedule_json,
                            job_type,
                            decl.prompt,
                            decl.name,
                            session_target,
                            decl.model,
                            i32::from(decl.enabled),
                            delivery_json,
                            i32::from(delete_after_run),
                            allowed_tools_json,
                            decl.id,
                        ],
                    )
                    .with_context(|| {
                        format!("Failed to update declarative cron job '{}'", decl.id)
                    })?;
                }

                tracing::debug!(job_id = %decl.id, "Updated declarative cron job");
            } else {

                let next_run = next_run_for_schedule(&schedule, now)?;
                conn.execute(
                    "INSERT INTO cron_jobs (
                        id, expression, command, schedule, job_type, prompt, name,
                        session_target, model, enabled, delivery, delete_after_run,
                        allowed_tools, source, created_at, next_run
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'declarative', ?14, ?15)",
                    params![
                        decl.id,
                        expression,
                        command,
                        schedule_json,
                        job_type,
                        decl.prompt,
                        decl.name,
                        session_target,
                        decl.model,
                        i32::from(decl.enabled),
                        delivery_json,
                        i32::from(delete_after_run),
                        allowed_tools_json,
                        now.to_rfc3339(),
                        next_run.to_rfc3339(),
                    ],
                )
                .with_context(|| {
                    format!(
                        "Failed to insert declarative cron job '{}'",
                        decl.id
                    )
                })?;

                tracing::info!(job_id = %decl.id, "Inserted declarative cron job from config");
            }
        }

        Ok(())
    })
}

fn validate_decl(decl: &crate::config::schema::CronJobDecl) -> Result<()> {
    if decl.id.trim().is_empty() {
        anyhow::bail!("Declarative cron job has empty id");
    }

    match decl.job_type.to_lowercase().as_str() {
        "shell" => {
            if decl
                .command
                .as_deref()
                .map_or(true, |c| c.trim().is_empty())
            {
                anyhow::bail!(
                    "Declarative cron job '{}': shell job requires a non-empty 'command'",
                    decl.id
                );
            }
        }
        "agent" => {
            if decl.prompt.as_deref().map_or(true, |p| p.trim().is_empty()) {
                anyhow::bail!(
                    "Declarative cron job '{}': agent job requires a non-empty 'prompt'",
                    decl.id
                );
            }
        }
        other => {
            anyhow::bail!(
                "Declarative cron job '{}': invalid job_type '{}', expected 'shell' or 'agent'",
                decl.id,
                other
            );
        }
    }

    Ok(())
}

fn convert_schedule_decl(decl: &crate::config::schema::CronScheduleDecl) -> Result<Schedule> {
    use crate::config::schema::CronScheduleDecl;
    match decl {
        CronScheduleDecl::Cron { expr, tz } => Ok(Schedule::Cron {
            expr: expr.clone(),
            tz: tz.clone(),
        }),
        CronScheduleDecl::Every { every_ms } => Ok(Schedule::Every {
            every_ms: *every_ms,
        }),
        CronScheduleDecl::At { at } => {
            let parsed = DateTime::parse_from_rfc3339(at)
                .with_context(|| {
                    format!("Invalid RFC3339 timestamp in declarative cron 'at': {at}")
                })?
                .with_timezone(&Utc);
            Ok(Schedule::At { at: parsed })
        }
    }
}

fn convert_delivery_decl(decl: &crate::config::schema::DeliveryConfigDecl) -> DeliveryConfig {
    DeliveryConfig {
        mode: decl.mode.clone(),
        channel: decl.channel.clone(),
        to: decl.to.clone(),
        best_effort: decl.best_effort,
    }
}

fn add_column_if_missing(conn: &Connection, name: &str, sql_type: &str) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(cron_jobs)")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let col_name: String = row.get(1)?;
        if col_name == name {
            return Ok(());
        }
    }

    drop(rows);
    drop(stmt);

    match conn.execute(
        &format!("ALTER TABLE cron_jobs ADD COLUMN {name} {sql_type}"),
        [],
    ) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(err, Some(ref msg)))
            if msg.contains("duplicate column name") =>
        {
            tracing::debug!("Column cron_jobs.{name} already exists (concurrent migration): {err}");
            Ok(())
        }
        Err(e) => Err(e).with_context(|| format!("Failed to add cron_jobs.{name}")),
    }
}

fn with_connection<T>(config: &Config, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let db_path = config.workspace_dir.join("cron").join("jobs.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create cron directory: {}", parent.display()))?;
    }

    let conn = Connection::open(&db_path)
        .with_context(|| format!("Failed to open cron DB: {}", db_path.display()))?;

    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS cron_jobs (
            id               TEXT PRIMARY KEY,
            expression       TEXT NOT NULL,
            command          TEXT NOT NULL,
            schedule         TEXT,
            job_type         TEXT NOT NULL DEFAULT 'shell',
            prompt           TEXT,
            name             TEXT,
            session_target   TEXT NOT NULL DEFAULT 'isolated',
            model            TEXT,
            enabled          INTEGER NOT NULL DEFAULT 1,
            delivery         TEXT,
            delete_after_run INTEGER NOT NULL DEFAULT 0,
            allowed_tools    TEXT,
            created_at       TEXT NOT NULL,
            next_run         TEXT NOT NULL,
            last_run         TEXT,
            last_status      TEXT,
            last_output      TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_cron_jobs_next_run ON cron_jobs(next_run);

        CREATE TABLE IF NOT EXISTS cron_runs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            job_id      TEXT NOT NULL,
            started_at  TEXT NOT NULL,
            finished_at TEXT NOT NULL,
            status      TEXT NOT NULL,
            output      TEXT,
            duration_ms INTEGER,
            FOREIGN KEY (job_id) REFERENCES cron_jobs(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_cron_runs_job_id ON cron_runs(job_id);
        CREATE INDEX IF NOT EXISTS idx_cron_runs_started_at ON cron_runs(started_at);
        CREATE INDEX IF NOT EXISTS idx_cron_runs_job_started ON cron_runs(job_id, started_at);",
    )
    .context("Failed to initialize cron schema")?;

    add_column_if_missing(&conn, "schedule", "TEXT")?;
    add_column_if_missing(&conn, "job_type", "TEXT NOT NULL DEFAULT 'shell'")?;
    add_column_if_missing(&conn, "prompt", "TEXT")?;
    add_column_if_missing(&conn, "name", "TEXT")?;
    add_column_if_missing(&conn, "session_target", "TEXT NOT NULL DEFAULT 'isolated'")?;
    add_column_if_missing(&conn, "model", "TEXT")?;
    add_column_if_missing(&conn, "enabled", "INTEGER NOT NULL DEFAULT 1")?;
    add_column_if_missing(&conn, "delivery", "TEXT")?;
    add_column_if_missing(&conn, "delete_after_run", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(&conn, "allowed_tools", "TEXT")?;
    add_column_if_missing(&conn, "source", "TEXT DEFAULT 'imperative'")?;
    add_column_if_missing(&conn, "permission_mode", "TEXT")?;
    add_column_if_missing(&conn, "coding_mode", "TEXT")?;
    add_column_if_missing(&conn, "folder_path", "TEXT")?;
    add_column_if_missing(&conn, "use_worktree", "INTEGER")?;
    add_column_if_missing(&conn, "notification", "TEXT")?;
    add_column_if_missing(&conn, "task_description", "TEXT")?;

    f(&conn)
}
