// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;

use parking_lot::RwLock;

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum Segment {

    System,

    Memory,

    Experience,

    ToolsSchema,

    History,

    TurnInput,

    TurnOutput,

    Reserved,
}

impl Segment {

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Memory => "memory",
            Self::Experience => "experience",
            Self::ToolsSchema => "tools_schema",
            Self::History => "history",
            Self::TurnInput => "turn_input",
            Self::TurnOutput => "turn_output",
            Self::Reserved => "reserved",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetError {

    Insufficient { want: u64, got: u64 },
}

impl std::fmt::Display for BudgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Insufficient { want, got } => {
                write!(f, "budget: insufficient tokens (want {want}, have {got})")
            }
        }
    }
}

impl std::error::Error for BudgetError {}

#[derive(Debug, Clone, Default)]
pub struct BudgetSnapshot {

    pub total: u64,

    pub used: u64,

    pub remaining: u64,

    pub per_segment: Vec<(&'static str, u64)>,
}

pub struct BudgetLedger {
    total: u64,
    inner: RwLock<LedgerInner>,
}

#[derive(Default)]
struct LedgerInner {
    allocations: HashMap<Segment, u64>,
}

impl BudgetLedger {

    pub fn new(total: u64) -> Self {
        Self {
            total,
            inner: RwLock::new(LedgerInner::default()),
        }
    }

    #[inline]
    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn remaining(&self) -> u64 {
        let inner = self.inner.read();
        let used: u64 = inner.allocations.values().copied().sum();
        self.total.saturating_sub(used)
    }

    pub fn reserved(&self, seg: Segment) -> u64 {
        let inner = self.inner.read();
        inner.allocations.get(&seg).copied().unwrap_or(0)
    }

    pub fn reserve(&self, seg: Segment, want: u64) -> Result<u64, BudgetError> {
        let mut inner = self.inner.write();
        let used: u64 = inner.allocations.values().copied().sum();
        let remaining = self.total.saturating_sub(used);
        if want > remaining {
            return Err(BudgetError::Insufficient {
                want,
                got: remaining,
            });
        }
        *inner.allocations.entry(seg).or_insert(0) += want;
        Ok(want)
    }

    pub fn release(&self, seg: Segment, n: u64) {
        let mut inner = self.inner.write();
        if let Some(cur) = inner.allocations.get_mut(&seg) {
            *cur = cur.saturating_sub(n);
            if *cur == 0 {
                inner.allocations.remove(&seg);
            }
        }
    }

    pub fn reset(&self, seg: Segment) {
        let mut inner = self.inner.write();
        inner.allocations.remove(&seg);
    }

    pub fn reset_all(&self) {
        let mut inner = self.inner.write();
        inner.allocations.clear();
    }

    pub fn snapshot(&self) -> BudgetSnapshot {
        let inner = self.inner.read();
        let used: u64 = inner.allocations.values().copied().sum();
        let mut per_segment: Vec<(&'static str, u64)> = inner
            .allocations
            .iter()
            .map(|(seg, n)| (seg.as_str(), *n))
            .collect();
        per_segment.sort_by_key(|(k, _)| *k);
        BudgetSnapshot {
            total: self.total,
            used,
            remaining: self.total.saturating_sub(used),
            per_segment,
        }
    }
}

impl std::fmt::Debug for BudgetLedger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.snapshot();
        f.debug_struct("BudgetLedger")
            .field("total", &s.total)
            .field("used", &s.used)
            .field("remaining", &s.remaining)
            .field("per_segment", &s.per_segment)
            .finish()
    }
}
