// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Turn-level façade.
//!
//! The /2 turn entry points live in `agent::loop_`
//! (`agent_turn`, `run_tool_call_loop`, `run`, `process_message`).
//! D2.1 introduces a stable `turn_engine::turn::*` surface
//! that currently re-exports the legacy functions by name.  Subsequent
//! sprints can move the bodies here without touching call sites.
//!
//! This is not a compatibility shim — the re-exports are *deliberate*
//! because they communicate intent to future maintainers: "this is the
//! target location for the turn implementation".

#[doc(inline)]
pub use crate::agent::loop_::process_message;
#[doc(inline)]
pub use crate::agent::loop_::run;
