// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// CLI entrypoint — mirrors cc-typescript-src `entrypoints/cli.tsx`.
// Bootstraps the interactive terminal REPL, headless/SDK, remote, or
// background session.

use std::path::PathBuf;

use crate::config::live::LiveConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliOptions {

    pub prompt: Option<String>,

    pub resume: Option<String>,

    pub plan_mode: bool,

    pub model: Option<String>,

    pub cwd: Option<PathBuf>,

    pub output_format: OutputFormat,

    pub max_turns: Option<u32>,

    pub system_prompt_append: Option<String>,

    pub mcp_servers: Vec<String>,

    pub allowed_tools: Vec<String>,

    pub denied_tools: Vec<String>,

    pub verbose: bool,

    pub add_dirs: Vec<PathBuf>,

    pub remote: bool,

    pub background: bool,

    pub dump_system_prompt: bool,

    pub bare: bool,

    pub session_id: Option<String>,

    pub provider: Option<String>,

    pub temperature: Option<f64>,

    pub peripherals: Vec<String>,

    pub session_state_file: Option<PathBuf>,

    pub session_mode: bool,

    pub legacy_mode: bool,
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

            session_mode: std::env::var("SEN_SESSION_MODE")
                .map(|v| !v.eq_ignore_ascii_case("0") && !v.eq_ignore_ascii_case("false"))
                .unwrap_or(true),
            legacy_mode: std::env::var("SEN_LEGACY_MODE")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
        }
    }
}

pub struct CliEntrypoint;

impl CliEntrypoint {

    pub async fn run(options: CliOptions) -> anyhow::Result<()> {
        let config = crate::Config::load_or_init().await?;

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

        let svc_data_dir = config
            .config_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| cwd.join(".senweavercoding"));
        let _ = crate::services::init_services(crate::services::ServiceContainerConfig {
            data_dir: svc_data_dir,
            ..Default::default()
        });
        let _ = crate::event_bus::integration::init_global_bus();

        let multi_agent_rt = crate::agent::multi_agent_runtime::init_global_runtime();
        {
            use crate::agent::registry::{AgentCapability, AgentInfo};
            let mut primary = AgentInfo::new("primary", "Primary Agent", "coder");
            primary.capabilities.push(AgentCapability {
                name: "coding".into(),
                description: "Default single-agent session".into(),
                proficiency: 1.0,
            });
            primary.capabilities.push(AgentCapability {
                name: "general".into(),
                description: "General purpose assistant".into(),
                proficiency: 0.9,
            });
            let _ = multi_agent_rt.supervisor.register_agent(primary);
        }

        {
            use crate::agent::registry::{AgentCapability, AgentInfo};
            for (swarm_name, swarm_cfg) in &config.swarms {
                for agent_name in &swarm_cfg.agents {
                    let id = format!("{swarm_name}/{agent_name}");
                    let mut info = AgentInfo::new(&id, agent_name.as_str(), swarm_name.as_str());

                    info.capabilities.push(AgentCapability {
                        name: agent_name.clone(),
                        description: format!("Swarm member of '{swarm_name}'"),
                        proficiency: 0.9,
                    });
                    info.capabilities.push(AgentCapability {
                        name: "general".into(),
                        description: "General fallback capability".into(),
                        proficiency: 0.6,
                    });
                    let _ = multi_agent_rt.supervisor.register_agent(info);
                }
            }
        }

        {
            let metrics = crate::services::try_get_services().map(|s| s.agent_metrics.clone());

            let (_handle, _token) = crate::memory::gc::spawn_memory_gc_task(
                crate::config::domain::MemoryRuntimeExtras::default(),
                metrics,
            );
        }

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

        if options.background {
            return Self::run_background(options, &config, &cwd).await;
        }

        if !is_interactive(&options) {
            return Self::run_headless(options).await;
        }

        if options.remote {
            return Self::run_remote(options).await;
        }

        Self::run_interactive(options, config).await
    }

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
            pid_start_time: bg::capture_current_start_time(),
            argv0_hash: std::env::args().next().map(|a| {
                use sha2::{Digest, Sha256};
                let mut h = Sha256::new();
                h.update(a.as_bytes());
                hex::encode(&h.finalize()[..8])
            }),
        };

        bg::save_session(&config.workspace_dir, &session_info).await?;

        println!("session_id={session_id}");
        println!("cwd={}", cwd.display());
        println!("background_mode=true");

        Self::run_headless(options).await
    }

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

    async fn run_remote(options: CliOptions) -> anyhow::Result<()> {
        tracing::info!("Remote mode requested — connecting to bridge");

        Self::run_headless(options).await
    }

    async fn run_interactive(options: CliOptions, config: crate::Config) -> anyhow::Result<()> {

        if !options.legacy_mode && options.session_mode {
            return Self::run_interactive_session(options, config).await;
        }

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
            None,
        ))
        .await
        .map(|_| ())
    }

    async fn run_interactive_session(
        options: CliOptions,
        config: crate::Config,
    ) -> anyhow::Result<()> {
        use crate::agent::agent::Agent;
        use crate::entrypoints::session_driven::run_session_driven;
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let temperature = options.temperature.unwrap_or(config.default_temperature);
        let provider_name = options
            .provider
            .clone()
            .or_else(|| config.default_provider.clone())
            .unwrap_or_else(|| "openrouter".into());
        let model = options
            .model
            .clone()
            .or_else(|| config.default_model.clone())
            .unwrap_or_else(|| "claude-sonnet-4-20250514".into());

        let provider = crate::providers::create_provider_with_url(
            &provider_name,
            config.api_key.as_deref(),
            config.api_url.as_deref(),
        )?;

        let cwd = resolve_cwd(&options);
        let security_policy = std::sync::Arc::new(crate::security::SecurityPolicy::from_config(
            &config.autonomy,
            &cwd,
        ));
        let agent = Agent::builder()
            .provider(provider)
            .tools(crate::tools::default_tools(security_policy))
            .shared_config(LiveConfig::new(config.clone()))
            .cached_provider_config(
                provider_name.clone(),
                config.api_key.clone().unwrap_or_default(),
                config.api_url.clone().unwrap_or_default(),
            )
            .temperature(temperature)
            .model_name(model)
            .build()?;

        let agent = Arc::new(Mutex::new(agent));

        tracing::info!("Starting session-driven interactive REPL");
        run_session_driven(agent, "> ").await
    }
}

fn resolve_cwd(options: &CliOptions) -> PathBuf {
    options
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

fn is_interactive(options: &CliOptions) -> bool {
    if options.output_format != OutputFormat::Text {
        return false;
    }
    if options.prompt.is_some() && options.max_turns.is_some() {
        return false;
    }
    true
}
