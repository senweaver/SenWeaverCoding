// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Schemas module — mirrors claude-code's `schemas/` directory.
// Defines hook schemas and validation for plugin/SDK hook registration.

pub mod hooks;

pub use hooks::{HookEventSchema, HookSchema, validate_hook_config};
