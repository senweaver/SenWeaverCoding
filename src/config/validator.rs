// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;

use crate::config::schema::Config;

#[derive(Debug, Default, Clone)]
pub struct SectionReport {
    pub section: &'static str,
    pub errors: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct ValidationReport {
    pub sections: Vec<SectionReport>,
}

impl ValidationReport {
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.sections.iter().all(|s| s.errors.is_empty())
    }

    #[must_use]
    pub fn total_errors(&self) -> usize {
        self.sections.iter().map(|s| s.errors.len()).sum()
    }

    pub fn log_summary(&self) {
        for section in &self.sections {
            if section.errors.is_empty() {
                tracing::debug!(target: "config.validate", section = section.section, status = "ok");
            } else {
                for err in &section.errors {
                    tracing::warn!(
                        target: "config.validate",
                        section = section.section,
                        message = %err,
                        "config validation issue"
                    );
                }
            }
        }
    }

    pub fn into_result(self) -> Result<()> {
        if self.is_ok() {
            return Ok(());
        }
        let mut buf = String::new();
        for section in &self.sections {
            for err in &section.errors {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str("  - [");
                buf.push_str(section.section);
                buf.push_str("] ");
                buf.push_str(err);
            }
        }
        anyhow::bail!("configuration validation failed:\n{buf}")
    }
}

pub struct ConfigValidator<'a> {
    config: &'a Config,
}

impl<'a> ConfigValidator<'a> {
    #[must_use]
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    pub fn run(&self) -> ValidationReport {
        let mut report = ValidationReport::default();

        let mut proxy_errors = Vec::new();
        if let Err(err) = self.config.proxy.validate() {
            proxy_errors.push(err.to_string());
        }
        report.sections.push(SectionReport {
            section: "proxy",
            errors: proxy_errors,
        });

        report.sections.push(SectionReport {
            section: "memory",
            errors: self.config.memory.validate(),
        });

        report.sections.push(SectionReport {
            section: "memory_runtime",
            errors: self.config.memory_runtime.validate(),
        });

        let mut embedding_errors = Vec::new();
        for (i, route) in self.config.embedding_routes.iter().enumerate() {
            if route.hint.trim().is_empty() {
                embedding_errors.push(format!("embedding_routes[{i}].hint must not be empty"));
            }
            if route.provider.trim().is_empty() {
                embedding_errors.push(format!("embedding_routes[{i}].provider must not be empty"));
            }
            if route.model.trim().is_empty() {
                embedding_errors.push(format!("embedding_routes[{i}].model must not be empty"));
            }
        }
        report.sections.push(SectionReport {
            section: "embedding_routes",
            errors: embedding_errors,
        });

        let mut code_rag_errors = Vec::new();
        let cr = &self.config.code_rag;
        if cr.top_k == 0 {
            code_rag_errors.push("code_rag.top_k must be greater than 0".to_string());
        }
        if cr.dense_enabled {
            let backend = cr.embedder.backend.trim().to_ascii_lowercase();
            let backend = backend.as_str();
            let valid_backends = [
                "ollama",
                "openai",
                "gemini",
                "openai_compatible",
                "openai-compatible",
                "compatible",
                "local_bge",
                "localbge",
                "bge",
            ];
            if !valid_backends.contains(&backend) {
                code_rag_errors.push(format!(
                    "code_rag.embedder.backend '{}' is not one of {:?}",
                    backend, valid_backends
                ));
            }
            if matches!(backend, "openai_compatible" | "openai-compatible" | "compatible")
                && cr
                    .embedder
                    .endpoint
                    .as_deref()
                    .map(str::trim)
                    .filter(|e| !e.is_empty())
                    .is_none()
            {
                code_rag_errors.push(
                    "code_rag.embedder.endpoint is required when backend is 'openai_compatible'"
                        .to_string(),
                );
            }
            if cr.embedder.model.trim().is_empty() {
                code_rag_errors.push("code_rag.embedder.model must not be empty".to_string());
            }
            if cr.embedder.dims == 0 {
                code_rag_errors.push("code_rag.embedder.dims must be greater than 0".to_string());
            }
            if let Some(endpoint) = cr.embedder.endpoint.as_deref() {
                let trimmed = endpoint.trim();
                if !trimmed.is_empty() {
                    match reqwest::Url::parse(trimmed) {
                        Ok(parsed) => {
                            if !matches!(parsed.scheme(), "http" | "https") {
                                code_rag_errors.push(format!(
                                    "code_rag.embedder.endpoint scheme '{}' must be http or https",
                                    parsed.scheme()
                                ));
                            }
                        }
                        Err(err) => code_rag_errors.push(format!(
                            "code_rag.embedder.endpoint '{trimmed}' failed to parse: {err}"
                        )),
                    }
                }
            }
        }
        report.sections.push(SectionReport {
            section: "code_rag",
            errors: code_rag_errors,
        });

        report
    }
}
