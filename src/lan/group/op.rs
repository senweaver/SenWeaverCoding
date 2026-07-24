// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::BTreeMap;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Hlc {
    #[serde(rename = "ms")]
    pub millis: u64,
    #[serde(rename = "c")]
    pub counter: u32,
}

impl Hlc {
    pub fn zero() -> Self {
        Self {
            millis: 0,
            counter: 0,
        }
    }
}

pub struct HlcClock {
    last: Mutex<Hlc>,
}

impl HlcClock {
    pub fn new() -> Self {
        Self {
            last: Mutex::new(Hlc::zero()),
        }
    }

    pub fn tick(&self) -> Hlc {
        let mut last = self.last.lock();
        let phys = now_ms_u64();
        let next = if phys > last.millis {
            Hlc {
                millis: phys,
                counter: 0,
            }
        } else {
            Hlc {
                millis: last.millis,
                counter: last.counter.saturating_add(1),
            }
        };
        *last = next;
        next
    }

    pub fn observe(&self, remote: Hlc) {
        let mut last = self.last.lock();
        let now = now_ms_u64();
        let phys = Hlc {
            millis: now,
            counter: 0,
        };
        const MAX_DRIFT_MS: u64 = 5 * 60 * 1000;
        let remote = if remote.millis > now.saturating_add(MAX_DRIFT_MS) {
            tracing::warn!(
                target: "lan.group",
                remote_ms = remote.millis,
                now_ms = now,
                "clamping remote HLC timestamp beyond max drift"
            );
            Hlc {
                millis: now.saturating_add(MAX_DRIFT_MS),
                counter: remote.counter,
            }
        } else {
            remote
        };
        let merged = (*last).max(remote).max(phys);
        *last = merged;
    }
}

impl Default for HlcClock {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VersionVector(pub BTreeMap<String, u64>);

impl VersionVector {
    pub fn covers(&self, author: &str, seq: u64) -> bool {
        self.0.get(author).is_some_and(|seen| *seen >= seq)
    }

    pub fn set(&mut self, author: &str, seq: u64) {
        self.0.insert(author.to_string(), seq);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupRole {
    Owner,
    Manager,
    Member,
    Viewer,
}

impl GroupRole {
    pub fn rank(self) -> u8 {
        match self {
            GroupRole::Owner => 3,
            GroupRole::Manager => 2,
            GroupRole::Member => 1,
            GroupRole::Viewer => 0,
        }
    }

    pub fn can_manage(self) -> bool {
        self.rank() >= GroupRole::Manager.rank()
    }

    pub fn can_contribute(self) -> bool {
        self.rank() >= GroupRole::Member.rank()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            GroupRole::Owner => "owner",
            GroupRole::Manager => "manager",
            GroupRole::Member => "member",
            GroupRole::Viewer => "viewer",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(GroupRole::Owner),
            "manager" => Some(GroupRole::Manager),
            "member" => Some(GroupRole::Member),
            "viewer" => Some(GroupRole::Viewer),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum GroupOpPayload {
    GroupMeta {
        name: String,
        description: String,
    },
    MemberUpsert {
        user_id: String,
        nickname: String,
        role: GroupRole,
    },
    MemberRemove {
        user_id: String,
    },
    PhaseUpsert {
        phase_id: String,
        name: String,
        order: i64,
        status: String,
        color: String,
    },
    PhaseRemove {
        phase_id: String,
    },
    DocUpsert {
        doc_id: String,
        name: String,
        is_dir: bool,
        size: i64,
        phase_id: String,
        uploader: String,
        content_hash: String,
        version: i64,
        note: String,
    },
    DocRemove {
        doc_id: String,
    },
    TaskUpsert {
        task_id: String,
        title: String,
        description: String,
        phase_id: String,
        assignee: String,
        status: String,
        priority: String,
        due_ms: i64,
        deps: Vec<String>,
        parent: String,
        kind: String,
        progress: i64,
    },
    TaskRemove {
        task_id: String,
    },
    ChatPost {
        msg_id: String,
        body: String,
        kind: String,
        doc_id: String,
        ts_ms: i64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupOp {
    pub op_id: String,
    pub group_id: String,
    pub hlc: Hlc,
    #[serde(default)]
    pub seq: u64,
    pub author: String,
    pub payload: GroupOpPayload,
}

impl GroupOp {
    pub fn order_key(&self) -> (Hlc, &str, &str) {
        (self.hlc, self.author.as_str(), self.op_id.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupVvEntry {
    #[serde(rename = "groupId")]
    pub group_id: String,
    pub vv: VersionVector,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupOpsEntry {
    #[serde(rename = "groupId")]
    pub group_id: String,
    pub ops: Vec<GroupOp>,
}

pub enum GroupInbound {
    Gossip {
        group_id: String,
        ops: Vec<GroupOp>,
    },
    SyncRequest {
        groups: Vec<GroupVvEntry>,
    },
    SyncResponse {
        groups: Vec<GroupOpsEntry>,
    },
    Invite {
        group_id: String,
        ops: Vec<GroupOp>,
    },
    DocRequest {
        group_id: String,
        doc_id: String,
    },
}

pub fn now_ms_u64() -> u64 {
    let millis = chrono::Utc::now().timestamp_millis();
    if millis < 0 {
        0
    } else {
        millis as u64
    }
}
