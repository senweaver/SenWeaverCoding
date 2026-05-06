// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Bidirectional credential-leak scanning.
//!
//! [`super::leak_detector::LeakDetector`] was originally
//! wired only to the **outbound** channel pipeline.  This module
//! exposes a symmetric [`ScanDirection`] enum and a [`scan_io`]
//! helper so callers can run the same detector on both legs:
//!
//! - **Inbound** messages (user input, webhook payloads) are scanned
//!   to prevent an attacker from planting credentials we would later
//!   echo back.
//! - **Outbound** messages (agent replies, tool outputs) continue to
//!   be scanned to stop accidental exfiltration.
//!
//! The helper is intentionally thin: it does **not** mutate the
//! input, only reports, so callers decide whether to redact, block,
//! or just log.

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
