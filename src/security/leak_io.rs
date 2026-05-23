// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::leak_detector::{LeakDetector, LeakResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanDirection {

    Inbound,

    Outbound,
}

impl ScanDirection {

    pub fn label(&self) -> &'static str {
        match self {
            ScanDirection::Inbound => "inbound",
            ScanDirection::Outbound => "outbound",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanOutcome {
    pub direction: ScanDirection,
    pub result: LeakResult,
}

impl ScanOutcome {

    pub fn has_leak(&self) -> bool {
        !matches!(self.result, LeakResult::Clean)
    }
}

pub fn scan_io(detector: &LeakDetector, content: &str, direction: ScanDirection) -> ScanOutcome {
    ScanOutcome {
        direction,
        result: detector.scan(content),
    }
}
