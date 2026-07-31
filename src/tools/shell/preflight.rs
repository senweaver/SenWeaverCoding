// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::security::SecurityPolicy;
use crate::tools::traits::ToolResult;

pub struct ShellPreflight {
    pub resource_guard: Option<crate::session::resource_lock::ResourceGuard>,
    pub workspace_guard: Option<crate::session::resource_lock::ResourceGuard>,
    pub write_guards: Option<Vec<crate::session::resource_lock::ResourceGuard>>,
}

impl ShellPreflight {
    pub fn guarded_write_paths(&self) -> Vec<std::path::PathBuf> {
        let Some(guards) = &self.write_guards else {
            return Vec::new();
        };
        guards
            .iter()
            .filter_map(|guard| match guard.kind() {
                crate::session::resource_lock::ResourceKind::FileWrite { path } => {
                    Some(path.clone())
                }
                _ => None,
            })
            .collect()
    }

    pub fn record_guarded_writes(&self) {
        for path in self.guarded_write_paths() {
            if path.is_file() {
                crate::session::record_write_for_current_session(&path);
            }
        }
    }
}

fn deny(error: String) -> ToolResult {
    ToolResult {
        success: false,
        output: String::new(),
        error: Some(error),
    }
}

pub(crate) async fn acquire_shell_execution_clearance(
    security: &SecurityPolicy,
    command: &str,
) -> Result<ShellPreflight, ToolResult> {
    let approved = crate::agent::loop_::current_tool_runtime_approved();

    if security.is_rate_limited() {
        return Err(deny(
            "Rate limit exceeded: too many actions in the last hour".into(),
        ));
    }

    let risk_level = match security.validate_command_execution(command, approved) {
        Ok(risk) => risk,
        Err(reason) => {
            crate::security::record_command_execution(
                "agent", command, "denied", approved, false, false, 0,
            );
            return Err(deny(reason));
        }
    };

    if let Some(path) = security.forbidden_path_argument(command) {
        return Err(deny(format!("Path blocked by security policy: {path}")));
    }

    if let Some(reason) = super::core::validate_shell_write_targets(security, command) {
        return Err(deny(reason));
    }

    if !security.record_action() {
        return Err(deny(
            "Rate limit exceeded: action budget exhausted".into(),
        ));
    }

    crate::security::record_command_execution(
        "agent",
        command,
        risk_level.as_str(),
        approved,
        true,
        true,
        0,
    );

    let resource_guard = match crate::session::acquire_shell_for_current_session().await {
        Some(Ok(g)) => Some(g),
        Some(Err(e)) => return Err(deny(format!("{e}"))),
        None => None,
    };

    let workspace_guard = if super::core::workspace_build_lock_enabled()
        && super::core::command_is_build_like(command)
    {
        match crate::session::acquire_workspace_exclusive_for_current_session().await {
            Some(Ok(g)) => Some(g),
            Some(Err(e)) => return Err(deny(format!("{e}"))),
            None => None,
        }
    } else {
        None
    };

    let write_guards = {
        let ws = security.workspace_dir();
        let mut paths: Vec<std::path::PathBuf> = Vec::new();
        for target in super::core::extract_shell_write_targets(command) {
            let p = if std::path::Path::new(&target).is_absolute() {
                std::path::PathBuf::from(&target)
            } else {
                ws.join(&target)
            };
            paths.push(p);
        }
        if paths.is_empty() {
            None
        } else {
            match crate::session::acquire_many_file_write_guards(paths).await {
                Ok(guards) => guards,
                Err(e) => return Err(deny(format!("{e}"))),
            }
        }
    };

    Ok(ShellPreflight {
        resource_guard,
        workspace_guard,
        write_guards,
    })
}
