// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[derive(Debug, Clone, PartialEq)]
pub struct BudgetRow {
    pub segment: String,
    pub reserved: u64,

    pub share: f32,
}

#[derive(Debug, Clone, Default)]
pub struct BudgetView {
    total: u64,
    used: u64,
    rows: Vec<BudgetRow>,
}

impl BudgetView {
    pub fn new(total: u64, used: u64, per_segment: Vec<(String, u64)>) -> Self {
        let total_denom = total.max(1);
        let mut rows: Vec<BudgetRow> = per_segment
            .into_iter()
            .map(|(name, reserved)| {
                let share = (reserved as f32 / total_denom as f32).clamp(0.0, 1.0);
                BudgetRow {
                    segment: name,
                    reserved,
                    share,
                }
            })
            .collect();
        rows.sort_by(|a, b| b.reserved.cmp(&a.reserved).then(a.segment.cmp(&b.segment)));
        Self { total, used, rows }
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn used(&self) -> u64 {
        self.used
    }

    pub fn remaining(&self) -> u64 {
        self.total.saturating_sub(self.used)
    }

    pub fn usage_ratio(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.used as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }

    pub fn rows(&self) -> &[BudgetRow] {
        &self.rows
    }

    pub fn header_line(&self) -> String {
        format!(
            "{} / {} tokens ({} %)",
            self.used,
            self.total,
            (self.usage_ratio() * 100.0) as u32
        )
    }
}
