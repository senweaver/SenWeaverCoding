// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Output style loader — mirrors claude-code-typescript-src`outputStyles/loadOutputStylesDir.ts`.

use std::path::Path;

use super::types::{OutputStyleDefinition, OutputStyleSource};
use crate::constants::output_styles::builtin_output_styles;

pub async fn load_output_styles(
    project_dir: &Path,
    user_home: Option<&Path>,
) -> Vec<OutputStyleDefinition> {
    let mut styles: Vec<OutputStyleDefinition> = builtin_output_styles()
        .into_iter()
        .map(|def| OutputStyleDefinition {
            name: def.name,
            description: def.description,
            source: OutputStyleSource::Builtin,
            system_prompt_addition: def.system_prompt_addition,
            file_path: None,
        })
        .collect();

    let project_styles_dir = project_dir.join(".senweavercoding").join("output-styles");
    if project_styles_dir.is_dir() {
        if let Ok(entries) =
            load_styles_from_dir(&project_styles_dir, OutputStyleSource::Project).await
        {
            styles.extend(entries);
        }
    }

    if let Some(home) = user_home {
        let user_styles_dir = home.join(".senweavercoding").join("output-styles");
        if user_styles_dir.is_dir() {
            if let Ok(entries) =
                load_styles_from_dir(&user_styles_dir, OutputStyleSource::User).await
            {
                styles.extend(entries);
            }
        }
    }

    styles
}

async fn load_styles_from_dir(
    dir: &Path,
    source: OutputStyleSource,
) -> anyhow::Result<Vec<OutputStyleDefinition>> {
    let mut styles = Vec::new();
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                styles.push(OutputStyleDefinition {
                    name: name.clone(),
                    description: format!("Custom style: {name}"),
                    source,
                    system_prompt_addition: content,
                    file_path: Some(path.display().to_string()),
                });
            }
        }
    }
    Ok(styles)
}
