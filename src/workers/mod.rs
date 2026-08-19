// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod events;
pub mod persistence;
pub mod router;
pub mod runner;
pub mod supervisor;
pub mod worker;
pub mod overlay;
pub mod worktree;
pub mod ws;

pub use events::{WorkerMeta, WorkerResult, WorkerSpec, WorkerStatus, WorkerSummary};
pub use persistence::{
    WorkerEventLog, find_worker_root, list_meta, mark_worker_failed, read_meta,
    scan_and_recover, scan_interrupted, workers_root, write_meta,
};
pub use runner::{WorkerRunContext, run_worker};
pub use supervisor::{
    WorkerSupervisor, candidate_worker_roots, ensure_supervisor, global_supervisor,
    init_global_supervisor, scan_and_recover_at, scan_and_recover_with_resume,
    try_init_default,
};
pub use worker::WorkerHandle;
pub use worktree::WorktreeInfo;
