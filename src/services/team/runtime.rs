// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::agent::team_protocol::{MessagePayload, Role, Team, TeamConfig, TeamMessage};

#[derive(Debug, Clone, Serialize)]
pub struct TeamMessageRecord {
    pub id: u64,
    pub from: String,
    pub to: String,
    pub content: String,
    pub channel: String,
    pub timestamp: String,
}

struct TeamRuntimeEntry {
    team: Team,
    log: Vec<TeamMessageRecord>,
    next_id: u64,
    _keepalive: broadcast::Receiver<TeamMessage>,
}

type Manager = Arc<RwLock<HashMap<String, TeamRuntimeEntry>>>;

fn manager() -> &'static Manager {
    static GLOBAL: OnceLock<Manager> = OnceLock::new();
    GLOBAL.get_or_init(|| Arc::new(RwLock::new(HashMap::new())))
}

pub fn create_team(
    id: &str,
    name: &str,
    members: &[String],
    leader: Option<&str>,
    config: TeamConfig,
) -> Result<Vec<String>, String> {
    if manager().read().contains_key(id) {
        return Err(format!("team '{id}' already exists"));
    }
    let mut team = Team::new(id.to_string(), name.to_string(), config);
    let mut rejected: Vec<String> = Vec::new();
    for member in members {
        let role = if leader == Some(member.as_str()) {
            Role::Orchestrator
        } else {
            Role::Specialist
        };
        if let Err(e) = team.add_member(member.clone(), role) {
            tracing::warn!(team = id, member = %member, error = %e, "team member rejected");
            rejected.push(member.clone());
        }
    }
    let keepalive = team.subscribe();
    let entry = TeamRuntimeEntry {
        team,
        log: Vec::new(),
        next_id: 1,
        _keepalive: keepalive,
    };
    let mut guard = manager().write();
    if guard.contains_key(id) {
        return Err(format!("team '{id}' already exists"));
    }
    guard.insert(id.to_string(), entry);
    Ok(rejected)
}

pub fn delete_team(id: &str) -> bool {
    manager().write().remove(id).is_some()
}

pub fn team_exists(id: &str) -> bool {
    manager().read().contains_key(id)
}

pub fn send_message(
    team_id: &str,
    from: &str,
    to: &str,
    content: &str,
) -> Result<TeamMessageRecord, String> {
    let mut guard = manager().write();
    let entry = guard
        .get_mut(team_id)
        .ok_or_else(|| format!("team '{team_id}' not found"))?;

    let is_broadcast = to.eq_ignore_ascii_case("broadcast") || to.is_empty();
    if !entry.team.members.contains_key(from) {
        return Err(format!(
            "sender '{from}' is not a member of team '{team_id}'"
        ));
    }
    if !is_broadcast && !entry.team.members.contains_key(to) {
        return Err(format!(
            "recipient '{to}' is not a member of team '{team_id}'"
        ));
    }
    let id = entry.next_id;
    entry.next_id += 1;
    let record = TeamMessageRecord {
        id,
        from: from.to_string(),
        to: if is_broadcast {
            "broadcast".to_string()
        } else {
            to.to_string()
        },
        content: content.to_string(),
        channel: if is_broadcast { "broadcast" } else { "direct" }.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    entry.log.push(record.clone());
    const MAX_TEAM_LOG: usize = 2_000;
    if entry.log.len() > MAX_TEAM_LOG {
        let overflow = entry.log.len() - MAX_TEAM_LOG;
        entry.log.drain(0..overflow);
    }

    let payload = MessagePayload::Text(content.to_string());
    let send_result = if is_broadcast {
        entry.team.broadcast(id, &from.to_string(), payload)
    } else {
        entry
            .team
            .send_direct(id, &from.to_string(), &to.to_string(), payload)
    };
    if let Err(e) = send_result {
        tracing::debug!(team = team_id, error = %e, "team bus send had no active subscribers");
    }

    Ok(record)
}

pub fn transcript(team_id: &str, member: &str) -> Option<Vec<TeamMessageRecord>> {
    let guard = manager().read();
    let entry = guard.get(team_id)?;
    if member.is_empty() {
        return Some(entry.log.clone());
    }
    Some(
        entry
            .log
            .iter()
            .filter(|m| m.to == member || m.to == "broadcast" || m.from == member)
            .cloned()
            .collect(),
    )
}
