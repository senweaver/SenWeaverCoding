// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// CLI entrypoint — mirrors cc-typescript-src `entrypoints/cli.tsx`.
// Bootstraps the interactive terminal REPL, headless/SDK, remote, or
// background session.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// CLI launch options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliOptions {
    /// Initial prompt (non-interactive single-shot mode).
    pub prompt: Option<String>,
    /// Resume a previous session.
    pub resume: Option<String>,
    /// Enable plan mode from the start.
    pub plan_mode: bool,
    /// Model override.
    pub model: Option<String>,
    /// Working directory override.
    pub cwd: Option<PathBuf>,
    /// Output format (text, json, stream-json).
    pub output_format: OutputFormat,
    /// Maximum turns for non-interactive mode.
    pub max_turns: Option<u32>,
    /// System prompt override/append.
    pub system_prompt_append: Option<String>,
    /// MCP server configs to load.
    pub mcp_servers: Vec<String>,
    /// Tool allow-list (empty = all).
    pub allowed_tools: Vec<String>,
    /// Tool deny-list.
    pub denied_tools: Vec<String>,
    /// Enable verbose logging.
    pub verbose: bool,
    /// Additional directories to load CLAUDE.md / AGENTS.md from.
    pub add_dirs: Vec<PathBuf>,
    /// Enable remote bridge mode.
    pub remote: bool,
    /// Run in background mode (no attached terminal; non-interactive session).
    pub background: bool,
    /// Dump the effective system prompt and exit.
    pub dump_system_prompt: bool,
    /// Minimal/simple mode with a reduced tool surface (see `SEN_CLI_BARE`).
    pub bare: bool,
    /// Explicit session ID (overrides the default generated id).
    pub session_id: Option<String>,
    /// Provider override for the agent.
    pub provider: Option<String>,
    /// Temperature override.
    pub temperature: Option<f64>,
    /// Peripherals to attach.
    pub peripherals: Vec<String>,
    /// Session state file for persistence.
    pub session_state_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Text,
    Json,
    StreamJson,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            prompt: None,
            resume: None,
            plan_mode: false,
            model: None,
            cwd: None,
            output_format: OutputFormat::Text,
            max_turns: None,
            system_prompt_append: None,
            mcp_servers: Vec::new(),
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
            verbose: false,
            add_dirs: Vec::new(),
            remote: false,
            background: false,
            dump_system_prompt: false,
            bare: false,
            session_id: None,
            provider: None,
            temperature: None,
            peripherals: Vec::new(),
            session_state_file: None,
        }
    }
}

/// CLI entrypoint — bootstraps and runs the interactive agent session.
pub struct CliEntrypoint;

impl CliEntrypoint {
    /// Run the CLI entrypoint with the given options.
    /// This is the main integration point called from `main.rs`.
    ///
    /// Dispatch logic:
    /// 1. `--dump-system-prompt` → print and exit
    /// 2. `output_format` is Json/StreamJson OR `prompt` is set → headless/SDK
    /// 3. `--background` → spawn detached session
    /// 4. `--remote` → remote bridge mode via RemoteIO
    /// 5. Default → interactive REPL via `agent::run`
    pub async fn run(options: CliOptions) -> anyhow::Result<()> {
        let config = crate::Config::load_or_init().await?;
        // ── Fast path: dump system prompt ────────────────────────
        if options.dump_system_prompt {
            let cwd = resolve_cwd(&options);
            let files = crate::memdir::discover_memory_files(&cwd)
                .await
                .unwrap_or_default();
            let prompt = crate::memdir::build_memory_prompt(&files);
            println!("{prompt}");
            return Ok(());
        }

        let cwd = resolve_cwd(&options);

        if options.bare {
            crate::util::set_env_var("SEN_CLI_BARE", "1");
        }

        crate::bootstrap::init_state(cwd.clone());

        crate::bootstrap::get_state().write(|state| {
            if let Some(ref sid) = options.session_id {
                state.session_id = crate::bootstrap::state::SessionId(sid.clone());
            }
            state.is_remote_mode = options.remote;
            if options.background || !is_interactive(&options) {
                state.is_interactive = false;
            }
        });

        tracing::info!(
            cwd = %cwd.display(),
            prompt = ?options.prompt,
            resume = ?options.resume,
            plan_mode = options.plan_mode,
            model = ?options.model,
            output_format = ?options.output_format,
            max_turns = ?options.max_turns,
            mcp_server_count = options.mcp_servers.len(),
            allowed_tool_count = options.allowed_tools.len(),
            denied_tool_count = options.denied_tools.len(),
            remote = options.remote,
            background = options.background,
            bare = options.bare,
            "CLI launch configuration"
        );

        // ── Background session ───────────────────────────────────
        if options.background {
            return Self::run_background(options, &config, &cwd).await;
        }

        // ── Headless / SDK mode ──────────────────────────────────
        if !is_interactive(&options) {
            return Self::run_headless(options).await;
        }

        // ── Remote bridge ────────────────────────────────────────
        if options.remote {
            return Self::run_remote(options).await;
        }

        // ── Interactive REPL (default) ───────────────────────────
        Self::run_interactive(options, config).await
    }

