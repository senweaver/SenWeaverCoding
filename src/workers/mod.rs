// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod events;
pub mod persistence;
pub mod router;
pub mod runner;
pub mod supervisor;
pub mod worker;
pub mod ws;

pub use events::{WorkerMeta, WorkerResult, WorkerSpec, WorkerStatus, WorkerSummary};
pub use persistence::{
    WorkerEventLog, list_meta, read_meta, scan_and_recover, workers_root, write_meta,
};
pub use router::router as workers_router;
pub use runner::{WorkerRunContext, run_worker};
pub use supervisor::{
    WorkerSupervisor, ensure_supervisor, global_supervisor, init_global_supervisor,
    scan_and_recover_at, try_init_default,
};
pub use worker::WorkerHandle;
