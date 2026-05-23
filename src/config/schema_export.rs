// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::{Schema, SchemaGenerator, generate::SchemaSettings};

use super::schema::Config;

pub fn export_config_schema() -> Result<String, String> {
    let settings = SchemaSettings::draft2020_12();
    let mut generator = SchemaGenerator::new(settings);
    let schema: Schema = generator.root_schema_for::<Config>();
    serde_json::to_string_pretty(&schema).map_err(|e| format!("Failed to serialize schema: {e}"))
}

pub fn write_config_schema(path: &std::path::Path) -> Result<(), String> {
    let schema = export_config_schema()?;
    std::fs::write(path, schema).map_err(|e| format!("Failed to write schema: {e}"))
}
