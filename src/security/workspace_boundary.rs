// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::config::workspace::WorkspaceProfile;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryVerdict {

    Allow,

    Deny(String),
}

#[derive(Debug, Clone)]
pub struct WorkspaceBoundary {

    profile: Option<WorkspaceProfile>,

    cross_workspace_search: bool,
}

impl WorkspaceBoundary {

    pub fn new(profile: Option<WorkspaceProfile>, cross_workspace_search: bool) -> Self {
        Self {
            profile,
            cross_workspace_search,
        }
    }

    pub fn inactive() -> Self {
        Self {
            profile: None,
            cross_workspace_search: false,
        }
    }

    pub fn check_tool_access(&self, tool_name: &str) -> BoundaryVerdict {
        if let Some(profile) = &self.profile {
            if profile.is_tool_restricted(tool_name) {
                return BoundaryVerdict::Deny(format!(
                    "tool '{}' is restricted in workspace '{}'",
                    tool_name, profile.name
                ));
            }
        }
        BoundaryVerdict::Allow
    }

    pub fn check_domain_access(&self, domain: &str) -> BoundaryVerdict {
        if let Some(profile) = &self.profile {
            if !profile.is_domain_allowed(domain) {
                return BoundaryVerdict::Deny(format!(
                    "domain '{}' is not in the allowlist for workspace '{}'",
                    domain, profile.name
                ));
            }
        }
        BoundaryVerdict::Allow
    }

    pub fn check_path_access(&self, path: &Path, workspaces_base: &Path) -> BoundaryVerdict {
        let profile = match &self.profile {
            Some(p) => p,
            None => return BoundaryVerdict::Allow,
        };

        if let Ok(relative) = path.strip_prefix(workspaces_base) {
            let first_component = relative
                .components()
                .next()
                .and_then(|c| c.as_os_str().to_str());

            if let Some(ws_name) = first_component {
                if ws_name != profile.name {
                    if self.cross_workspace_search {

                        return BoundaryVerdict::Allow;
                    }
                    return BoundaryVerdict::Deny(format!(
                        "access to workspace '{}' is denied from workspace '{}'",
                        ws_name, profile.name
                    ));
                }
            }
        }

        BoundaryVerdict::Allow
    }

    pub fn is_active(&self) -> bool {
        self.profile.is_some()
    }

    pub fn active_workspace_name(&self) -> Option<&str> {
        self.profile.as_ref().map(|p| p.name.as_str())
    }
}
