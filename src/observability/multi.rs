// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Observer, ObserverEvent, ObserverMetric};
use std::any::Any;

pub struct MultiObserver {
    observers: Vec<Box<dyn Observer>>,
}

impl MultiObserver {
    pub fn new(observers: Vec<Box<dyn Observer>>) -> Self {
        Self { observers }
    }
}

impl Observer for MultiObserver {
    fn record_event(&self, event: &ObserverEvent) {
        for obs in &self.observers {
            obs.record_event(event);
        }
    }

    fn record_metric(&self, metric: &ObserverMetric) {
        for obs in &self.observers {
            obs.record_metric(metric);
        }
    }

    fn flush(&self) {
        for obs in &self.observers {
            obs.flush();
        }
    }

    fn name(&self) -> &str {
        "multi"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
