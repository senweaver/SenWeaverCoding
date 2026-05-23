// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod engine;
pub mod event_matcher;

#[allow(unused_imports)]
pub use engine::{
    Routine, RoutineAction, RoutineDispatchResult, RoutinesEngine, load_routines,
    load_routines_from_file,
};
#[allow(unused_imports)]
pub use event_matcher::{EventPattern, MatchStrategy, RoutineEvent, matches, matches_any};
