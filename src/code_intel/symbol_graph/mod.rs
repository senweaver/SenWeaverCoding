// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod core;
pub mod impact;
pub mod incremental;

pub use core::*;
pub use impact::{ImpactResult, impact_radius, max_impact_depth, max_impact_nodes, seeds_for_files};
