// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// File constants — mirrors claude-code-typescript-src`constants/files.ts`.

/// Config directory name.
pub const CONFIG_DIR_NAME: &str = ".senweavercoding";

/// Agents instruction file name.
pub const AGENTS_MD: &str = "AGENTS.md";

/// Claude-compatible instruction file name.
pub const CLAUDE_MD: &str = "CLAUDE.md";

/// Skills directory within config.
pub const SKILLS_DIR: &str = "skills";

/// Memory directory within config.
pub const MEMORY_DIR: &str = "memory";

/// Session storage directory.
pub const SESSIONS_DIR: &str = "sessions";

/// Plugins directory.
pub const PLUGINS_DIR: &str = "plugins";

/// Scheduled tasks file.
pub const SCHEDULED_TASKS_FILE: &str = "scheduled_tasks.json";

/// Output styles directory.
pub const OUTPUT_STYLES_DIR: &str = "output-styles";

/// Settings file.
pub const SETTINGS_FILE: &str = "settings.json";

/// Trusted devices file for bridge.
pub const TRUSTED_DEVICES_FILE: &str = "trusted_devices.json";

/// Max file size for reading (10 MB).
pub const MAX_FILE_READ_BYTES: usize = 10 * 1024 * 1024;

/// Max line length before truncation (characters).
pub const MAX_LINE_LENGTH: usize = 2000;

/// Binary file extensions to skip.
pub const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "svg", "mp3", "mp4", "avi", "mov", "wav",
    "flac", "zip", "tar", "gz", "bz2", "xz", "7z", "rar", "exe", "dll", "so", "dylib", "o", "a",
    "wasm", "pyc", "pyo", "class", "pdf", "doc", "docx", "xls", "xlsx", "ttf", "otf", "woff",
    "woff2", "eot", "sqlite", "db",
];

/// Check if a file extension is binary.
pub fn is_binary_extension(ext: &str) -> bool {
    BINARY_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}
