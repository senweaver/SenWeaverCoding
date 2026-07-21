// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
        Self::run_with_config(options, config).await
    }

    pub async fn run_with_config(
        options: CliOptions,
        config: crate::Config,
    ) -> anyhow::Result<()> {
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
            crate::util::set_runtime_var("SEN_CLI_BARE", "1");
        }

        crate::bootstrap::init_state(cwd.clone());

        let svc_data_dir = config
            .config_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| cwd.join(".senweavercoding"));
        let _ = crate::services::init_services(crate::services::ServiceContainerConfig {
            data_dir: svc_data_dir,
            team_sync_enabled: config.teams.sync_enabled,
            ..Default::default()
        });
        let _ = crate::event_bus::integration::init_global_bus(
            config
                .config_path
                .parent()
                .map(|p| p.join("event_audit.jsonl")),
        );

        {
            crate::workers::init_global_supervisor(cwd.clone());
            crate::workers::scan_and_recover_at(&cwd);
        }

        // The headless/interactive CLI paths must install the workspace resource
        // manager too (the gateway installs its own). Without it the cross-session
        // write lock and stale-file detection are inert on the no-gateway path, so
        // concurrent agent sessions sharing this workspace would not serialize.
        crate::session::install_global_workspace_resources(
            crate::session::WorkspaceResourceManager::new(),
        );

        let multi_agent_rt = crate::agent::multi_agent_runtime::init_global_runtime();
        crate::agent::multi_agent_runtime::register_configured_agents(&multi_agent_rt, &config);

        {
            let metrics = crate::services::try_get_services().map(|s| s.agent_metrics.clone());

            let (_handle, _token) =
                crate::memory::gc::spawn_memory_gc_task(config.memory_runtime.clone(), metrics);
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
            return Self::run_remote(options, &config).await;
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

    async fn run_remote(options: CliOptions, config: &crate::Config) -> anyhow::Result<()> {
        use futures_util::{SinkExt, StreamExt};
        use std::io::Write as _;

        let host = if config.gateway.host == "0.0.0.0" {
            "127.0.0.1".to_string()
        } else {
            config.gateway.host.clone()
        };
        let port = config.gateway.port;
        let session_id = options
            .session_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let mut url = format!("ws://{host}:{port}/ws/chat?session_id={session_id}");
        if let Some(token) = config.gateway.paired_tokens.first() {
            url.push_str(&format!("&token={token}"));
        } else if config.gateway.require_pairing {
            anyhow::bail!(
                "Remote mode requires a gateway pairing token: gateway.require_pairing is enabled \
                 but [gateway] paired_tokens in config.toml is empty. Pair with the gateway first \
                 (or add a valid token to [gateway] / disable require_pairing), then retry."
            );
        }

        tracing::info!(url = %format!("ws://{host}:{port}/ws/chat"), "Remote mode: connecting to gateway");

        let connect_future = {
            use tokio_tungstenite::tungstenite::client::IntoClientRequest;
            let mut request = url
                .as_str()
                .into_client_request()
                .map_err(|e| anyhow::anyhow!("Remote mode: failed to build the connection request: {e}"))?;
            if let Some(secret) = config
                .gateway
                .signing_secret
                .as_deref()
                .filter(|s| !s.trim().is_empty())
            {
                use hmac::{Hmac, Mac};
                use sha2::Sha256;
                let ts = chrono::Utc::now().timestamp().to_string();
                if let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) {
                    mac.update(ts.as_bytes());
                    let sig = hex::encode(mac.finalize().into_bytes());
                    if let (Ok(ts_v), Ok(sig_v)) = (
                        tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&ts),
                        tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&sig),
                    ) {
                        request.headers_mut().insert("x-sen-timestamp", ts_v);
                        request.headers_mut().insert("x-sen-signature", sig_v);
                    }
                }
            }
            tokio_tungstenite::connect_async(request)
        };
        let (ws_stream, _) = connect_future.await.map_err(|e| {
            anyhow::anyhow!(
                "Remote mode: failed to connect to the gateway (ws://{host}:{port}/ws/chat): {e}\n\
                 Check that: 1) the gateway is running (`sen gateway`); 2) [gateway] host/port in \
                 config.toml are correct; 3) if require_pairing is enabled, paired_tokens contains \
                 a valid token."
            )
        })?;

        let (mut sink, mut stream) = ws_stream.split();

        let connect_msg = serde_json::json!({
            "type": "connect",
            "session_id": session_id,
            "device_name": "sen-cli-remote",
            "capabilities": ["text"],
        });
        sink.send(tokio_tungstenite::tungstenite::Message::Text(
            connect_msg.to_string().into(),
        ))
        .await?;

        async fn remote_turn<S, R>(
            sink: &mut S,
            stream: &mut R,
            content: &str,
        ) -> anyhow::Result<()>
        where
            S: futures_util::Sink<tokio_tungstenite::tungstenite::Message> + Unpin,
            S::Error: std::error::Error + Send + Sync + 'static,
            R: futures_util::Stream<
                    Item = Result<
                        tokio_tungstenite::tungstenite::Message,
                        tokio_tungstenite::tungstenite::Error,
                    >,
                > + Unpin,
        {
            use std::io::Write as _;
            let msg = serde_json::json!({ "type": "message", "content": content });
            sink.send(tokio_tungstenite::tungstenite::Message::Text(
                msg.to_string().into(),
            ))
            .await?;

            while let Some(incoming) = stream.next().await {
                match incoming? {
                    tokio_tungstenite::tungstenite::Message::Text(text) => {
                        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) else {
                            continue;
                        };
                        match parsed["type"].as_str().unwrap_or("") {
                            "chunk" => {
                                print!("{}", parsed["content"].as_str().unwrap_or(""));
                                let _ = std::io::stdout().flush();
                            }
                            "tool_call" => {
                                eprintln!(
                                    "\x1b[2m→ {}\x1b[0m",
                                    parsed["name"].as_str().unwrap_or("tool")
                                );
                            }
                            "error" => {
                                eprintln!(
                                    "\x1b[31mRemote error: {}\x1b[0m",
                                    parsed["content"]
                                        .as_str()
                                        .or_else(|| parsed["message"].as_str())
                                        .unwrap_or("unknown")
                                );
                            }
                            "done" => {
                                println!();
                                return Ok(());
                            }
                            _ => {}
                        }
                    }
                    tokio_tungstenite::tungstenite::Message::Close(_) => {
                        anyhow::bail!("Remote session was closed by the gateway");
                    }
                    _ => {}
                }
            }
            anyhow::bail!("Remote connection lost: the gateway disconnected before the turn finished")
        }

        if let Some(prompt) = options.prompt {
            remote_turn(&mut sink, &mut stream, &prompt).await?;
            return Ok(());
        }

        println!("Connected to remote gateway (session: {session_id}). Type /exit to quit.");
        loop {
            print!("\x1b[1;32mremote>\x1b[0m ");
            let _ = std::io::stdout().flush();
            let line = tokio::task::spawn_blocking(|| {
                crate::cli::input::read_stdin_line_lossy().ok().flatten()
            })
            .await
            .ok()
            .flatten();
            let Some(line) = line else { break };
            let input = line.trim();
            if input.is_empty() {
                continue;
            }
            if input == "/exit" || input == "/quit" {
                break;
            }
            remote_turn(&mut sink, &mut stream, input).await?;
        }

        let _ = sink
            .send(tokio_tungstenite::tungstenite::Message::Close(None))
            .await;
        Ok(())
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
        let model = match options.model.clone() {
            Some(m) if !m.trim().is_empty() => m,
            _ => crate::providers::resolve_default_model(&config)?,
        };

        let cwd = resolve_cwd(&options);

        let mut effective_config = config.clone();
        effective_config.default_provider = Some(provider_name);
        effective_config.default_model = Some(model);
        effective_config.default_temperature = temperature;
        effective_config.workspace_dir = cwd;

        let agent = Agent::from_config(
            &effective_config,
            None,
            Some(LiveConfig::new(config.clone())),
        )
        .await?;

        let agent = Arc::new(Mutex::new(agent));

        tracing::info!("Starting session-driven interactive REPL with full tool surface");
        run_session_driven(agent, "> ", options.prompt, options.resume).await
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
