// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Context module — mirrors claude-code's `context/` and `context.ts`.
//
// Builds the runtime context for agent queries: git status, AGENTS.md
// loading, memory injection, system prompt assembly, and context caching.

pub mod budget;
pub mod builder;
pub mod git;
pub mod lsp_ctx;
pub mod memory_files;
pub mod notifications;
pub mod open_files;
pub mod outline_ctx;
pub mod rag_ctx;
pub mod symbols_ctx;
pub mod system_prompt;
