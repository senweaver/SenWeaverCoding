// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::cron::Schedule;
use anyhow::{Context, Result};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use cron::Schedule as CronExprSchedule;
use std::str::FromStr;

pub fn next_run_for_schedule(schedule: &Schedule, from: DateTime<Utc>) -> Result<DateTime<Utc>> {
    match schedule {
        Schedule::Cron { expr, tz } => {
            let normalized = normalize_expression(expr)?;
            let cron = CronExprSchedule::from_str(&normalized)
                .with_context(|| format!("Invalid cron expression: {expr}"))?;

            if let Some(tz_name) = tz {
                let timezone = chrono_tz::Tz::from_str(tz_name)
                    .with_context(|| format!("Invalid IANA timezone: {tz_name}"))?;
                let localized_from = from.with_timezone(&timezone);
                let next_local = cron.after(&localized_from).next().ok_or_else(|| {
                    anyhow::anyhow!("No future occurrence for expression: {expr}")
                })?;
                Ok(next_local.with_timezone(&Utc))
            } else {
                cron.after(&from)
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("No future occurrence for expression: {expr}"))
            }
        }
        Schedule::At { at } => Ok(*at),
        Schedule::Every { every_ms } => {
            if *every_ms == 0 {
                anyhow::bail!("Invalid schedule: every_ms must be > 0");
            }
            let ms = i64::try_from(*every_ms).context("every_ms is too large")?;
            let delta = ChronoDuration::milliseconds(ms);
            from.checked_add_signed(delta)
                .ok_or_else(|| anyhow::anyhow!("every_ms overflowed DateTime"))
        }
    }
}

pub fn validate_schedule(schedule: &Schedule, now: DateTime<Utc>) -> Result<()> {
    match schedule {
        Schedule::Cron { expr, .. } => {
            let _ = normalize_expression(expr)?;
            let _ = next_run_for_schedule(schedule, now)?;
            Ok(())
        }
        Schedule::At { at } => {
            if *at <= now {
                anyhow::bail!("Invalid schedule: 'at' must be in the future");
            }
            Ok(())
        }
        Schedule::Every { every_ms } => {
            if *every_ms == 0 {
                anyhow::bail!("Invalid schedule: every_ms must be > 0");
            }
            Ok(())
        }
    }
}

pub fn schedule_cron_expression(schedule: &Schedule) -> Option<String> {
    match schedule {
        Schedule::Cron { expr, .. } => Some(expr.clone()),
        _ => None,
    }
}

pub fn normalize_expression(expression: &str) -> Result<String> {
    let expression = expression.trim();
    let field_count = expression.split_whitespace().count();

    match field_count {

        5 => {
            let mut fields: Vec<&str> = expression.split_whitespace().collect();
            let weekday = fields[4];
            let normalized_weekday = normalize_weekday_field(weekday)?;
            fields[4] = &normalized_weekday;
            Ok(format!(
                "0 {} {} {} {} {}",
                fields[0], fields[1], fields[2], fields[3], fields[4]
            ))
        }

        6 | 7 => Ok(expression.to_string()),
        _ => anyhow::bail!(
            "Invalid cron expression: {expression} (expected 5, 6, or 7 fields, got {field_count})"
        ),
    }
}

fn translate_weekday_value(val: u8) -> Result<u8> {
    match val {
        0 | 7 => Ok(1),
        1..=6 => Ok(val + 1),
        _ => anyhow::bail!("Invalid weekday value: {val} (expected 0-7)"),
    }
}

fn normalize_weekday_field(field: &str) -> Result<String> {

    if field == "*" || field == "?" {
        return Ok(field.to_string());
    }

    if field.chars().any(|c| c.is_ascii_alphabetic()) {
        return Ok(field.to_string());
    }

    let parts: Vec<&str> = field.split(',').collect();
    let mut result_parts = Vec::with_capacity(parts.len());

    for part in parts {

        let (range_part, step) = if let Some((r, s)) = part.split_once('/') {
            (r, Some(s))
        } else {
            (part, None)
        };

        let translated = if let Some((start_s, end_s)) = range_part.split_once('-') {
            let start: u8 = start_s
                .parse()
                .with_context(|| format!("Invalid weekday in range: {start_s}"))?;
            let end: u8 = end_s
                .parse()
                .with_context(|| format!("Invalid weekday in range: {end_s}"))?;
            let new_start = translate_weekday_value(start)?;
            let new_end = translate_weekday_value(end)?;
            format!("{new_start}-{new_end}")
        } else if range_part == "*" {
            "*".to_string()
        } else {
            let val: u8 = range_part
                .parse()
                .with_context(|| format!("Invalid weekday value: {range_part}"))?;
            translate_weekday_value(val)?.to_string()
        };

        if let Some(s) = step {
            result_parts.push(format!("{translated}/{s}"));
        } else {
            result_parts.push(translated);
        }
    }

    Ok(result_parts.join(","))
}
