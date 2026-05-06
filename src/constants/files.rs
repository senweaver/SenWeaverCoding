// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// File constants — mirrors claude-code-typescript-src`constants/files.ts`.

pub const CONFIG_DIR_NAME: &str = ".senweavercoding";

pub const AGENTS_MD: &str = "AGENTS.md";

pub const CLAUDE_MD: &str = "CLAUDE.md";

pub const SKILLS_DIR: &str = "skills";

pub const MEMORY_DIR: &str = "memory";

pub const SESSIONS_DIR: &str = "sessions";

pub const PLUGINS_DIR: &str = "plugins";

pub const SCHEDULED_TASKS_FILE: &str = "scheduled_tasks.json";

pub const OUTPUT_STYLES_DIR: &str = "output-styles";

pub const SETTINGS_FILE: &str = "settings.json";

pub const TRUSTED_DEVICES_FILE: &str = "trusted_devices.json";

pub const MAX_FILE_READ_BYTES: usize = 10 * 1024 * 1024;

pub const MAX_LINE_LENGTH: usize = 2000;

pub const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "svg", "mp3", "mp4", "avi", "mov", "wav",
    "flac", "zip", "tar", "gz", "bz2", "xz", "7z", "rar", "exe", "dll", "so", "dylib", "o", "a",
    "wasm", "pyc", "pyo", "class", "pdf", "doc", "docx", "xls", "xlsx", "ttf", "otf", "woff",
    "woff2", "eot", "sqlite", "db",
];

pub fn is_binary_extension(ext: &str) -> bool {
    BINARY_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}