    /// Spawn a detached background session.
    async fn run_background(
        options: CliOptions,
        config: &crate::Config,
        cwd: &std::path::Path,
    ) -> anyhow::Result<()> {
        use crate::cli::bg;

        let session_id = options
            .session_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let session_info = bg::SessionInfo {
            id: session_id.clone(),
            pid: Some(std::process::id()),
            started_at: chrono::Utc::now().to_rfc3339(),
            status: bg::SessionStatus::Running,
            cwd: cwd.to_path_buf(),
            last_activity: chrono::Utc::now().to_rfc3339(),
        };

        bg::save_session(&config.workspace_dir, &session_info).await?;

        println!("session_id={session_id}");
        println!("cwd={}", cwd.display());
        println!("background_mode=true");

        Self::run_headless(options).await
    }

    /// Run in headless/SDK mode — no terminal UI, NDJSON I/O.
    async fn run_headless(options: CliOptions) -> anyhow::Result<()> {
        use crate::cli::headless;
        use crate::cli::structured_io::StructuredIO;

        let mut io = StructuredIO::from_stdin();

        let headless_config = headless::HeadlessConfig {
            session_id: options
                .session_id
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            initial_prompt: options.prompt.unwrap_or_default(),
            max_turns: options.max_turns,
            model: options.model,
            system_prompt_append: options.system_prompt_append,
            allowed_tools: options.allowed_tools,
            denied_tools: options.denied_tools,
            mcp_servers: options.mcp_servers,
            output_format: match options.output_format {
                OutputFormat::Json => headless::OutputFormat::Json,
                OutputFormat::StreamJson => headless::OutputFormat::StreamJson,
                OutputFormat::Text => headless::OutputFormat::Text,
            },
            plan_mode: options.plan_mode,
        };

        let result = headless::run_headless(headless_config, &mut io).await?;

        tracing::info!(
            session_id = %result.session_id,
            turns = result.num_turns,
            duration_ms = result.duration_ms,
            exit_reason = ?result.exit_reason,
            "Headless session completed"
        );

        Ok(())
    }

    /// Run in remote bridge mode — NDJSON over network transport.
    async fn run_remote(options: CliOptions) -> anyhow::Result<()> {
        tracing::info!("Remote mode requested — connecting to bridge");

        // Remote mode is wired via RemoteIO + transports.
        // For now, fall through to headless with stdin/stdout
        // (real transport negotiation requires bridge URL config).
        Self::run_headless(options).await
    }

    /// Run the interactive REPL.
    async fn run_interactive(options: CliOptions, config: crate::Config) -> anyhow::Result<()> {
        let temperature = options.temperature.unwrap_or(config.default_temperature);

        Box::pin(crate::agent::run(
            config,
            options.prompt,
            options.provider,
            options.model,
            temperature,
            options.peripherals,
            true,
            options.session_state_file,
            if options.allowed_tools.is_empty() {
                None
            } else {
                Some(options.allowed_tools)
            },
        ))
        .await
        .map(|_| ())
    }
}

/// Determine the effective working directory.
fn resolve_cwd(options: &CliOptions) -> PathBuf {
    options
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

/// Check if the session should run in interactive mode.
fn is_interactive(options: &CliOptions) -> bool {
    if options.output_format != OutputFormat::Text {
        return false;
    }
    if options.prompt.is_some() && options.max_turns.is_some() {
        return false;
    }
    true
}
