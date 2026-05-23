// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "agents",
    aliases: &["agent-list"],
    description: "List registered agents in the multi-agent runtime",
    usage: "/agents",
    category: CommandCategory::Configuration,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_agents),
});

inventory::submit!(StaticSlashCommand {
    name: "tasks",
    aliases: &["task-list"],
    description: "Show pending and running tasks in the multi-agent queue",
    usage: "/tasks",
    category: CommandCategory::Configuration,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_tasks),
});

inventory::submit!(StaticSlashCommand {
    name: "blackboard",
    aliases: &["bb"],
    description: "Inspect a shared blackboard entry by key",
    usage: "/blackboard <key> | /blackboard --list",
    category: CommandCategory::Configuration,
    hidden: false,
    requires_interactive: false,
    remote_safe: true,
    handler: make_handler!(handle_blackboard),
});

pub async fn handle_agents(_ctx: CommandContext) -> CommandResult {
    let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() else {
        return CommandResult::err(
            "Multi-agent runtime not initialized. Start via `sen` or enable swarm mode.",
        );
    };
    let summary = rt.health_summary();
    let mut body = format!(
        "Multi-Agent Runtime\n\
         ─────────────────────────\n\
         Total agents:   {}\n\
         Healthy:        {}\n\
         Unhealthy:      {}\n\
         Pending tasks:  {}\n\
         Running tasks:  {}\n\
         Blackboard:     {} entries\n",
        summary.total_agents,
        summary.healthy_agents,
        summary.unhealthy_agents,
        summary.pending_tasks,
        summary.running_tasks,
        summary.blackboard_entries,
    );
    let agents = rt.registry.all();
    if !agents.is_empty() {
        body.push_str("\nAgents:\n");
        for a in agents {
            body.push_str(&format!(
                "  - {} ({}): role={}, state={:?}, tasks_completed={}\n",
                a.id, a.name, a.role, a.state, a.tasks_completed
            ));
        }
    }
    CommandResult::ok(body)
}

pub async fn handle_tasks(_ctx: CommandContext) -> CommandResult {
    let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() else {
        return CommandResult::err("Multi-agent runtime not initialized");
    };
    let pending = rt.task_queue.pending_count();
    let running = rt.task_queue.running_count();
    let body = format!(
        "Task Queue\n\
         ─────────────────────────\n\
         Pending:  {}\n\
         Running:  {}\n",
        pending, running
    );
    CommandResult::ok(body)
}

pub async fn handle_blackboard(ctx: CommandContext) -> CommandResult {
    let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() else {
        return CommandResult::err("Multi-agent runtime not initialized");
    };

    if ctx.args.is_empty() {
        return CommandResult::err("Usage: /blackboard <key>");
    }

    if ctx.args[0] == "--list" {
        let len = rt.blackboard.inner().len();
        return CommandResult::ok(format!("Blackboard has {} entries", len));
    }

    let key = &ctx.args[0];
    match rt.blackboard.inner().read(key) {
        Some(entry) => CommandResult::ok(format!(
            "Key: {}\nNamespace: {}\nVersion: {}\nOwner: {}\nValue:\n{}",
            key,
            entry.namespace,
            entry.version,
            entry.owner,
            serde_json::to_string_pretty(&entry.value).unwrap_or_default()
        )),
        None => CommandResult::err(format!("Key '{}' not found", key)),
    }
}
