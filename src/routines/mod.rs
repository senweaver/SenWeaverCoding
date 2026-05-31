// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod engine;
pub mod event_matcher;
pub use engine::{
    Routine, RoutineAction, RoutineDispatchResult, RoutinesEngine, load_routines,
    load_routines_from_file,
};
pub use event_matcher::{EventPattern, MatchStrategy, RoutineEvent, matches, matches_any};
