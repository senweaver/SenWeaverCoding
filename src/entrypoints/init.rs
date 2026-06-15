// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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

        result.migrations_applied =
            Self::run_migrations(&config_dir, &mut result.warnings).await;

        Ok(result)
    }

    fn default_agents_md(project_dir: &Path) -> String {
        let project_name = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project");
        format!(
            "# AGENTS.md  -  {project_name}\n\n\
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

    async fn run_migrations(config_dir: &Path, warnings: &mut Vec<String>) -> Vec<String> {
        let mut applied = Vec::new();

        let legacy_path = config_dir.join("config.json");
        if legacy_path.exists() {
            tracing::info!("Detected legacy config.json  -  migrating to config.toml");
            match Self::migrate_legacy_config(config_dir, &legacy_path).await {
                Ok(migrated_fields) => {
                    applied.push(format!(
                        "legacy_config_migrated:{migrated_fields}_fields"
                    ));
                    tracing::info!(
                        fields = migrated_fields,
                        "Legacy config.json migrated; source renamed to config.json.migrated"
                    );
                }
                Err(e) => {
                    warnings.push(format!(
                        "legacy config.json migration failed: {e}; the file was left in place"
                    ));
                    tracing::warn!(error = %e, "Legacy config.json migration failed");
                }
            }
        }
        applied
    }

    async fn migrate_legacy_config(
        config_dir: &Path,
        legacy_path: &Path,
    ) -> anyhow::Result<usize> {
        let raw = tokio::fs::read_to_string(legacy_path).await?;
        let legacy: serde_json::Value = serde_json::from_str(&raw)?;
        let legacy_obj = legacy
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("legacy config.json is not a JSON object"))?;

        let config_file = config_dir.join("config.toml");
        let existing_toml = tokio::fs::read_to_string(&config_file)
            .await
            .unwrap_or_default();
        let mut doc: toml::Value = toml::from_str(&existing_toml)
            .unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new()));
        let table = doc
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("config.toml root is not a table"))?;

        let pick_str = |keys: &[&str]| -> Option<String> {
            keys.iter().find_map(|k| {
                legacy_obj
                    .get(*k)
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            })
        };
        let pick_f64 = |keys: &[&str]| -> Option<f64> {
            keys.iter()
                .find_map(|k| legacy_obj.get(*k).and_then(serde_json::Value::as_f64))
        };

        let mut migrated = 0usize;
        if let Some(api_key) = pick_str(&["api_key", "apiKey"]) {
            table.insert("api_key".into(), toml::Value::String(api_key));
            migrated += 1;
        }
        if let Some(api_url) = pick_str(&["api_url", "apiUrl", "base_url", "baseUrl"]) {
            table.insert("api_url".into(), toml::Value::String(api_url));
            migrated += 1;
        }
        if let Some(model) = pick_str(&["default_model", "model", "defaultModel"]) {
            table.insert("default_model".into(), toml::Value::String(model));
            migrated += 1;
        }
        if let Some(provider) = pick_str(&["default_provider", "provider", "defaultProvider"]) {
            table.insert("default_provider".into(), toml::Value::String(provider));
            migrated += 1;
        }
        if let Some(temperature) =
            pick_f64(&["default_temperature", "temperature", "defaultTemperature"])
        {
            table.insert(
                "default_temperature".into(),
                toml::Value::Float(temperature),
            );
            migrated += 1;
        }

        if migrated > 0 {
            let serialized = toml::to_string_pretty(&doc)?;
            tokio::fs::write(&config_file, serialized).await?;
        }

        let migrated_path = legacy_path.with_extension("json.migrated");
        tokio::fs::rename(legacy_path, &migrated_path).await?;
        Ok(migrated)
    }
}
