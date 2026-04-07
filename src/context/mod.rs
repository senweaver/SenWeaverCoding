// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Context module — mirrors claude-code's `context/` and `context.ts`.
//
// Builds the runtime context for agent queries: git status, AGENTS.md
// loading, memory injection, system prompt assembly, and context caching.

pub mod builder;
pub mod git;
pub mod memory_files;
pub mod notifications;
pub mod system_prompt;

pub use builder::ContextBuilder;
pub use notifications::{NotificationContext, NotificationEntry, NotificationPriority};
pub use system_prompt::SystemPromptParts;
