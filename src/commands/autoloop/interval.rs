// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};
use crate::cron::{AgentJobOptions, Schedule};

inventory::submit!(StaticSlashCommand {
    name: "loop",
    aliases: &[],
    description: "Schedule a recurring autonomous agent run at a fixed interval (Loop Engineering automation). Reuses the cron 'every' scheduler.",
    usage: "/loop <interval e.g. 30m|1h|90s> <prompt>",
    category: CommandCategory::Tasks,
    hidden: false,
    requires_interactive: false,
    remote_safe: false,
    handler: make_handler!(handle),
});

const MIN_INTERVAL_MS: u64 = 60_000;

pub async fn handle(ctx: CommandContext) -> CommandResult {
    if ctx.args.len() < 2 {
        return CommandResult::err(
            "Usage: /loop <interval> <prompt>  (interval e.g. 30m, 1h, 90s)",
        );
    }

    let interval_raw = ctx.args[0].clone();
    let prompt = ctx.args[1..].join(" ");
    if prompt.trim().is_empty() {
        return CommandResult::err("A prompt is required: /loop <interval> <prompt>");
    }

    let every_ms = match parse_interval_ms(&interval_raw) {
        Some(ms) if ms >= MIN_INTERVAL_MS => ms,
        Some(_) => {
            return CommandResult::err(
                "Interval too small; minimum is 60s to avoid runaway loops.",
            );
        }
        None => {
            return CommandResult::err(format!(
                "Could not parse interval '{interval_raw}'. Use forms like 30m, 1h, 90s."
            ));
        }
    };

    let Some(svc) = crate::services::try_get_services() else {
        return CommandResult::err("Services not initialized; cannot schedule a loop.");
    };
    let config = (*svc.config()).clone();

    let name = Some(format!("loop:{}", truncate_for_name(&prompt)));
    match crate::cron::add_agent_job(
        &config,
        name,
        Schedule::Every { every_ms },
        &prompt,
        AgentJobOptions::default(),
    ) {
        Ok(job) => CommandResult::ok(format!(
            "Scheduled recurring agent loop every {interval_raw} (job id: {}). \
             It runs the prompt autonomously on each tick; manage or stop it via the cron scheduler.",
            job.id
        )),
        Err(e) => CommandResult::err(format!("Failed to schedule loop: {e}")),
    }
}

pub fn parse_interval_ms(raw: &str) -> Option<u64> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    if let Ok(secs) = s.parse::<u64>() {
        return Some(secs.saturating_mul(1000));
    }
    let (num_part, unit) = s.split_at(s.len() - 1);
    let value: u64 = num_part.trim().parse().ok()?;
    let multiplier_ms = match unit {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return None,
    };
    Some(value.saturating_mul(multiplier_ms))
}

fn truncate_for_name(prompt: &str) -> String {
    let cleaned: String = prompt
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.chars().count() <= 48 {
        trimmed.to_string()
    } else {
        let head: String = trimmed.chars().take(48).collect();
        format!("{head}…")
    }
}
