// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod automation;
pub mod catalogue;
pub mod engine;
pub mod instructions;
pub mod skill;
pub mod values;

pub use automation::{AutomationPlan, BuiltAutomation};
pub use catalogue::{build_targets, Architecture};
pub use skill::{BuiltSkill, SkillPlan};
