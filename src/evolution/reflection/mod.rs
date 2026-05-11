// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod lesson;
pub mod reflector;
pub mod store;
pub mod trigger;
pub mod types;
pub mod writeback;

pub use lesson::{ReflectionLesson, ReflectionLessonKind};
pub use reflector::{REFLECTION_QUEUE_CAPACITY, ReflectionRequest, run_reflection_worker};
pub use store::ReflectionStore;
pub use trigger::ReflectionTriggerCause;
pub use types::{ReflectionRun, ReflectionRunStatus, ReflectionSummary, ReflectionWritebackReport};
pub use writeback::apply_writeback;
