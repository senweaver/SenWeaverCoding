// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkStatus {
    Pending,
    Applied,
    Reverted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingStatus {

    Pending,

    Applied,

    Reverted,

    PartiallyReverted,
}

#[derive(Debug, Clone)]
pub struct PendingEdit {

    pub id: u64,
    pub timestamp: String,
    pub path: String,
    pub additions: i32,
    pub deletions: i32,

    pub diff: Option<String>,

    pub edit_batch_id: Option<String>,

    pub hunk_status: Vec<HunkStatus>,

    pub from_inline_edit: bool,
}

impl PendingEdit {

    #[must_use]
    pub fn status(&self) -> PendingStatus {
        if self.hunk_status.is_empty() {
            return PendingStatus::Pending;
        }
        let applied = self
            .hunk_status
            .iter()
            .filter(|s| **s == HunkStatus::Applied)
            .count();
        let reverted = self
            .hunk_status
            .iter()
            .filter(|s| **s == HunkStatus::Reverted)
            .count();
        let total = self.hunk_status.len();
        if applied == total {
            PendingStatus::Applied
        } else if reverted == total {
            PendingStatus::Reverted
        } else if reverted > 0 && applied > 0 {
            PendingStatus::PartiallyReverted
        } else if reverted > 0 {
            PendingStatus::PartiallyReverted
        } else {
            PendingStatus::Pending
        }
    }
}

#[derive(Debug)]
pub struct EditBatchRegistry {
    entries: VecDeque<PendingEdit>,
    capacity: usize,
    next_id: u64,
}

impl Default for EditBatchRegistry {
    fn default() -> Self {
        Self::new(256)
    }
}

impl EditBatchRegistry {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity.min(256)),
            capacity,
            next_id: 1,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries_newest_first(&self) -> impl Iterator<Item = &PendingEdit> {
        self.entries.iter().rev()
    }

    pub fn recent_edits_for_nep(
        &self,
        limit: usize,
    ) -> Vec<crate::inline_completion::nep::RecentEdit> {
        let mut out = Vec::with_capacity(limit.min(self.entries.len()));
        for (idx, entry) in self.entries.iter().rev().take(limit).enumerate() {
            let Some(diff) = entry.diff.as_ref() else {
                continue;
            };
            out.push(crate::inline_completion::nep::RecentEdit {
                file_path: std::path::PathBuf::from(&entry.path),
                diff: diff.clone(),
                instruction: None,
                since_start_ms: idx as u64,
            });
        }
        out
    }

    pub fn entries(&self) -> &VecDeque<PendingEdit> {
        &self.entries
    }

    pub fn get_mut_by_id(&mut self, id: u64) -> Option<&mut PendingEdit> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    pub fn find_by_batch_id(&self, batch_id: &str) -> Option<&PendingEdit> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.edit_batch_id.as_deref() == Some(batch_id))
    }

    pub fn push_from_file_edit(
        &mut self,
        path: String,
        additions: i32,
        deletions: i32,
        diff: Option<String>,
        edit_batch_id: Option<String>,
        timestamp: String,
    ) -> u64 {
        let hunk_count = diff
            .as_deref()
            .map(count_hunks_in_unified_diff)
            .unwrap_or(0);
        let hunk_status = vec![HunkStatus::Pending; hunk_count];
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push_back(PendingEdit {
            id,
            timestamp,
            path,
            additions,
            deletions,
            diff,
            edit_batch_id,
            hunk_status,
            from_inline_edit: false,
        });
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
        id
    }

    pub fn mark_latest_inline_for(&mut self, path: &std::path::Path) -> bool {
        let needle = path.to_string_lossy().replace('\\', "/");
        let needle_suffix = needle
            .rsplit('/')
            .next()
            .unwrap_or(needle.as_str())
            .to_string();
        if let Some(entry) = self.entries.iter_mut().rev().find(|e| {
            let p = e.path.replace('\\', "/");
            p == needle || p.ends_with(&needle) || p.ends_with(&needle_suffix)
        }) {
            entry.from_inline_edit = true;
            true
        } else {
            false
        }
    }

    pub fn push_from_inline_edit(
        &mut self,
        path: String,
        additions: i32,
        deletions: i32,
        diff: Option<String>,
        edit_batch_id: Option<String>,
        timestamp: String,
    ) -> u64 {
        let hunk_count = diff
            .as_deref()
            .map(count_hunks_in_unified_diff)
            .unwrap_or(0);
        let hunk_status = vec![HunkStatus::Applied; hunk_count];
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push_back(PendingEdit {
            id,
            timestamp,
            path,
            additions,
            deletions,
            diff,
            edit_batch_id,
            hunk_status,
            from_inline_edit: true,
        });
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
        id
    }
}

fn count_hunks_in_unified_diff(diff: &str) -> usize {
    diff.lines()
        .filter(|l| l.starts_with("@@") && l.contains("@@"))
        .count()
}
