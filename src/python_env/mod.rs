// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod activate;
pub mod cache;
pub mod discover;
pub mod events;
pub mod manager;

pub use activate::activation_env;
pub use cache::{forget_state, load_state, store_state};
pub use discover::{
    detect_workspace_project, discover_interpreters, read_required_python,
    recommend_install_strategy, InstallRecommendation, InstallStrategy, InterpreterInfo,
    ProjectMarkers, RequiredPython,
};
pub use events::{subscribe_events, PythonEnvEvent};
pub use manager::{
    create_venv, install_requirements, install_with_strategy, purge_venv, refresh_status,
    select_interpreter, status_for, CreateOutcome, CreateTool, PythonEnvState,
    PythonInterpreterTool,
};
