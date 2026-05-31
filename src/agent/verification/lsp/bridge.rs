// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::diag::{LspDiagnostic, LspDiagnosticFetcher};
use super::super::traits::IssueSeverity;
use crate::services::lsp::{
    DiagnosticSeverity, LspDiagnostic as ServiceDiagnostic, LspService,
};

pub const DEFAULT_FETCH_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub struct LspPoolDiagnosticFetcher {
    lsp: Arc<LspService>,
    workspace_root: PathBuf,
    timeout: Duration,
}

impl LspPoolDiagnosticFetcher {
    pub fn new(lsp: Arc<LspService>, workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            lsp,
            workspace_root: workspace_root.into(),
            timeout: DEFAULT_FETCH_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

#[async_trait]
impl LspDiagnosticFetcher for LspPoolDiagnosticFetcher {
    async fn fetch(&self, path: &Path) -> anyhow::Result<Vec<LspDiagnostic>> {

        if path == self.workspace_root.as_path() {
            return self.fetch_workspace().await;
        }
        let lang = infer_lsp_language_id(path).unwrap_or("plaintext");
        let fut = self
            .lsp
            .refresh_diagnostics(path, lang, &self.workspace_root);
        match tokio::time::timeout(self.timeout, fut).await {
            Ok(Ok(diags)) => Ok(diags.into_iter().map(map_diagnostic).collect()),

            Ok(Err(e)) => Err(e),

            Err(_) => Ok(Vec::new()),
        }
    }
}

impl LspPoolDiagnosticFetcher {

    pub async fn fetch_workspace(&self) -> anyhow::Result<Vec<LspDiagnostic>> {
        let tracked: Vec<PathBuf> = self
            .lsp
            .get_all_diagnostics()
            .await
            .into_keys()
            .collect();
        if tracked.is_empty() {
            return Ok(Vec::new());
        }
        let mut out: Vec<LspDiagnostic> = Vec::new();
        for path in tracked {
            let lang = infer_lsp_language_id(&path).unwrap_or("plaintext");
            let fut = self
                .lsp
                .refresh_diagnostics(&path, lang, &self.workspace_root);
            match tokio::time::timeout(self.timeout, fut).await {
                Ok(Ok(diags)) => out.extend(diags.into_iter().map(map_diagnostic)),
                Ok(Err(e)) => {
                    tracing::warn!(
                        target: "agent.verification.lsp.bridge",
                        path = %path.display(),
                        error = %e,
                        "fetch_workspace refresh_diagnostics returned error",
                    );
                    continue;
                }
                Err(_) => {
                    tracing::warn!(
                        target: "agent.verification.lsp.bridge",
                        path = %path.display(),
                        "fetch_workspace timed out for one file; continuing",
                    );
                    continue;
                }
            }
        }
        Ok(out)
    }
}

fn map_diagnostic(d: ServiceDiagnostic) -> LspDiagnostic {
    LspDiagnostic {
        line: d.range.start_line + 1,
        column: d.range.start_character + 1,
        message: d.message,
        severity: match d.severity {
            DiagnosticSeverity::Error => IssueSeverity::Error,
            DiagnosticSeverity::Warning => IssueSeverity::Warning,
            DiagnosticSeverity::Information | DiagnosticSeverity::Hint => IssueSeverity::Info,
        },
    }
}

pub fn infer_lsp_language_id(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "rs" => "rust",
        "py" | "pyi" => "python",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cxx" | "cc" | "hpp" | "hh" | "hxx" => "cpp",
        "json" => "json",
        "toml" => "toml",
        "md" | "markdown" => "markdown",
        _ => return None,
    })
}
