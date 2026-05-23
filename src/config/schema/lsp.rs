// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LspConfig {

    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_lsp_servers")]
    pub servers: Vec<LspServerEntry>,

    #[serde(default = "default_true")]
    pub inlay_hints_enabled: bool,

    #[serde(default)]
    pub format_on_save: bool,

    #[serde(default = "default_lsp_hover_delay_ms")]
    pub hover_delay_ms: u32,
}

fn default_lsp_hover_delay_ms() -> u32 {
    250
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            servers: default_lsp_servers(),
            inlay_hints_enabled: true,
            format_on_save: false,
            hover_delay_ms: 250,
        }
    }
}

fn default_lsp_servers() -> Vec<LspServerEntry> {
    vec![
        LspServerEntry::template_rust_analyzer(),
        LspServerEntry::template_typescript_language_server(),
        LspServerEntry::template_pyright(),
        LspServerEntry::template_gopls(),
        LspServerEntry::template_clangd(),
        LspServerEntry::template_bash_language_server(),
        LspServerEntry::template_yaml_language_server(),
        LspServerEntry::template_vscode_html_language_server(),
        LspServerEntry::template_vscode_css_language_server(),
        LspServerEntry::template_vscode_json_language_server(),
        LspServerEntry::template_lua_language_server(),
        LspServerEntry::template_jdtls(),
        LspServerEntry::template_omnisharp(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LspServerEntry {

    pub id: String,

    pub language_id: String,

    #[serde(default)]
    pub display_name: String,

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub managed: bool,

    #[serde(default)]
    pub command: Option<String>,

    #[serde(default)]
    pub args: Vec<String>,

    #[serde(default)]
    pub env: HashMap<String, String>,

    #[serde(default)]
    pub file_extensions: Vec<String>,

    #[serde(default)]
    pub initialization_options: Option<serde_json::Value>,

    #[serde(default)]
    pub install_state: LspInstallState,
}

impl LspServerEntry {
    fn template_rust_analyzer() -> Self {
        Self {
            id: "rust-analyzer".to_string(),
            language_id: "rust".to_string(),
            display_name: "rust-analyzer".to_string(),
            enabled: false,
            managed: true,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            file_extensions: vec!["rs".to_string()],
            initialization_options: None,
            install_state: LspInstallState::default(),
        }
    }

    fn template_typescript_language_server() -> Self {
        Self {
            id: "typescript-language-server".to_string(),
            language_id: "typescript".to_string(),
            display_name: "typescript-language-server".to_string(),
            enabled: false,
            managed: true,
            command: None,
            args: vec!["--stdio".to_string()],
            env: HashMap::new(),
            file_extensions: vec![
                "ts".to_string(),
                "tsx".to_string(),
                "js".to_string(),
                "jsx".to_string(),
                "mjs".to_string(),
                "cjs".to_string(),
            ],
            initialization_options: None,
            install_state: LspInstallState::default(),
        }
    }

    fn template_pyright() -> Self {
        Self {
            id: "pyright".to_string(),
            language_id: "python".to_string(),
            display_name: "Pyright".to_string(),
            enabled: false,
            managed: true,
            command: None,
            args: vec!["--stdio".to_string()],
            env: HashMap::new(),
            file_extensions: vec!["py".to_string(), "pyi".to_string()],
            initialization_options: None,
            install_state: LspInstallState::default(),
        }
    }

    fn template_gopls() -> Self {
        Self {
            id: "gopls".to_string(),
            language_id: "go".to_string(),
            display_name: "gopls".to_string(),
            enabled: false,
            managed: true,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            file_extensions: vec!["go".to_string()],
            initialization_options: None,
            install_state: LspInstallState::default(),
        }
    }

    fn template_clangd() -> Self {
        Self {
            id: "clangd".to_string(),
            language_id: "cpp".to_string(),
            display_name: "clangd".to_string(),
            enabled: false,
            managed: true,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            file_extensions: vec![
                "c".to_string(),
                "h".to_string(),
                "cc".to_string(),
                "cpp".to_string(),
                "cxx".to_string(),
                "hpp".to_string(),
                "hh".to_string(),
                "hxx".to_string(),
            ],
            initialization_options: None,
            install_state: LspInstallState::default(),
        }
    }

    fn template_bash_language_server() -> Self {
        Self {
            id: "bash-language-server".to_string(),
            language_id: "shell".to_string(),
            display_name: "bash-language-server".to_string(),
            enabled: false,
            managed: true,
            command: None,
            args: vec!["start".to_string()],
            env: HashMap::new(),
            file_extensions: vec!["sh".to_string(), "bash".to_string(), "zsh".to_string()],
            initialization_options: None,
            install_state: LspInstallState::default(),
        }
    }

    fn template_yaml_language_server() -> Self {
        Self {
            id: "yaml-language-server".to_string(),
            language_id: "yaml".to_string(),
            display_name: "yaml-language-server".to_string(),
            enabled: false,
            managed: true,
            command: None,
            args: vec!["--stdio".to_string()],
            env: HashMap::new(),
            file_extensions: vec!["yaml".to_string(), "yml".to_string()],
            initialization_options: None,
            install_state: LspInstallState::default(),
        }
    }

    fn template_vscode_html_language_server() -> Self {
        Self {
            id: "vscode-html-language-server".to_string(),
            language_id: "html".to_string(),
            display_name: "vscode-html-language-server".to_string(),
            enabled: false,
            managed: true,
            command: None,
            args: vec!["--stdio".to_string()],
            env: HashMap::new(),
            file_extensions: vec!["html".to_string(), "htm".to_string()],
            initialization_options: None,
            install_state: LspInstallState::default(),
        }
    }

    fn template_vscode_css_language_server() -> Self {
        Self {
            id: "vscode-css-language-server".to_string(),
            language_id: "css".to_string(),
            display_name: "vscode-css-language-server".to_string(),
            enabled: false,
            managed: true,
            command: None,
            args: vec!["--stdio".to_string()],
            env: HashMap::new(),
            file_extensions: vec![
                "css".to_string(),
                "scss".to_string(),
                "less".to_string(),
            ],
            initialization_options: None,
            install_state: LspInstallState::default(),
        }
    }

    fn template_vscode_json_language_server() -> Self {
        Self {
            id: "vscode-json-language-server".to_string(),
            language_id: "json".to_string(),
            display_name: "vscode-json-language-server".to_string(),
            enabled: false,
            managed: true,
            command: None,
            args: vec!["--stdio".to_string()],
            env: HashMap::new(),
            file_extensions: vec!["json".to_string(), "jsonc".to_string()],
            initialization_options: None,
            install_state: LspInstallState::default(),
        }
    }

    fn template_lua_language_server() -> Self {
        Self {
            id: "lua-language-server".to_string(),
            language_id: "lua".to_string(),
            display_name: "lua-language-server".to_string(),
            enabled: false,
            managed: false,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            file_extensions: vec!["lua".to_string()],
            initialization_options: None,
            install_state: LspInstallState::default(),
        }
    }

    fn template_jdtls() -> Self {
        Self {
            id: "jdtls".to_string(),
            language_id: "java".to_string(),
            display_name: "Eclipse JDT Language Server".to_string(),
            enabled: false,
            managed: false,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            file_extensions: vec!["java".to_string()],
            initialization_options: None,
            install_state: LspInstallState::default(),
        }
    }

    fn template_omnisharp() -> Self {
        Self {
            id: "omnisharp".to_string(),
            language_id: "csharp".to_string(),
            display_name: "OmniSharp".to_string(),
            enabled: false,
            managed: false,
            command: None,
            args: vec!["-lsp".to_string()],
            env: HashMap::new(),
            file_extensions: vec!["cs".to_string()],
            initialization_options: None,
            install_state: LspInstallState::default(),
        }
    }

    pub fn resolved_command(&self) -> Option<&str> {
        match self.command.as_deref() {
            Some(s) if !s.trim().is_empty() => Some(s),
            _ => None,
        }
    }
}

impl Default for LspServerEntry {
    fn default() -> Self {
        Self {
            id: String::new(),
            language_id: String::new(),
            display_name: String::new(),
            enabled: false,
            managed: false,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            file_extensions: Vec::new(),
            initialization_options: None,
            install_state: LspInstallState::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LspInstallState {

    NotInstalled,

    Installing,

    Installed { version: String, path: String },

    Failed { reason: String },
}

impl Default for LspInstallState {
    fn default() -> Self {
        Self::NotInstalled
    }
}
