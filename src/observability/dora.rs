// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::collections::VecDeque;
use std::sync::RwLock;
use std::time::Duration;

use chrono::{DateTime, Utc};

const MAX_RECORDS: usize = 1000;

const WINDOW_7D: Duration = Duration::from_secs(7 * 24 * 3600);
const WINDOW_30D: Duration = Duration::from_secs(30 * 24 * 3600);
const WINDOW_90D: Duration = Duration::from_secs(90 * 24 * 3600);

#[derive(Debug, Clone)]
struct DeploymentRecord {

    timestamp: DateTime<Utc>,

    success: bool,

    lead_time: Option<Duration>,
}

#[derive(Debug, Clone)]
struct RecoveryRecord {
    timestamp: DateTime<Utc>,
    duration: Duration,
}

#[derive(Debug, Clone)]
pub struct DoraSnapshot {

    pub total_deployments: u64,

    pub failed_deployments: u64,

    pub change_failure_rate: Option<f64>,

    pub mean_lead_time: Option<Duration>,

    pub mttr: Option<Duration>,

    pub window: Duration,
}

#[derive(Debug, Default)]
struct CollectorState {
    deployments: VecDeque<DeploymentRecord>,
    recoveries: VecDeque<RecoveryRecord>,
}

pub struct DoraCollector {
    inner: RwLock<CollectorState>,
}

impl DoraCollector {

    pub fn new() -> Self {
        Self {
            inner: RwLock::new(CollectorState::default()),
        }
    }

    pub fn record_deployment(&self, success: bool, lead_time: Option<Duration>) {
        let mut state = self.inner.write().expect("DORA lock poisoned");
        if state.deployments.len() >= MAX_RECORDS {
            state.deployments.pop_front();
        }
        state.deployments.push_back(DeploymentRecord {
            timestamp: Utc::now(),
            success,
            lead_time,
        });
    }

    pub fn record_failure(&self) {
        self.record_deployment(false, None);
    }

    pub fn record_recovery(&self, duration: Duration) {
        let mut state = self.inner.write().expect("DORA lock poisoned");
        if state.recoveries.len() >= MAX_RECORDS {
            state.recoveries.pop_front();
        }
        state.recoveries.push_back(RecoveryRecord {
            timestamp: Utc::now(),
            duration,
        });
    }

    pub fn snapshot_7d(&self) -> DoraSnapshot {
        self.snapshot_window(WINDOW_7D)
    }

    pub fn snapshot_30d(&self) -> DoraSnapshot {
        self.snapshot_window(WINDOW_30D)
    }

    pub fn snapshot_90d(&self) -> DoraSnapshot {
        self.snapshot_window(WINDOW_90D)
    }

    pub fn snapshot(&self) -> DoraSnapshot {
        self.snapshot_window(WINDOW_30D)
    }

    fn snapshot_window(&self, window: Duration) -> DoraSnapshot {
        let state = self.inner.read().expect("DORA lock poisoned");
        let cutoff =
            Utc::now() - chrono::Duration::from_std(window).unwrap_or(chrono::Duration::MAX);

        let deploys_in_window: Vec<&DeploymentRecord> = state
            .deployments
            .iter()
            .filter(|d| d.timestamp >= cutoff)
            .collect();

        let total_deployments = deploys_in_window.len() as u64;
        let failed_deployments = deploys_in_window.iter().filter(|d| !d.success).count() as u64;

        let change_failure_rate = if total_deployments > 0 {
            Some(failed_deployments as f64 / total_deployments as f64)
        } else {
            None
        };

        let lead_times: Vec<Duration> = deploys_in_window
            .iter()
            .filter_map(|d| d.lead_time)
            .collect();
        let mean_lead_time = if lead_times.is_empty() {
            None
        } else {
            let count = u32::try_from(lead_times.len()).unwrap_or(u32::MAX);
            let total: Duration = lead_times.iter().sum();
            Some(total / count)
        };

        let recoveries_in_window: Vec<&RecoveryRecord> = state
            .recoveries
            .iter()
            .filter(|r| r.timestamp >= cutoff)
            .collect();
        let mttr = if recoveries_in_window.is_empty() {
            None
        } else {
            let count = u32::try_from(recoveries_in_window.len()).unwrap_or(u32::MAX);
            let total: Duration = recoveries_in_window.iter().map(|r| r.duration).sum();
            Some(total / count)
        };

        DoraSnapshot {
            total_deployments,
            failed_deployments,
            change_failure_rate,
            mean_lead_time,
            mttr,
            window,
        }
    }
}

impl Default for DoraCollector {
    fn default() -> Self {
        Self::new()
    }
}
