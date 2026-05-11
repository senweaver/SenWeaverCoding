// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Init entrypoint — mirrors claude-code-typescript-src`entrypoints/init.ts`.
// Project initialization: config scaffolding, AGENTS.md creation,
// trust acceptance, and migration detection.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitResult {
    pub config_created: bool,
    pub agents_md_created: bool,
    pub trust_accepted: bool,
    pub migrations_applied: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct InitEntrypoint;

impl InitEntrypoint {

    pub async fn run(project_dir: &Path, _interactive: bool) -> anyhow::Result<InitResult> {
        let mut result = InitResult {
            config_created: false,
            agents_md_created: false,
            trust_accepted: false,
            migrations_applied: Vec::new(),
            warnings: Vec::new(),
        };

        let config_dir = project_dir.join(".senweavercoding");
        if !config_dir.exists() {
            tokio::fs::create_dir_all(&config_dir).await?;
            result.config_created = true;
            tracing::info!(dir = %config_dir.display(), "Created config directory");
        }

        let agents_md = project_dir.join("AGENTS.md");
        if !agents_md.exists() {
            let template = Self::default_agents_md(project_dir);
            tokio::fs::write(&agents_md, template).await?;
            result.agents_md_created = true;
            tracing::info!("Created AGENTS.md");
        }

        let config_file = config_dir.join("config.toml");
        if !config_file.exists() {
            let default_config = Self::default_config();
            tokio::fs::write(&config_file, default_config).await?;
            tracing::info!("Created default config.toml");
        }

        let skills_dir = config_dir.join("skills");
        if !skills_dir.exists() {
            tokio::fs::create_dir_all(&skills_dir).await?;
        }

        let memory_dir = config_dir.join("memory");
        if !memory_dir.exists() {
            tokio::fs::create_dir_all(&memory_dir).await?;
        }

        result.migrations_applied = Self::detect_migrations(&config_dir).await;

        Ok(result)
    }

    fn default_agents_md(project_dir: &Path) -> String {
        let project_name = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project");
        format!(
            "# AGENTS.md — {project_name}\n\n\
             Cross-tool agent instructions for this project.\n\n\
             ## Project Overview\n\n\
             <!-- Describe your project here -->\n\n\
             ## Commands\n\n\
             ```bash\n\
             # Add your common commands here\n\
             ```\n\n\
             ## Guidelines\n\n\
             - Follow existing code style and conventions\n\
             - Write tests for new functionality\n\
             - Keep changes focused and minimal\n"
        )
    }

    fn default_config() -> String {
        "# SenWeaverCoding configuration\n\
         # See docs for full reference.\n\n\
         [agent]\n\
         # model = \"<your-model-id>\"\n\n\
         [memory]\n\
         backend = \"markdown\"\n\n\
         [gateway]\n\
         # host = \"127.0.0.1\"\n\
         # port = 3777\n"
            .to_string()
    }

    async fn detect_migrations(config_dir: &Path) -> Vec<String> {
        let mut applied = Vec::new();

        let legacy_path = config_dir.join("config.json");
        if legacy_path.exists() {
            tracing::info!("Detected legacy config.json — migration available");
            applied.push("legacy_config_detected".to_string());
        }
        applied
    }
}
