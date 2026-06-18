// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashSet;

use super::op::{GroupOpsEntry, GroupVvEntry};
use super::store::GroupStore;
use crate::lan::protocol::ControlMessage;

pub fn sync_request(store: &GroupStore) -> ControlMessage {
    let group_ids = store.groups_for_member(store.self_user_id());
    let groups: Vec<GroupVvEntry> = group_ids
        .into_iter()
        .map(|group_id| {
            let vv = store.version_vector(&group_id);
            GroupVvEntry { group_id, vv }
        })
        .collect();
    ControlMessage::GroupSyncRequest { groups }
}

pub fn sync_response_for_request(
    store: &GroupStore,
    requester_id: &str,
    requested: &[GroupVvEntry],
) -> Option<ControlMessage> {
    let mut groups: Vec<GroupOpsEntry> = Vec::new();
    let requested_ids: HashSet<&str> = requested.iter().map(|e| e.group_id.as_str()).collect();

    for entry in requested {
        if !store.group_exists(&entry.group_id) {
            continue;
        }
        let ops = store.ops_since(&entry.group_id, &entry.vv);
        if !ops.is_empty() {
            groups.push(GroupOpsEntry {
                group_id: entry.group_id.clone(),
                ops,
            });
        }
    }

    for group_id in store.groups_for_member(requester_id) {
        if requested_ids.contains(group_id.as_str()) {
            continue;
        }
        let ops = store.ops_for_group(&group_id);
        if !ops.is_empty() {
            groups.push(GroupOpsEntry { group_id, ops });
        }
    }

    if groups.is_empty() {
        None
    } else {
        Some(ControlMessage::GroupSyncResponse { groups })
    }
}
