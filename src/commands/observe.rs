// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "observe",
    aliases: &[],
    description: "Manage Loop Engineering Observe sources: a debounced file-watch trigger and an on-demand git-change trigger that submit autonomous fix tasks to the task queue.",
    usage: "/observe <watch [dir] [ext,ext]|git|stop|status>",
    category: CommandCategory::Tasks,
    hidden: false,
    requires_interactive: false,
    remote_safe: false,
    handler: make_handler!(handle),
});

pub async fn handle(ctx: CommandContext) -> CommandResult {
    let subcmd = ctx.args.first().map(|s| s.as_str()).unwrap_or("status");

    let Some(svc) = crate::services::try_get_services() else {
        return CommandResult::err("Services not initialized; cannot manage Observe sources.");
    };
    let config = (*svc.config()).clone();

    match subcmd {
        "watch" => {
            let root = ctx
                .args
                .get(1)
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| ctx.cwd.clone());
            if !root.exists() {
                return CommandResult::err(format!(
                    "Directory does not exist: {}",
                    root.display()
                ));
            }
            let extensions: Vec<String> = ctx
                .args
                .get(2)
                .map(|raw| raw.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default();

            match crate::agent::observe::start_file_watch(&config, root.clone(), extensions) {
                Ok(()) => CommandResult::ok(format!(
                    "File-watch trigger started on `{}`. Changes will submit autonomous review/fix \
                     tasks to the task queue (a worker is running to consume them). Use `/observe stop` to halt.",
                    root.display()
                )),
                Err(e) => CommandResult::err(e),
            }
        }
        "git" => match crate::agent::observe::trigger_from_git(&config, &ctx.cwd).await {
            Ok(Some(task_id)) => CommandResult::ok(format!(
                "Git trigger submitted an autonomous review task (id: {task_id}) for the current working-tree changes."
            )),
            Ok(None) => CommandResult::ok("No working-tree changes detected; nothing to submit."),
            Err(e) => CommandResult::err(e),
        },
        "stop" => {
            if crate::agent::observe::stop_file_watch() {
                CommandResult::ok("File-watch trigger stopped.")
            } else {
                CommandResult::ok("No file-watch trigger was running.")
            }
        }
        "status" => {
            let watching = crate::agent::observe::is_watching();
            CommandResult::ok(format!(
                "Observe sources: file-watch is {}.",
                if watching { "running" } else { "stopped" }
            ))
        }
        other => CommandResult::err(format!(
            "Unknown /observe subcommand '{other}'. Available: watch, git, stop, status."
        )),
    }
}
