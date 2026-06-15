// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
#![recursion_limit = "256"]
use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use dialoguer::Password;
use serde::{Deserialize, Serialize};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, fmt};

use senweavercoding::cli_entry::bootstrap as _bootstrap;

#[inline]
fn load_env() {
    _bootstrap::load_env()
}

#[inline]
fn parse_temperature(s: &str) -> std::result::Result<f64, String> {
    _bootstrap::parse_temperature(s)
}

mod agent { pub use senweavercoding::agent::*; }
mod auth { pub use senweavercoding::auth::*; }
mod channels { pub use senweavercoding::channels::*; }
mod cli { pub use senweavercoding::cli::*; }
mod commands { pub use senweavercoding::commands::*; }
mod config { pub use senweavercoding::config::*; }
mod cost { pub use senweavercoding::cost::*; }
mod cron { pub use senweavercoding::cron::*; }
mod daemon { pub use senweavercoding::daemon::*; }
mod doctor { pub use senweavercoding::doctor::*; }
mod gateway { pub use senweavercoding::gateway::*; }
mod hardware { pub use senweavercoding::hardware::*; }
mod integrations { pub use senweavercoding::integrations::*; }
mod memory { pub use senweavercoding::memory::*; }
mod migration { pub use senweavercoding::migration::*; }
mod observability { pub use senweavercoding::observability::*; }
mod onboard { pub use senweavercoding::onboard::*; }
mod peripherals { pub use senweavercoding::peripherals::*; }
#[cfg(feature = "plugins-wasm")]
mod plugins { pub use senweavercoding::plugins::*; }
mod providers { pub use senweavercoding::providers::*; }
mod rpc { pub use senweavercoding::rpc::*; }
mod runtime { pub use senweavercoding::runtime::*; }
mod security { pub use senweavercoding::security::*; }
mod services { pub use senweavercoding::services::*; }
mod apply_model { pub use senweavercoding::apply_model::*; }

mod inline_completion { pub use senweavercoding::inline_completion::*; }
mod inline_edit { pub use senweavercoding::inline_edit::*; }
mod skills { pub use senweavercoding::skills::*; }
mod sop { pub use senweavercoding::sop::*; }
mod token_saver { pub use senweavercoding::token_saver::*; }
mod util { pub use senweavercoding::util::*; }
use config::Config;

pub use senweavercoding::{
    ChannelCommands, CronCommands, GatewayCommands, HardwareCommands, IntegrationCommands,
    MemoryCommands, MigrateCommands, PeripheralCommands, ServiceCommands, SkillCommands,
    SopCommands,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum CompletionShell {
    #[value(name = "bash")]
    Bash,
    #[value(name = "fish")]
    Fish,
    #[value(name = "zsh")]
    Zsh,
    #[value(name = "powershell")]
    PowerShell,
    #[value(name = "elvish")]
    Elvish,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum EstopLevelArg {
    #[value(name = "kill-all")]
    KillAll,
    #[value(name = "network-kill")]
    NetworkKill,
    #[value(name = "domain-block")]
    DomainBlock,
    #[value(name = "tool-freeze")]
    ToolFreeze,
}

#[derive(Parser, Debug)]
#[command(name = "sen")]
#[command(author = "senweaver")]
#[command(version)]
#[command(
    about = "SenWeaverCoding \u{1F680} AI Code Editor",
    long_about = "\
SenWeaverCoding \u{1F680} AI Code Editor

Usage:
  sen                          Start interactive session
  sen \"explain this code\"      Start with initial prompt
  sen -p \"summarize\"           One-shot print mode
  sen -c                       Continue last conversation
  sen onboard                  First-time setup
  sen --help                   Show all commands"
)]
struct Cli {

    #[arg(short = 'P', long = "project", global = true, value_name = "PATH")]
    project: Option<PathBuf>,

    #[arg(long, global = true)]
    config_dir: Option<String>,

    #[arg(long, global = true)]
    read_only: bool,

    #[arg(long, global = true, value_name = "N")]
    max_iterations: Option<usize>,

    #[arg(long, global = true)]
    dry_run: bool,

    #[arg(short = 'v', long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[arg(value_name = "PROMPT", conflicts_with = "continue_session")]
    prompt: Option<String>,

    #[arg(short = 'p', long = "print")]
    print_mode: bool,

    #[arg(short = 'c', long = "continue")]
    continue_session: bool,

    #[arg(long, global = true)]
    model: Option<String>,

    #[arg(long)]
    mode: Option<String>,

    #[arg(long = "legacy-mode", global = true)]
    legacy_mode: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {

    Onboard {

        #[arg(long)]
        force: bool,

        #[arg(long)]
        reinit: bool,

        #[arg(long)]
        channels_only: bool,

        #[arg(long)]
        api_key: Option<String>,

        #[arg(long)]
        provider: Option<String>,

        #[arg(long)]
        model: Option<String>,

        #[arg(long)]
        memory: Option<String>,

        #[arg(long)]
        quick: bool,
    },

    #[command(long_about = "\
Start the AI agent loop.

Launches an interactive chat session with the configured AI provider. \
Use --message for single-shot queries without entering interactive mode.

Examples:
  sen agent                              # interactive session
  sen agent -m \"Summarize today's logs\"  # single message
  sen agent -p anthropic --model <model-id>
  sen agent --peripheral nucleo-f401re:/dev/ttyACM0")]
    Agent {

        #[arg(short, long)]
        message: Option<String>,

        #[arg(short, long)]
        interactive: bool,

        #[arg(short, long)]
        background: bool,

        #[arg(long)]
        session_state_file: Option<PathBuf>,

        #[arg(short, long)]
        provider: Option<String>,

        #[arg(long)]
        model: Option<String>,

        #[arg(short, long, value_parser = parse_temperature)]
        temperature: Option<f64>,

        #[arg(long)]
        peripheral: Vec<String>,

        #[arg(long)]
        mode: Option<String>,
    },

    #[command(long_about = "\
Manage the gateway server (webhooks, websockets).

Start, restart, or inspect the HTTP/WebSocket gateway that accepts \
incoming webhook events and WebSocket connections.

Examples:
  sen gateway start              # start gateway
  sen gateway restart            # restart gateway
  sen gateway get-paircode       # show pairing code")]
    Gateway {
        #[command(subcommand)]
        gateway_command: Option<senweavercoding::GatewayCommands>,
    },

    #[command(long_about = "\
Start the ACP server (JSON-RPC 2.0 over stdio).

Launches a JSON-RPC 2.0 server on stdin/stdout for IDE and tool \
integration. Supports session management and streaming agent \
responses as notifications.

Methods: initialize, session/new, session/prompt, session/stop.

Examples:
  sen acp                        # start ACP server
  sen acp --max-sessions 5       # limit concurrent sessions")]
    Acp {

        #[arg(long)]
        max_sessions: Option<usize>,

        #[arg(long)]
        session_timeout: Option<u64>,
    },

    #[command(long_about = "\
Start the long-running autonomous daemon.

Launches the full SenWeaverCoding runtime: gateway server, all configured \
channels (Telegram, Discord, Slack, etc.), heartbeat monitor, and \
the cron scheduler. This is the recommended way to run SenWeaverCoding in \
production or as an always-on assistant.

Use 'sen service install' to register the daemon as an OS \
service (systemd/launchd) for auto-start on boot.

Examples:
  sen daemon                   # use config defaults
  sen daemon -p 9090           # gateway on port 9090
  sen daemon --host 127.0.0.1  # localhost only")]
    Daemon {

        #[arg(short, long)]
        port: Option<u16>,

        #[arg(long)]
        host: Option<String>,
    },

    Service {

        #[arg(long, default_value = "auto", value_parser = ["auto", "systemd", "openrc"])]
        service_init: String,

        #[command(subcommand)]
        service_command: ServiceCommands,
    },

    Doctor {
        #[command(subcommand)]
        doctor_command: Option<DoctorCommands>,
    },

    Rpc {

        #[arg(long)]
        stdio: bool,

        #[arg(long, num_args = 1)]
        unix_socket: Option<String>,

        #[arg(long)]
        http: bool,

        #[arg(long)]
        http_host: Option<String>,

        #[arg(long)]
        http_port: Option<u16>,
    },

    Status {

        #[arg(long)]
        format: Option<String>,
    },

    Estop {
        #[command(subcommand)]
        estop_command: Option<EstopSubcommands>,

        #[arg(long, value_enum)]
        level: Option<EstopLevelArg>,

        #[arg(long = "domain")]
        domains: Vec<String>,

        #[arg(long = "tool")]
        tools: Vec<String>,
    },

    #[command(long_about = "\
Configure and manage scheduled tasks.

Schedule recurring, one-shot, or interval-based tasks using cron \
expressions, RFC 3339 timestamps, durations, or fixed intervals.

Cron expressions use the standard 5-field format: \
'min hour day month weekday'. Timezones default to UTC; \
override with --tz and an IANA timezone name.

Examples:
  sen cron list
  sen cron add '0 9 * * 1-5' 'Good morning' --tz America/New_York --agent
  sen cron add '*/30 * * * *' 'Check system health' --agent
  sen cron add '*/5 * * * *' 'echo ok'
  sen cron add-at 2025-01-15T14:00:00Z 'Send reminder' --agent
  sen cron add-every 60000 'Ping heartbeat'
  sen cron once 30m 'Run backup in 30 minutes' --agent
  sen cron pause <task-id>
  sen cron update <task-id> --expression '0 8 * * *' --tz Europe/London")]
    Cron {
        #[command(subcommand)]
        cron_command: CronCommands,
    },

    Models {
        #[command(subcommand)]
        model_command: ModelCommands,
    },

    Providers,

    #[command(long_about = "\
Manage communication channels.

Add, remove, list, send, and health-check channels that connect SenWeaverCoding \
to messaging platforms. Supported channel types: telegram, discord, \
slack, whatsapp, matrix, imessage, email.

Examples:
  sen channel list
  sen channel doctor
  sen channel add telegram '{\"bot_token\":\"...\",\"name\":\"my-bot\"}'
  sen channel remove my-bot
  sen channel bind-telegram sen_user
  sen channel send 'Alert!' --channel-id telegram --recipient 123456789")]
    Channel {
        #[command(subcommand)]
        channel_command: ChannelCommands,
    },

    Integrations {
        #[command(subcommand)]
        integration_command: IntegrationCommands,
    },

    Skills {
        #[command(subcommand)]
        skill_command: SkillCommands,
    },

    Migrate {
        #[command(subcommand)]
        migrate_command: MigrateCommands,
    },

    Auth {
        #[command(subcommand)]
        auth_command: AuthCommands,
    },

    #[command(long_about = "\
Discover and introspect USB hardware.

Enumerate connected USB devices, identify known development boards \
(STM32 Nucleo, Arduino, ESP32), and retrieve chip information via \
probe-rs / ST-Link.

Examples:
  sen hardware discover
  sen hardware introspect /dev/ttyACM0
  sen hardware info --chip STM32F401RETx")]
    Hardware {
        #[command(subcommand)]
        hardware_command: senweavercoding::HardwareCommands,
    },

    #[command(long_about = "\
Manage hardware peripherals.

Add, list, flash, and configure hardware boards that expose tools \
to the agent (GPIO, sensors, actuators). Supported boards: \
nucleo-f401re, rpi-gpio, esp32, arduino-uno.

Examples:
  sen peripheral list
  sen peripheral add nucleo-f401re /dev/ttyACM0
  sen peripheral add rpi-gpio native
  sen peripheral flash --port /dev/cu.usbmodem12345
  sen peripheral flash-nucleo")]
    Peripheral {
        #[command(subcommand)]
        peripheral_command: senweavercoding::PeripheralCommands,
    },

    #[command(long_about = "\
Manage agent memory entries.

List, inspect, and clear memory entries stored by the agent. \
Supports filtering by category and session, pagination, and \
batch clearing with confirmation.

Examples:
  sen memory stats
  sen memory list
  sen memory list --category core --limit 10
  sen memory get <key>
  sen memory clear --category conversation --yes")]
    Memory {
        #[command(subcommand)]
        memory_command: MemoryCommands,
    },

    #[command(long_about = "\
Manage SenWeaverCoding configuration.

Inspect and export configuration settings. Use 'schema' to dump \
the full JSON Schema for the config file, which documents every \
available key, type, and default value.

Examples:
  sen config schema              # print JSON Schema to stdout
  sen config schema > schema.json")]
    Config {
        #[command(subcommand)]
        config_command: ConfigCommands,
    },

    #[command(long_about = "\
Check for and apply SenWeaverCoding updates.

By default, downloads and installs the latest release with a \
6-phase pipeline: preflight, download, backup, validate, swap, \
and smoke test. Automatic rollback on failure.

Use --check to only check for updates without installing.
Use --force to skip the confirmation prompt.
Use --version to target a specific release instead of latest.

Examples:
  sen update                      # download and install latest
  sen update --check              # check only, don't install
  sen update --force              # install without confirmation
  sen update --version 0.6.0      # install specific version")]
    Update {

        #[arg(long)]
        check: bool,

        #[arg(long)]
        force: bool,

        #[arg(long)]
        version: Option<String>,
    },

    #[command(long_about = "\
Run diagnostic self-tests to verify the SenWeaverCoding installation.

By default, runs the full test suite including network checks \
(gateway health, memory round-trip). Use --quick to skip network \
checks for faster offline validation.

Examples:
  sen self-test             # full suite
  sen self-test --quick     # quick checks only (no network)")]
    SelfTest {

        #[arg(long)]
        quick: bool,
    },

    #[command(long_about = "\
Generate shell completion scripts for `sen`.

The script is printed to stdout so it can be sourced directly:

Examples:
  source <(sen completions bash)
  sen completions zsh > ~/.zfunc/_sen
  sen completions fish > ~/.config/fish/completions/sen.fish")]
    Completions {

        #[arg(value_enum)]
        shell: CompletionShell,
    },

    #[command(name = "complete")]
    Complete {

        #[arg(long)]
        prefix: Option<String>,

        #[arg(long, default_value = "")]
        suffix: String,

        #[arg(long)]
        language: Option<String>,

        #[arg(long)]
        file_path: Option<PathBuf>,

        #[arg(long, default_value_t = 128)]
        max_tokens: u32,

        #[arg(long, default_value_t = 1)]
        top_k: u32,

        #[arg(long = "stop")]
        stop_sequences: Vec<String>,

        #[arg(long)]
        stream: bool,
    },

    #[command(name = "edit")]
    Edit {

        #[arg(long)]
        file: PathBuf,

        #[arg(long)]
        instruction: String,

        #[arg(long)]
        apply: bool,

        #[arg(long)]
        show_applied: bool,
    },

    #[command(name = "predict-next")]
    PredictNext {

        #[arg(long)]
        file: PathBuf,

        #[arg(long)]
        cursor_line: Option<u32>,

        #[arg(long)]
        recent_diff: Option<PathBuf>,

        #[arg(long)]
        apply: bool,
    },

    #[command(name = "mcp")]
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },

    #[command(name = "team")]
    Team {
        #[command(subcommand)]
        action: TeamAction,
    },

    #[command(long_about = "\
Launch the SenWeaverCoding companion desktop app.

The companion app is a lightweight menu bar / system tray application \
that connects to the same gateway as the CLI. It provides quick access \
to the dashboard, status monitoring, and device pairing.

Use --install to download the pre-built companion app for your platform.

Examples:
  sen desktop              # launch the companion app
  sen desktop --install    # download and install it")]
    Desktop {

        #[arg(long)]
        install: bool,
    },

    #[cfg(feature = "plugins-wasm")]
    Plugin {
        #[command(subcommand)]
        plugin_command: PluginCommands,
    },

    #[command(long_about = "\
List background agent sessions.

Shows all background sessions that have been started with `sen agent --background`.

Examples:
  sen ps")]
    Ps,

    #[command(long_about = "\
Show logs from a background agent session.

Displays the log output from the specified session ID.

Examples:
  sen logs <session-id>
  sen logs <session-id> --tail 50")]
    Logs {

        id: String,

        #[arg(long)]
        tail: Option<usize>,
    },

    #[command(long_about = "\
Terminate a running background agent session.

Sends a termination signal to the specified session and marks it as stopped.

Examples:
  sen kill <session-id>")]
    Kill {

        id: String,
    },

    #[command(long_about = "\
Run the agent in headless evaluation mode.

Executes an instruction against a working directory without interactive \
input. Outputs structured JSON results and a conversation transcript. \
Designed for CI pipelines, benchmarks, and automated testing.

Exit codes:
  0  success
  1  error
  2  timeout
  3  interrupted

Examples:
  sen eval --instruction 'Fix all linter errors' --workdir ./project
  sen eval -i 'Add tests for auth module' --model <model-id>
  sen eval --instruction - --timeout 300  # read instruction from stdin
  cat task.txt | sen eval -i - --output-dir ./results")]
    Eval {

        #[arg(short, long)]
        instruction: String,

        #[arg(long)]
        workdir: Option<PathBuf>,

        #[arg(long)]
        model: Option<String>,

        #[arg(short, long)]
        provider: Option<String>,

        #[arg(long, default_value = "600")]
        timeout: u64,

        #[arg(long)]
        output_dir: Option<PathBuf>,
    },

    #[command(long_about = "\
Run a benchmark evaluation suite by driving the agent on each problem.

Available suites: humaneval, mbpp, swebench. The agent runs once per problem \
and the suite judges each output; a JSON report with pass@1 is printed.

Exit codes:
  0  all problems ran without executor errors
  1  one or more problems errored (e.g. timeout / agent failure)

Examples:
  sen evals --suite humaneval
  sen evals --suite mbpp --concurrency 2 --model <model-id>
  sen evals --suite swebench --output ./report.json")]
    Evals {

        #[arg(long, default_value = "humaneval")]
        suite: String,

        #[arg(long, default_value = "1")]
        concurrency: usize,

        #[arg(long)]
        model: Option<String>,

        #[arg(short, long)]
        provider: Option<String>,

        #[arg(long, default_value = "600")]
        timeout: u64,

        #[arg(long)]
        output: Option<PathBuf>,
    },

    #[command(long_about = "\
Compare two files and display a unified diff.

Shows the differences between two files in unified diff format, \
similar to `diff -u`. Useful for reviewing changes between file versions.

Examples:
  sen diff old.rs new.rs
  sen diff src/main.rs.bak src/main.rs
  sen diff --context 5 a.txt b.txt")]
    Diff {

        old: PathBuf,

        new: PathBuf,

        #[arg(short, long, default_value = "3")]
        context: usize,
    },

    #[cfg(feature = "tui")]
    #[command(long_about = "\
Launch the SenWeaverCoding terminal user interface.

Provides a rich dashboard with tabs for system status, agent chat, \
memory, channels, tasks, tools, commands, cost tracking, events, \
and logs. The Chat tab connects to the live agent loop.

By default, an event-driven main loop backed by a \
`spawn_blocking` input worker + 16 ms redraw ticks is used.  Pass `--legacy` \
(or set `TUI_LEGACY_LOOP=1`) to fall back to the legacy \
`poll(100ms)` loop on terminals where the new loop misbehaves.

Examples:
  sen tui
  sen tui --legacy")]
    Tui {

        #[arg(long = "legacy", default_value_t = false)]
        legacy: bool,
    },

    #[command(long_about = "\
Launch the SenWeaverCoding desktop GUI application.

Spawns the bundled `sen-desktop` Tauri binary which serves a React
frontend talking to an embedded HTTP/WebSocket gateway.

Examples:
  sen gui")]
    Gui,

    #[cfg(not(feature = "tui"))]
    Tui {

        #[arg(long = "legacy", default_value_t = false)]
        legacy: bool,
    },

    #[command(long_about = "\
Manage Standard Operating Procedures (SOPs) for agent workflows.

SOPs define step-by-step procedures that the agent follows for \
repetitive tasks. Each SOP has triggers, steps, and expected outcomes.

Examples:
  sen sop list                    # List all loaded SOPs
  sen sop validate                # Validate all SOP definitions
  sen sop validate my-sop         # Validate a specific SOP
  sen sop show my-sop             # Show details of a specific SOP")]
    Sop {
        #[command(subcommand)]
        sop_command: Option<SopCommands>,
    },

    Tokens {
        #[command(subcommand)]
        tokens_command: TokensCommands,
    },
}

#[derive(Subcommand, Debug)]
enum TeamAction {

    Run {

        goal: String,

        #[arg(long, default_value = "default")]
        pipeline: String,

        #[arg(long)]
        temperature: Option<f64>,

        #[arg(long)]
        stage_timeout_secs: Option<u64>,

        #[arg(long)]
        json: bool,
    },

    List,
}

#[derive(Subcommand, Debug)]
enum McpAction {

    Serve {

        #[arg(long, default_value = "stdio")]
        transport: String,

        #[arg(long)]
        bind: Option<String>,

        #[arg(long = "allow", value_delimiter = ',')]
        allow: Vec<String>,

        #[arg(long = "deny", value_delimiter = ',')]
        deny: Vec<String>,

        #[arg(long)]
        list_tools: bool,
    },
}

#[cfg(feature = "plugins-wasm")]
#[derive(Subcommand, Debug)]
enum PluginCommands {

    List,

    Install {

        source: String,
    },

    Remove {

        name: String,
    },

    Info {

        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCommands {

    Schema,

    Get {

        #[arg(value_name = "KEY")]
        key: String,
    },

    Set {

        #[arg(value_name = "KEY")]
        key: String,

        #[arg(value_name = "VALUE")]
        value: String,
    },

    List {

        #[arg(long, short)]
        keys_only: bool,
    },
}

#[derive(Subcommand, Debug)]
enum EstopSubcommands {

    Status,

    Resume {

        #[arg(long)]
        network: bool,

        #[arg(long = "domain")]
        domains: Vec<String>,

        #[arg(long = "tool")]
        tools: Vec<String>,

        #[arg(long)]
        otp: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum AuthCommands {

    Login {

        #[arg(long)]
        provider: String,

        #[arg(long, default_value = "default")]
        profile: String,

        #[arg(long)]
        device_code: bool,

        #[arg(long, value_name = "PATH", conflicts_with = "device_code")]
        import: Option<PathBuf>,
    },

    PasteRedirect {

        #[arg(long)]
        provider: String,

        #[arg(long, default_value = "default")]
        profile: String,

        #[arg(long)]
        input: Option<String>,
    },

    PasteToken {

        #[arg(long)]
        provider: String,

        #[arg(long, default_value = "default")]
        profile: String,

        #[arg(long)]
        token: Option<String>,

        #[arg(long)]
        auth_kind: Option<String>,
    },

    SetupToken {

        #[arg(long)]
        provider: String,

        #[arg(long, default_value = "default")]
        profile: String,
    },

    Refresh {

        #[arg(long)]
        provider: String,

        #[arg(long)]
        profile: Option<String>,
    },

    Logout {

        #[arg(long)]
        provider: String,

        #[arg(long, default_value = "default")]
        profile: String,
    },

    Use {

        #[arg(long)]
        provider: String,

        #[arg(long)]
        profile: String,
    },

    List,

    Status,
}

#[derive(Subcommand, Debug)]
enum ModelCommands {

    Refresh {

        #[arg(long)]
        provider: Option<String>,

        #[arg(long)]
        all: bool,

        #[arg(long)]
        force: bool,
    },

    List {

        #[arg(long)]
        provider: Option<String>,
    },

    Set {

        model: String,
    },

    Status,
}

#[derive(Subcommand, Debug)]
enum DoctorCommands {

    Models {

        #[arg(long)]
        provider: Option<String>,

        #[arg(long)]
        use_cache: bool,
    },

    Traces {

        #[arg(long)]
        id: Option<String>,

        #[arg(long)]
        event: Option<String>,

        #[arg(long)]
        contains: Option<String>,

        #[arg(long, default_value = "20")]
        limit: usize,
    },

    Bench {

        #[arg(long, default_value = "target/criterion")]
        path: PathBuf,

        #[arg(long, default_value_t = 0.05)]
        threshold: f64,
    },
}

#[derive(Subcommand, Debug)]
enum TokensCommands {

    Stats {

        #[arg(long, default_value = "20")]
        top: usize,

        #[arg(long, default_value_t = false)]
        json: bool,
    },

    Compact {

        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 1..)]
        argv: Vec<String>,

        #[arg(long)]
        level: Option<String>,
    },

    Reset {

        #[arg(long, default_value_t = false)]
        yes: bool,
    },

    Filters {
        #[command(subcommand)]
        filters_command: TokensFiltersCommands,
    },
}

#[derive(Subcommand, Debug)]
enum TokensFiltersCommands {

    List,
}

const AGENT_WORKER_STACK_SIZE: usize = 32 * 1024 * 1024;

fn main() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(AGENT_WORKER_STACK_SIZE)
        .build()?;
    runtime.block_on(async_main())
}

#[allow(clippy::too_many_lines)]
async fn async_main() -> Result<()> {
    crate::runtime::task_manager::ensure_process_start_recorded();

    if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
        eprintln!("Warning: Failed to install default crypto provider: {e:?}");
    }

    load_env();

    let cli = Cli::parse();

    if let Some(config_dir) = &cli.config_dir {
        let trimmed = config_dir.trim();
        if trimmed.is_empty() {
            bail!("--config-dir cannot be empty");
        }
        let expanded = crate::config::schema::expand_tilde_path(trimmed);
        let resolved = if expanded.is_absolute() {
            expanded
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(&expanded))
                .unwrap_or(expanded)
        };
        if let Err(e) = std::fs::create_dir_all(&resolved) {
            bail!(
                "--config-dir '{}' is not usable: {e}",
                resolved.display()
            );
        }
        crate::util::set_runtime_var("SEN_CONFIG_DIR", resolved.to_string_lossy().as_ref());
    }

    if let Some(ref project) = cli.project {
        let project_path = if project.is_absolute() {
            project.clone()
        } else {
            std::env::current_dir().map(|cwd| cwd.join(project))?
        };
        crate::util::set_runtime_var("SEN_WORKSPACE", project_path.to_string_lossy().as_ref());
    }

    if cli.read_only {
        crate::util::set_runtime_var("SEN_READ_ONLY", "1");
    }

    if let Some(max_iters) = cli.max_iterations {
        crate::util::set_runtime_var("SEN_MAX_ITERATIONS", max_iters.to_string());
    }

    if cli.dry_run {
        crate::util::set_runtime_var("SEN_DRY_RUN", "1");
    }

    if let Some(Commands::Completions { shell }) = &cli.command {
        let mut stdout = std::io::stdout().lock();
        write_shell_completion(*shell, &mut stdout)?;
        return Ok(());
    }

    let log_filter = if cli.verbose > 0 {
        let log_level = match cli.verbose {
            1 => "debug",
            2 => "trace",
            _ => "trace",
        };
        EnvFilter::new(log_level)
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };

    let subscriber = fmt::Subscriber::builder()
        .with_env_filter(log_filter)
        .fmt_fields(senweavercoding::observability::redact_layer::RedactingFieldFormatter::default())
        .finish();

    if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
        eprintln!("Warning: failed to set tracing subscriber: {e}");
    }

    let _ = senweavercoding::keybindings::install_global_resolver_from_disk();

    {
        use std::sync::Arc;
        #[cfg(feature = "observability-prometheus")]
        let observer: Arc<dyn senweavercoding::observability::Observer> =
            Arc::new(senweavercoding::observability::PrometheusObserver::new());
        #[cfg(not(feature = "observability-prometheus"))]
        let observer: Arc<dyn senweavercoding::observability::Observer> =
            Arc::new(senweavercoding::observability::LogObserver::new());
        let _ = senweavercoding::observability::set_global_observer(observer);
    }

    if let Some(Commands::Onboard {
        force,
        reinit,
        channels_only,
        api_key,
        provider,
        model,
        memory,
        quick,
    }) = &cli.command
    {
        let force = *force;
        let reinit = *reinit;
        let channels_only = *channels_only;
        let api_key = api_key.clone();
        let provider = provider.clone();
        let model = model.clone();
        let memory = memory.clone();
        let quick = *quick;

        if reinit && channels_only {
            bail!("--reinit and --channels-only cannot be used together");
        }
        if channels_only
            && (api_key.is_some() || provider.is_some() || model.is_some() || memory.is_some())
        {
            bail!("--channels-only does not accept --api-key, --provider, --model, or --memory");
        }
        if channels_only && force {
            bail!("--channels-only does not accept --force");
        }
        if quick && channels_only {
            bail!("--quick and --channels-only cannot be used together");
        }

        if reinit {
            let (sen_dir, _) = crate::config::schema::resolve_runtime_dirs_for_onboarding().await?;

            if sen_dir.exists() {
                let timestamp = chrono::Local::now().format("%Y%m%d%H%M%S");
                let backup_dir = format!("{}.backup.{}", sen_dir.display(), timestamp);

                println!("\u{2699}\u{FE0F}  Reinitializing SenWeaverCoding configuration...");
                println!("   Current config directory: {}", sen_dir.display());
                println!(
                    "   This will back up your existing config to: {}",
                    backup_dir
                );
                println!();
                print!("Continue? [y/N] ");
                std::io::stdout()
                    .flush()
                    .context("Failed to flush stdout")?;

                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                if !answer.trim().eq_ignore_ascii_case("y") {
                    println!("Aborted.");
                    return Ok(());
                }
                println!();

                tokio::fs::rename(&sen_dir, &backup_dir)
                    .await
                    .with_context(|| {
                        format!("Failed to backup existing config to {}", backup_dir)
                    })?;

                println!("   Backup created successfully.");
                println!("   Starting fresh initialization...\n");
            }
        }

        let has_provider_flags =
            api_key.is_some() || provider.is_some() || model.is_some() || memory.is_some();
        let is_tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
        let env_interactive = std::env::var("SEN_INTERACTIVE").as_deref() == Ok("1");

        let config = if channels_only {
            Box::pin(onboard::run_channels_repair_wizard()).await
        } else if quick || has_provider_flags {
            Box::pin(onboard::run_quick_setup(
                api_key.as_deref(),
                provider.as_deref(),
                model.as_deref(),
                memory.as_deref(),
                force,
            ))
            .await
        } else if is_tty || env_interactive {
            Box::pin(onboard::run_wizard(force)).await
        } else {
            Box::pin(onboard::run_quick_setup(
                api_key.as_deref(),
                provider.as_deref(),
                model.as_deref(),
                memory.as_deref(),
                force,
            ))
            .await
        }?;

        if config.gateway.require_pairing {
            println!();
            println!("  Pairing is enabled. A one-time pairing code will be");
            println!("  displayed when the gateway starts.");
            println!("  Dashboard: http://127.0.0.1:{}", config.gateway.port);
            println!();
        }

        if crate::util::get_runtime_var("SEN_AUTOSTART_CHANNELS").as_deref() == Some("1") {
            Box::pin(channels::start_channels(config)).await?;
        }
        return Ok(());
    }

    let mut config = Box::pin(Config::load_or_init()).await?;
    config.apply_env_overrides();
    observability::runtime_trace::init_from_config(&config.observability, &config.workspace_dir);

    crate::token_saver::set_enabled(config.token_saver.enabled);
    crate::token_saver::set_global(config.token_saver.to_runtime_ctx());
    if config.security.otp.enabled {
        let config_dir = config
            .config_path
            .parent()
            .context("Config path must have a parent directory")?;
        let store = security::SecretStore::new(config_dir, config.secrets.encrypt);
        let (_validator, enrollment_uri) =
            security::OtpValidator::from_config(&config.security.otp, config_dir, &store)?;
        if let Some(uri) = enrollment_uri {
            println!("Initialized OTP secret for SenWeaverCoding.");
            println!("Enrollment URI: {uri}");
        }
    }

    let command = cli.command.unwrap_or_else(|| Commands::Agent {
        message: cli.prompt.clone(),
        interactive: !cli.print_mode,
        background: false,
        session_state_file: None,
        provider: None,
        model: cli.model.clone(),
        temperature: None,
        peripheral: vec![],
        mode: cli.mode.clone(),
    });

    match command {

        Commands::Onboard { .. } | Commands::Completions { .. } => {
            unreachable!("invariant: Onboard/Completions are short-circuited before entering the main command dispatcher")
        }

        Commands::Agent {
            message,
            interactive,
            background,
            session_state_file,
            provider,
            model,
            temperature,
            peripheral,
            mode,
        } => {

            senweavercoding::bootstrap::init_state(std::env::current_dir().unwrap_or_default());

            if let Some(ref mode_str) = mode {
                if let Some(coding_mode) =
                    senweavercoding::agent::coding_mode::CodingMode::from_str_loose(mode_str)
                {
                    let _ = std::panic::catch_unwind(|| {
                        let svc = senweavercoding::services::require_services();
                        *svc.coding_mode.write() = coding_mode;
                    });
                } else {

                    let available: Vec<&'static str> =
                        senweavercoding::agent::coding_mode::CodingMode::all()
                            .iter()
                            .map(|m| m.display_name())
                            .collect();
                    eprintln!(
                        "Warning: unknown mode '{}'. Available: {}",
                        mode_str,
                        available.join(", ")
                    );
                }
            }

            let session_state_file = if cli.continue_session && session_state_file.is_none() {
                let sessions_dir = config
                    .workspace_dir
                    .join(".senweavercoding")
                    .join("sessions");
                if sessions_dir.is_dir() {
                    let mut entries: Vec<_> = std::fs::read_dir(&sessions_dir)?
                        .filter_map(|e| e.ok())
                        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
                        .collect();
                    entries.sort_by_key(|e| {
                        std::cmp::Reverse(
                            e.metadata()
                                .and_then(|m| m.modified())
                                .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                        )
                    });
                    entries.first().map(|e| e.path())
                } else {
                    None
                }
            } else {
                session_state_file
            };

            let message = match message.as_deref() {
                Some("-") => {
                    let mut buf = String::new();
                    if std::io::stdin().is_terminal() {
                        eprintln!("Reading from stdin (press Ctrl+D to finish)...");
                    }
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
                    if buf.trim().is_empty() {
                        None
                    } else {
                        Some(buf)
                    }
                }
                _ => message,
            };

            let final_temperature = temperature.unwrap_or(config.default_temperature);
            let is_interactive = interactive || message.is_none();

            if background {

                let session_id = uuid::Uuid::new_v4().to_string();
                let workspace = config.workspace_dir.clone();
                let info = senweavercoding::cli::bg::SessionInfo {
                    id: session_id.clone(),
                    pid: Some(std::process::id()),
                    started_at: chrono::Utc::now().to_rfc3339(),
                    status: senweavercoding::cli::bg::SessionStatus::Running,
                    cwd: std::env::current_dir().unwrap_or_default(),
                    last_activity: chrono::Utc::now().to_rfc3339(),
                    pid_start_time: senweavercoding::cli::bg::capture_current_start_time(),
                    argv0_hash: std::env::args().next().map(|a| {
                        use sha2::{Digest, Sha256};
                        let mut h = Sha256::new();
                        h.update(a.as_bytes());
                        hex::encode(&h.finalize()[..8])
                    }),
                };
                senweavercoding::cli::bg::save_session(&workspace, &info).await?;
                println!("Background session started: {session_id}");
                println!("Use `sen ps` to check status, `sen logs {session_id}` for output.");

                let session_file = session_state_file.unwrap_or_else(|| {
                    workspace
                        .join(".senweavercoding")
                        .join("sessions")
                        .join(format!("{session_id}.state.json"))
                });

                Box::pin(agent::run(
                    config,
                    message,
                    provider,
                    model,
                    final_temperature,
                    peripheral,
                    false,
                    Some(session_file),
                    None,
                    None,
                ))
                .await
                .map(|_| ())?;

                let updated = senweavercoding::cli::bg::SessionInfo {
                    status: senweavercoding::cli::bg::SessionStatus::Stopped,
                    last_activity: chrono::Utc::now().to_rfc3339(),
                    ..info
                };
                senweavercoding::cli::bg::save_session(&workspace, &updated).await?;
                Ok(())
            } else if cli.legacy_mode
                || std::env::var("SEN_LEGACY_MODE")
                    .ok()
                    .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                || !is_interactive
            {

                Box::pin(agent::run(
                    config,
                    message,
                    provider,
                    model,
                    final_temperature,
                    peripheral,
                    is_interactive,
                    session_state_file,
                    None,
                    None,
                ))
                .await
                .map(|_| ())
            } else {

                use senweavercoding::entrypoints::cli::{CliEntrypoint, CliOptions};
                let opts = CliOptions {
                    prompt: message,
                    model,
                    provider,
                    temperature: Some(final_temperature),
                    peripherals: peripheral,
                    session_state_file,
                    legacy_mode: cli.legacy_mode,
                    ..CliOptions::default()
                };
                let _ = config;
                CliEntrypoint::run(opts).await
            }
        }

        Commands::Acp {
            max_sessions,
            session_timeout,
        } => {
            let mut acp_config = channels::acp_server::AcpServerConfig::default();
            if let Some(max) = max_sessions {
                acp_config.max_sessions = max;
            }
            if let Some(timeout) = session_timeout {
                acp_config.session_timeout_secs = timeout;
            }
            let server = channels::acp_server::AcpServer::new(config, acp_config);
            server.run().await
        }

        Commands::Gateway { gateway_command } => {
            match gateway_command {
                Some(senweavercoding::GatewayCommands::Restart { port, host }) => {
                    let (port, host) = resolve_gateway_addr(&config, port, host);
                    let addr = format!("{host}:{port}");
                    info!("\u{1F501} Restarting SenWeaverCoding Gateway on {addr}");

                    match shutdown_gateway(&host, port).await {
                        Ok(()) => {
                            info!("   \u{2713} Existing gateway on {addr} shut down gracefully");

                            let deadline =
                                tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
                            loop {
                                match tokio::net::TcpStream::connect(&addr).await {
                                    Err(_) => break,
                                    Ok(_) if tokio::time::Instant::now() >= deadline => {
                                        warn!(
                                            "   Timed out waiting for port {port} to be released"
                                        );
                                        break;
                                    }
                                    Ok(_) => {
                                        tokio::time::sleep(tokio::time::Duration::from_millis(50))
                                            .await;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            info!("   No existing gateway to shut down: {e}");
                        }
                    }

                    log_gateway_start(&host, port);
                    Box::pin(gateway::run_gateway_with_supervisors(&host, port, config, None)).await
                }
                Some(senweavercoding::GatewayCommands::GetPaircode { new }) => {
                    let port = config.gateway.port;
                    let host = &config.gateway.host;

                    match fetch_paircode(host, port, new).await {
                        Ok(Some(code)) => {
                            println!("\u{1F511} Gateway pairing is enabled.");
                            println!();
                            let width = code.chars().count() + 4;
                            let bar: String = std::iter::repeat('\u{2501}').take(width).collect();
                            println!("  \u{250F}{bar}\u{2513}");
                            println!("  \u{2503}  {code}  \u{2503}");
                            println!("  \u{2517}{bar}\u{251B}");
                            println!();
                            println!("  Use this one-time code to pair a new device:");
                            println!("    POST /pair with header X-Pairing-Code: {code}");
                        }
                        Ok(None) => {
                            if config.gateway.require_pairing {
                                println!(
                                    "\u{26A0}\u{FE0F} Gateway pairing is enabled, but no active pairing code available."
                                );
                                println!(
                                    "   The gateway may already be paired, or the code has been used."
                                );
                                println!("   Restart the gateway to generate a new pairing code.");
                            } else {
                                println!("\u{1F513}  Gateway pairing is disabled in config.");
                                println!(
                                    "   All requests will be accepted without authentication."
                                );
                                println!(
                                    "   To enable pairing, set [gateway] require_pairing = true"
                                );
                            }
                        }
                        Err(e) => {
                            println!(
                                "\u{274C} Failed to fetch pairing code from gateway at {host}:{port}"
                            );
                            println!("   Error: {e}");
                            println!();
                            println!("   Is the gateway running? Start it with:");
                            println!("     sen gateway start");
                        }
                    }
                    Ok(())
                }
                Some(senweavercoding::GatewayCommands::Start { port, host }) => {
                    let (port, host) = resolve_gateway_addr(&config, port, host);
                    log_gateway_start(&host, port);
                    Box::pin(gateway::run_gateway_with_supervisors(&host, port, config, None)).await
                }
                None => {
                    let port = config.gateway.port;
                    let host = config.gateway.host.clone();
                    log_gateway_start(&host, port);
                    Box::pin(gateway::run_gateway_with_supervisors(&host, port, config, None)).await
                }
            }
        }

        Commands::Daemon { port, host } => {
            if let Ok(exe) = std::env::current_exe() {
                let exe_str = exe.to_string_lossy();
                if exe_str.contains(".cargo/bin") || exe_str.contains("/home/") {
                    tracing::warn!(
                        "Daemon running from user home directory: {}. \
                         Consider installing to /usr/local/bin for system-wide service.",
                        exe_str
                    );
                }
            }
            let port = port.unwrap_or(config.gateway.port);
            let host = host.unwrap_or_else(|| config.gateway.host.clone());
            if port == 0 {
                info!("\u{1F680} Starting SenWeaverCoding Daemon on {host} (random port)");
            } else {
                info!("\u{1F680} Starting SenWeaverCoding Daemon on {host}:{port}");
            }
            Box::pin(daemon::run(config, host, port)).await
        }

        Commands::Status { format } => {
            if format.as_deref() == Some("exit-code") {

                let port = config.gateway.port;
                let host = if config.gateway.host == "[::]" || config.gateway.host == "0.0.0.0" {
                    "127.0.0.1"
                } else {
                    &config.gateway.host
                };
                let url = format!("http://{}:{}/health", host, port);
                match reqwest::Client::new()
                    .get(&url)
                    .timeout(std::time::Duration::from_secs(5))
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        std::process::exit(0);
                    }
                    _ => {
                        std::process::exit(1);
                    }
                }
            }
            println!("\u{1F4CB} SenWeaverCoding Status");
            println!();
            println!("Version:     {}", env!("CARGO_PKG_VERSION"));
            println!("Workspace:   {}", config.workspace_dir.display());
            println!("Config:      {}", config.config_path.display());
            println!();
            println!(
                "\u{1F916} Provider:      {}",
                config.default_provider.as_deref().unwrap_or("openrouter")
            );
            println!(
                "   Model:         {}",
                config.default_model.as_deref().unwrap_or("(default)")
            );
            println!("\u{1F4CA} Observability:  {}", config.observability.backend);
            println!(
                "\u{1F4BE} Trace storage:  {} ({})",
                config.observability.runtime_trace_mode, config.observability.runtime_trace_path
            );
            println!("\u{1F6E1}\u{FE0F}  Autonomy:      {:?}", config.autonomy.level);
            println!("\u{2699}\u{FE0F}  Runtime:       {}", config.runtime.kind);
            if services::service::is_running() {
                println!("\u{1F7E2} Service:       running");
            } else {
                println!("\u{1F534} Service:       stopped");
            }
            let effective_memory_backend = memory::effective_memory_backend_name(
                &config.memory.backend,
                Some(&config.storage.provider.config),
            );
            println!(
                "\u{1F493} Heartbeat:      {}",
                if config.heartbeat.enabled {
                    format!("every {}min", config.heartbeat.interval_minutes)
                } else {
                    "disabled".into()
                }
            );
            println!(
                "\u{1F9E0} Memory:         {} (auto-save: {})",
                effective_memory_backend,
                if config.memory.auto_save { "on" } else { "off" }
            );

            println!();
            println!("Security:");
            println!("  Workspace only:    {}", config.autonomy.workspace_only);
            println!(
                "  Allowed roots:     {}",
                if config.autonomy.allowed_roots.is_empty() {
                    "(none)".to_string()
                } else {
                    config.autonomy.allowed_roots.join(", ")
                }
            );
            println!(
                "  Allowed commands:  {}",
                config.autonomy.allowed_commands.join(", ")
            );
            println!(
                "  Max actions/hour:  {}",
                if config.autonomy.max_actions_per_hour == 0 {
                    "disabled".to_string()
                } else {
                    config.autonomy.max_actions_per_hour.to_string()
                }
            );
            println!(
                "  Cost tracking:     {}",
                if config.cost.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!("  Max cost/day:      ${:.2}", config.cost.daily_limit_usd);
            println!("  Max cost/month:    ${:.2}", config.cost.monthly_limit_usd);
            if config.cost.enabled {
                match cost::CostTracker::new(config.cost.clone(), &config.workspace_dir) {
                    Ok(tracker) => match tracker.get_summary() {
                        Ok(summary) => {
                            println!(
                                "  Spent today:       ${:.4} / ${:.2}",
                                summary.daily_cost_usd, config.cost.daily_limit_usd
                            );
                            println!(
                                "  Spent this month:  ${:.4} / ${:.2}",
                                summary.monthly_cost_usd, config.cost.monthly_limit_usd
                            );
                        }
                        Err(e) => {
                            eprintln!("  \u{26A0}\u{FE0F} Could not load cost usage: {e}");
                        }
                    },
                    Err(e) => {
                        eprintln!("  \u{26A0}\u{FE0F} Could not init cost tracker: {e}");
                    }
                }
            }
            println!("  OTP enabled:       {}", config.security.otp.enabled);
            println!("  E-stop enabled:    {}", config.security.estop.enabled);
            println!();
            println!("Channels:");
            println!("  CLI:      \u{2713} always");
            for (channel, configured) in config.channels_config.channels() {
                println!(
                    "  {:9} {}",
                    channel.name(),
                    if configured {
                        "\u{2713} configured"
                    } else {
                        "\u{2717} not configured"
                    }
                );
            }
            println!();
            println!("Peripherals:");
            println!(
                "  Enabled:   {}",
                if config.peripherals.enabled {
                    "yes"
                } else {
                    "no"
                }
            );
            println!("  Boards:    {}", config.peripherals.boards.len());

            Ok(())
        }

        Commands::Estop {
            estop_command,
            level,
            domains,
            tools,
        } => handle_estop_command(&config, estop_command, level, domains, tools),

        Commands::Cron { cron_command } => cron::handle_command(cron_command, &config),

        Commands::Models { model_command } => match model_command {
            ModelCommands::Refresh {
                provider,
                all,
                force,
            } => {
                if all {
                    if provider.is_some() {
                        bail!("`models refresh --all` cannot be combined with --provider");
                    }
                    onboard::run_models_refresh_all(&config, force).await
                } else {
                    onboard::run_models_refresh(&config, provider.as_deref(), force).await
                }
            }
            ModelCommands::List { provider } => {
                onboard::run_models_list(&config, provider.as_deref()).await
            }
            ModelCommands::Set { model } => {
                Box::pin(onboard::run_models_set(&config, &model)).await
            }
            ModelCommands::Status => onboard::run_models_status(&config).await,
        },

        Commands::Providers => {
            let providers = providers::list_providers();
            let current = config
                .default_provider
                .as_deref()
                .unwrap_or("openrouter")
                .trim()
                .to_ascii_lowercase();
            println!("Supported providers ({} total):\n", providers.len());
            println!("  ID (use in config)  DESCRIPTION");
            let col1: String = std::iter::repeat('\u{2500}').take(19).collect();
            let col2: String = std::iter::repeat('\u{2500}').take(33).collect();
            println!("  {col1} {col2}");
            for p in &providers {
                let is_active = p.name.eq_ignore_ascii_case(&current)
                    || p.aliases
                        .iter()
                        .any(|alias| alias.eq_ignore_ascii_case(&current));
                let marker = if is_active { " (active)" } else { "" };
                let local_tag = if p.local { " [local]" } else { "" };
                let aliases = if p.aliases.is_empty() {
                    String::new()
                } else {
                    format!("  (aliases: {})", p.aliases.join(", "))
                };
                println!(
                    "  {:<19} {}{}{}{}",
                    p.name, p.display_name, local_tag, marker, aliases
                );
            }
            println!("\n  custom:<URL>   Any OpenAI-compatible endpoint");
            println!("  anthropic-custom:<URL>  Any Anthropic-compatible endpoint");
            Ok(())
        }

        Commands::Service {
            service_command,
            service_init,
        } => {
            let init_system = service_init.parse()?;
            services::service::handle_command(&service_command, &config, init_system)
        }

        Commands::Rpc {
            stdio,
            unix_socket,
            http,
            http_host,
            http_port,
        } => {
            use crate::rpc::{RpcServer, RpcServerConfig, RpcTransport};

            let rpc_cfg = &config.rpc;
            let transport = if unix_socket.is_some() {
                #[cfg(unix)]
                {
                    let socket_path = unix_socket
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("--unix-socket is expected to be set"))?;
                    RpcTransport::UnixSocket {
                        path: std::path::PathBuf::from(socket_path),
                        mode: "0755".to_string(),
                    }
                }
                #[cfg(not(unix))]
                {
                    println!("Unix Domain Socket transport is not available on Windows");
                    return Ok(());
                }
            } else if http {
                RpcTransport::Http {
                    host: http_host.unwrap_or_else(|| "127.0.0.1".to_string()),
                    port: http_port.unwrap_or(42618),
                }
            } else {

                if stdio || (config.rpc.stdio && unix_socket.is_none() && !http) {
                    RpcTransport::Stdio
                } else {

                    crate::rpc::server::build_transport(rpc_cfg)?
                }
            };

            let server_config = RpcServerConfig {
                enabled: true,
                transport,
                max_sessions: rpc_cfg.max_sessions,
                session_timeout_secs: rpc_cfg.session_timeout_secs,
                default_socket_path: std::path::PathBuf::from("/tmp/sen-rpc.sock"),
                default_http_port: 42618,
            };

            let server = RpcServer::from_config(server_config, config.clone()).await?;
            server.run().await
        }

        Commands::Doctor { doctor_command } => match doctor_command {
            Some(DoctorCommands::Models {
                provider,
                use_cache,
            }) => doctor::run_models(&config, provider.as_deref(), use_cache).await,
            Some(DoctorCommands::Traces {
                id,
                event,
                contains,
                limit,
            }            ) => doctor::run_traces(
                &config,
                id.as_deref(),
                event.as_deref(),
                contains.as_deref(),
                limit,
            ),
            Some(DoctorCommands::Bench { path, threshold }) => {
                let report = senweavercoding::bench_diff::load_estimates(&path)?;
                println!(
                    "{}",
                    senweavercoding::bench_diff::format_regression_table(&report, threshold)
                );
                Ok(())
            }
            None => doctor::run(&config),
        },

        Commands::Channel { channel_command } => match channel_command {
            ChannelCommands::Start => Box::pin(channels::start_channels(config)).await,
            ChannelCommands::Doctor => Box::pin(channels::doctor_channels(config)).await,
            other => Box::pin(channels::handle_command(other, &config)).await,
        },

        Commands::Integrations {
            integration_command,
        } => integrations::handle_command(integration_command, &config),

        Commands::Skills { skill_command } => skills::handle_command(skill_command, &config).await,

        Commands::Migrate { migrate_command } => {
            migration::handle_command(migrate_command, &config).await
        }

        Commands::Memory { memory_command } => {
            memory::cli::handle_command(memory_command, &config).await
        }

        Commands::Auth { auth_command } => handle_auth_command(auth_command, &config).await,

        Commands::Hardware { hardware_command } => {
            hardware::handle_command(hardware_command.clone(), &config)
        }

        Commands::Peripheral { peripheral_command } => {
            Box::pin(peripherals::handle_command(
                peripheral_command.clone(),
                &config,
            ))
            .await
        }

        Commands::Complete {
            prefix,
            suffix,
            language,
            file_path,
            max_tokens,
            top_k,
            stop_sequences,
            stream,
        } => {
            run_inline_complete_command(
                &config,
                prefix,
                suffix,
                language,
                file_path,
                max_tokens,
                top_k,
                stop_sequences,
                stream,
            )
            .await
        }

        Commands::Edit {
            file,
            instruction,
            apply,
            show_applied,
        } => {
            run_inline_edit_command(&config, file, instruction, apply, show_applied)
                .await
        }
        Commands::PredictNext {
            file,
            cursor_line,
            recent_diff,
            apply,
        } => {
            run_predict_next_command(&config, file, cursor_line, recent_diff, apply).await
        }

        Commands::Mcp { action } => run_mcp_command(&config, action).await,

        Commands::Team { action } => run_team_command(&config, action).await,

        Commands::Desktop {
            install: do_install,
        } => {
            let download_url = "https://www.senweavercoding-os.ai/download";

            if do_install {
                println!("Download the SenWeaverCoding companion app:");
                println!();
                #[cfg(target_os = "macos")]
                {
                    println!("  macOS:  {download_url}");
                    println!();
                    println!("Or install via Homebrew (coming soon):");
                    println!("  brew install --cask sen");
                }
                #[cfg(target_os = "linux")]
                {
                    println!("  Linux:  {download_url}");
                    println!();
                    println!("  Download the .deb or .AppImage for your architecture.");
                }
                #[cfg(not(any(target_os = "macos", target_os = "linux")))]
                {
                    println!("  {download_url}");
                }
                println!();

                #[cfg(target_os = "macos")]
                {
                    let _ = senweavercoding::util::hidden_sync_command("open").arg(download_url).spawn();
                }
                #[cfg(target_os = "linux")]
                {
                    let _ = senweavercoding::util::hidden_sync_command("xdg-open")
                        .arg(download_url)
                        .spawn();
                }
                return Ok(());
            }

            let desktop_bin = {
                let mut found = None;

                #[cfg(target_os = "macos")]
                {
                    let app_paths = [
                        PathBuf::from(
                            "/Applications/SenWeaverCoding.app/Contents/MacOS/SenWeaverCoding",
                        ),
                        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(
                            "Applications/SenWeaverCoding.app/Contents/MacOS/SenWeaverCoding",
                        ),
                    ];
                    for app in &app_paths {
                        if app.is_file() {
                            found = Some(app.clone());
                            break;
                        }
                    }
                }

                if found.is_none() {
                    if let Ok(exe) = std::env::current_exe() {
                        let sibling = exe.with_file_name("sen-desktop");
                        if sibling.is_file() {
                            found = Some(sibling);
                        }
                    }
                }

                if found.is_none() {
                    if let Some(home) = std::env::var_os("HOME") {
                        let home = PathBuf::from(home);
                        for dir in &[".cargo/bin", ".local/bin"] {
                            let candidate = home.join(dir).join("sen-desktop");
                            if candidate.is_file() {
                                found = Some(candidate);
                                break;
                            }
                        }
                    }
                }

                if found.is_none() {
                    if let Ok(path) = which::which("sen-desktop") {
                        found = Some(path);
                    }
                }

                found
            };

            match desktop_bin {
                Some(bin) => {
                    println!("Launching SenWeaverCoding companion app...");
                    let _child = senweavercoding::util::hidden_sync_command(&bin)
                        .spawn()
                        .with_context(|| format!("Failed to launch {}", bin.display()))?;
                    Ok(())
                }
                None => {
                    println!("SenWeaverCoding companion app is not installed.");
                    println!();
                    println!("  Download it at: {download_url}");
                    println!("  Or run: sen desktop --install");
                    println!();
                    println!("The companion app is a lightweight menu bar app that");
                    println!("connects to the same gateway as the CLI.");
                    std::process::exit(1);
                }
            }
        }

        Commands::Update {
            check,
            force: _force,
            version,
        } => {
            if check {
                let info = commands::update::check(version.as_deref()).await?;
                if info.is_newer {
                    println!(
                        "Update available: v{} -> v{}",
                        info.current_version, info.latest_version
                    );
                } else {
                    println!("Already up to date (v{}).", info.current_version);
                }
                Ok(())
            } else {
                commands::update::run(version.as_deref()).await
            }
        }

        Commands::SelfTest { quick } => {
            let results = if quick {
                commands::self_test::run_quick(&config).await?
            } else {
                commands::self_test::run_full(&config).await?
            };
            commands::self_test::print_results(&results);
            let failed = results.iter().filter(|r| !r.passed).count();
            if failed > 0 {
                std::process::exit(1);
            }
            Ok(())
        }

        Commands::Config { config_command } => match config_command {
            ConfigCommands::Schema => {
                let schema = schemars::schema_for!(config::Config);
                match serde_json::to_string_pretty(&schema) {
                    Ok(rendered) => {
                        println!("{rendered}");
                        Ok(())
                    }
                    Err(e) => {
                        eprintln!("Failed to serialize JSON Schema: {e}");
                        std::process::exit(1);
                    }
                }
            }
            ConfigCommands::Get { key } => {
                get_config_value(&config, &key)?;
                Ok(())
            }
            ConfigCommands::Set { key, value } => {
                set_config_value(&config, &key, &value).await?;
                Ok(())
            }
            ConfigCommands::List { keys_only } => {
                list_config_values(&config, keys_only)?;
                Ok(())
            }
        },

        #[cfg(feature = "plugins-wasm")]
        Commands::Plugin { plugin_command } => match plugin_command {
            PluginCommands::List => {
                let host = senweavercoding::plugins::host::PluginHost::new(&config.workspace_dir)?;
                let plugins = host.list_plugins();
                if plugins.is_empty() {
                    println!("No plugins installed.");
                } else {
                    println!("Installed plugins:");
                    for p in &plugins {
                        println!(
                            "  {} v{} \u{2192} {}",
                            p.name,
                            p.version,
                            p.description.as_deref().unwrap_or("(no description)")
                        );
                    }
                }
                Ok(())
            }
            PluginCommands::Install { source } => {
                let mut host =
                    senweavercoding::plugins::host::PluginHost::new(&config.workspace_dir)?;
                host.install(&source)?;
                println!("Plugin installed from {source}");
                Ok(())
            }
            PluginCommands::Remove { name } => {
                let mut host =
                    senweavercoding::plugins::host::PluginHost::new(&config.workspace_dir)?;
                host.remove(&name)?;
                println!("Plugin '{name}' removed.");
                Ok(())
            }
            PluginCommands::Info { name } => {
                let host = senweavercoding::plugins::host::PluginHost::new(&config.workspace_dir)?;
                match host.get_plugin(&name) {
                    Some(info) => {
                        println!("Plugin: {} v{}", info.name, info.version);
                        if let Some(desc) = &info.description {
                            println!("Description: {desc}");
                        }
                        println!("Capabilities: {:?}", info.capabilities);
                        println!("Permissions: {:?}", info.permissions);
                        println!("WASM: {}", info.wasm_path.display());
                    }
                    None => println!("Plugin '{name}' not found."),
                }
                Ok(())
            }
        },

        Commands::Ps => {
            let sessions = senweavercoding::cli::bg::list_sessions(&config.workspace_dir).await?;
            senweavercoding::cli::bg::print_sessions(&sessions);
            Ok(())
        }

        Commands::Eval {
            instruction,
            workdir,
            model,
            provider,
            timeout,
            output_dir,
        } => {

            let instruction = if instruction == "-" {
                let mut buf = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
                buf
            } else {
                instruction
            };

            if instruction.trim().is_empty() {
                bail!("Empty instruction. Provide --instruction or pipe via stdin.");
            }

            let workdir = workdir
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));

            let start = std::time::Instant::now();

            let agent_result = tokio::time::timeout(
                std::time::Duration::from_secs(timeout),
                Box::pin(agent::run(
                    config.clone(),
                    Some(instruction.clone()),
                    provider,
                    model,
                    config.default_temperature,
                    vec![],
                    false,
                    None,
                    None,
                    None,
                )),
            )
            .await;

            let elapsed = start.elapsed();
            let (status, error_msg, exit_code) = match agent_result {
                Ok(Ok(response)) => {
                    let _ = response;
                    ("success".to_string(), None, 0)
                }
                Ok(Err(e)) => ("error".to_string(), Some(format!("{e:#}")), 1),
                Err(_) => (
                    "timeout".to_string(),
                    Some(format!("Agent timed out after {timeout}s")),
                    2,
                ),
            };

            let result_json = serde_json::json!({
                "status": status,
                "duration_secs": elapsed.as_secs_f64(),
                "instruction": instruction,
                "workdir": workdir.display().to_string(),
                "error": error_msg,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });

            if let Some(out_dir) = output_dir {
                std::fs::create_dir_all(&out_dir)?;
                let result_path = out_dir.join("result.json");
                std::fs::write(&result_path, serde_json::to_string_pretty(&result_json)?)?;
                eprintln!("Results written to {}", result_path.display());

                let thread_path = out_dir.join("thread.md");
                let thread_content = format!(
                    "# Eval Session\n\n\
                     **Instruction:** {}\n\n\
                     **Status:** {}\n\n\
                     **Duration:** {:.1}s\n\n\
                     **Workdir:** {}\n",
                    instruction,
                    status,
                    elapsed.as_secs_f64(),
                    workdir.display(),
                );
                std::fs::write(&thread_path, thread_content)?;
            } else {
                println!("{}", serde_json::to_string_pretty(&result_json)?);
            }

            std::process::exit(exit_code);
        }

        Commands::Evals {
            suite,
            concurrency,
            model,
            provider,
            timeout,
            output,
        } => {
            let temperature = config.default_temperature;
            let executor = senweavercoding::evals::AgentEvalExecutor {
                config: config.clone(),
                provider,
                model,
                temperature,
                timeout_secs: timeout,
            };

            let report =
                senweavercoding::evals::run_agent_suite(&suite, executor, concurrency).await?;
            let report_json = serde_json::to_string_pretty(&report)?;

            if let Some(out) = output {
                if let Some(parent) = out.parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent)?;
                    }
                }
                std::fs::write(&out, &report_json)?;
                eprintln!("Eval report written to {}", out.display());
            } else {
                println!("{report_json}");
            }

            eprintln!(
                "[evals] suite={} total={} passed={} failed={} errored={} pass@1={:.3} \
                 avg_latency_ms={:.1}",
                report.suite,
                report.total,
                report.passed,
                report.failed,
                report.errored,
                report.pass_at_1,
                report.avg_latency_ms
            );

            std::process::exit(if report.errored > 0 { 1 } else { 0 });
        }

        Commands::Diff { old, new, context } => {
            let old_content = std::fs::read_to_string(&old)
                .with_context(|| format!("Failed to read {}", old.display()))?;
            let new_content = std::fs::read_to_string(&new)
                .with_context(|| format!("Failed to read {}", new.display()))?;

            let diff = similar::TextDiff::from_lines(&old_content, &new_content);
            let unified = diff
                .unified_diff()
                .context_radius(context)
                .header(&old.to_string_lossy(), &new.to_string_lossy())
                .to_string();

            if unified.trim().is_empty() {
                println!("Files are identical.");
            } else {
                print!("{unified}");
            }
            Ok(())
        }

        Commands::Logs { id, tail } => {
            let logs = senweavercoding::cli::bg::get_session_logs(&config.workspace_dir, &id, tail)
                .await?;
            println!("{logs}");
            Ok(())
        }

        Commands::Kill { id } => {
            senweavercoding::cli::bg::kill_session(&config.workspace_dir, &id).await?;
            println!("Session '{id}' terminated.");
            Ok(())
        }

        #[cfg(feature = "tui")]
        Commands::Tui { legacy } => {
            senweavercoding::tui::run_tui_standalone_with_opts(legacy).await
        }

        Commands::Gui => launch_desktop_gui(),

        #[cfg(not(feature = "tui"))]
        Commands::Tui { .. } => {
            eprintln!("The TUI feature is not enabled in this build.");
            eprintln!("Rebuild with: cargo build --features tui");
            Ok(())
        }

        Commands::Sop { sop_command } => {
            let cmd = sop_command.unwrap_or_else(|| SopCommands::List);
            sop::handle_command(cmd, &config)?;
            Ok(())
        }

        Commands::Tokens { tokens_command } => handle_tokens_command(&config, tokens_command),
    }
}

fn launch_desktop_gui() -> Result<()> {
    fn binary_name() -> &'static str {
        if cfg!(windows) {
            "sen-desktop.exe"
        } else {
            "sen-desktop"
        }
    }

    fn workspace_dev_candidate(parent: &std::path::Path, profile: &str) -> std::path::PathBuf {
        parent
            .join("..")
            .join("desktop")
            .join("src-tauri")
            .join("target")
            .join(profile)
            .join(binary_name())
    }

    let mut candidates: Vec<std::path::PathBuf> = Vec::new();

    if let Ok(current_exe) = std::env::current_exe()
        && let Some(parent) = current_exe.parent()
    {
        candidates.push(parent.join(binary_name()));
        for profile in ["release", "debug"] {
            candidates.push(workspace_dev_candidate(parent, profile));
        }
    }

    let resolved = candidates.into_iter().find(|p| p.exists()).or_else(|| {
        which::which(binary_name().trim_end_matches(".exe")).ok()
    });

    let Some(binary) = resolved else {
        eprintln!("error: `sen-desktop` binary not found.");
        eprintln!();
        eprintln!("Build it from the workspace root with:");
        eprintln!("    cd desktop && bun install && bun run tauri build");
        eprintln!();
        eprintln!(
            "Then either install the produced bundle or copy `sen-desktop` next to the `sen` binary."
        );
        std::process::exit(1);
    };

    let mut command = senweavercoding::util::hidden_sync_command(&binary);
    match command.spawn() {
        Ok(_child) => Ok(()),
        Err(e) => {
            eprintln!("error: failed to spawn `{}`: {e}", binary.display());
            std::process::exit(1);
        }
    }
}

fn handle_tokens_command(config: &Config, command: TokensCommands) -> Result<()> {
    use crate::token_saver::{self, dispatcher, tracking};

    let runtime_ctx = config.token_saver.to_runtime_ctx();
    let data_dir = runtime_ctx.data_dir.clone();

    match command {
        TokensCommands::Stats { top, json } => {
            let totals = tracking::aggregate(0, &data_dir)?;
            let per_cat = tracking::aggregate_by_category(&data_dir).unwrap_or_default();
            if json {
                #[derive(serde::Serialize)]
                struct Out<'a> {
                    totals: &'a tracking::Aggregate,
                    per_category: &'a [tracking::CategoryAggregate],
                }
                let out = Out {
                    totals: &totals,
                    per_category: &per_cat,
                };
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                let pct = if totals.tokens_before == 0 {
                    0.0
                } else {
                    (totals.tokens_saved as f64 / totals.tokens_before as f64) * 100.0
                };
                println!("Token Saver \u{1F4B0} cumulative savings");
                println!("  total commands : {}", totals.commands);
                println!("  raw tokens     : {}", totals.tokens_before);
                println!("  compacted      : {}", totals.tokens_after);
                println!("  saved          : {} ({:.1}%)", totals.tokens_saved, pct);
                println!(
                    "  database       : {}",
                    data_dir.join("token_saver").join("tracking.db").display(),
                );
                if !per_cat.is_empty() {
                    println!("\n  Top categories (limit {top}):");
                    for entry in per_cat.iter().take(top) {
                        println!(
                            "    {:<14} hits={:<5} raw={:<8} saved={:<8} ({:.1}%)",
                            entry.category,
                            entry.hits,
                            entry.raw_tokens,
                            entry.saved_tokens,
                            entry.savings_pct(),
                        );
                    }
                }
            }
            Ok(())
        }

        TokensCommands::Compact { argv, level } => {
            if argv.is_empty() {
                bail!("usage: sen tokens compact -- <command...>  (raw stdout on stdin)");
            }
            let cmd_str = argv.join(" ");
            let mut ctx = runtime_ctx.clone();
            if let Some(l) = level.as_deref() {
                ctx.level = match l.to_ascii_lowercase().as_str() {
                    "conservative" => token_saver::CompactLevel::Conservative,
                    "balanced" => token_saver::CompactLevel::Balanced,
                    "aggressive" => token_saver::CompactLevel::Aggressive,
                    other => bail!(
                        "unknown level `{other}` (expected conservative|balanced|aggressive)"
                    ),
                };
            }

            use std::io::Read;
            let mut raw = String::new();
            std::io::stdin().read_to_string(&mut raw)?;
            let result =
                token_saver::compact_command_output(&cmd_str, &raw, "", 0, &ctx);
            eprintln!(
                "[token_saver] category={} raw_bytes={} compacted_bytes={} tokens_saved={}",
                result.category.unwrap_or("passthrough"),
                raw.len(),
                result.stdout.len(),
                result.tokens_saved,
            );
            print!("{}", result.stdout);
            Ok(())
        }

        TokensCommands::Reset { yes } => {
            if !yes {
                use dialoguer::Confirm;
                let go = Confirm::new()
                    .with_prompt("Wipe the token-saver tracking database?")
                    .default(false)
                    .interact()?;
                if !go {
                    println!("Aborted.");
                    return Ok(());
                }
            }
            let n = tracking::reset(&data_dir)?;
            println!("Token-saver tracking database has been reset ({n} rows).");
            Ok(())
        }

        TokensCommands::Filters { filters_command } => match filters_command {
            TokensFiltersCommands::List => {
                println!("{:<32} {:<14} {}", "pattern", "category", "handler");
                println!("{}", "-".repeat(72));
                for (pattern, category) in dispatcher::list_rules() {
                    println!("{:<32} {:<14} {}", pattern, category, "fast-path/toml");
                }
                Ok(())
            }
        },
    }
}

fn handle_estop_command(
    config: &Config,
    estop_command: Option<EstopSubcommands>,
    level: Option<EstopLevelArg>,
    domains: Vec<String>,
    tools: Vec<String>,
) -> Result<()> {
    if !config.security.estop.enabled {
        bail!("Emergency stop is disabled. Enable [security.estop].enabled = true in config.toml");
    }

    let config_dir = config
        .config_path
        .parent()
        .context("Config path must have a parent directory")?;
    let mut manager = security::EstopManager::load(&config.security.estop, config_dir)?;

    match estop_command {
        Some(EstopSubcommands::Status) => {
            print_estop_status(&manager.status());
            Ok(())
        }
        Some(EstopSubcommands::Resume {
            network,
            domains,
            tools,
            otp,
        }) => {
            let selector = build_resume_selector(network, domains, tools)?;
            let mut otp_code = otp;
            let otp_validator = if config.security.estop.require_otp_to_resume {
                if !config.security.otp.enabled {
                    bail!(
                        "security.estop.require_otp_to_resume=true but security.otp.enabled=false"
                    );
                }
                if otp_code.is_none() {
                    let entered = Password::new()
                        .with_prompt("Enter OTP code")
                        .allow_empty_password(false)
                        .interact()?;
                    otp_code = Some(entered);
                }

                let store = security::SecretStore::new(config_dir, config.secrets.encrypt);
                let (validator, enrollment_uri) =
                    security::OtpValidator::from_config(&config.security.otp, config_dir, &store)?;
                if let Some(uri) = enrollment_uri {
                    println!("Initialized OTP secret for SenWeaverCoding.");
                    println!("Enrollment URI: {uri}");
                }
                Some(validator)
            } else {
                None
            };

            manager.resume(selector, otp_code.as_deref(), otp_validator.as_ref())?;
            println!("Estop resume completed.");
            print_estop_status(&manager.status());
            Ok(())
        }
        None => {
            let engage_level = build_engage_level(level, domains, tools)?;
            manager.engage(engage_level)?;
            println!("Estop engaged.");
            print_estop_status(&manager.status());
            Ok(())
        }
    }
}

fn build_engage_level(
    level: Option<EstopLevelArg>,
    domains: Vec<String>,
    tools: Vec<String>,
) -> Result<security::EstopLevel> {
    let requested = level.unwrap_or(EstopLevelArg::KillAll);
    match requested {
        EstopLevelArg::KillAll => {
            if !domains.is_empty() || !tools.is_empty() {
                bail!("--domain/--tool are only valid with --level domain-block/tool-freeze");
            }
            Ok(security::EstopLevel::KillAll)
        }
        EstopLevelArg::NetworkKill => {
            if !domains.is_empty() || !tools.is_empty() {
                bail!("--domain/--tool are not valid with --level network-kill");
            }
            Ok(security::EstopLevel::NetworkKill)
        }
        EstopLevelArg::DomainBlock => {
            if domains.is_empty() {
                bail!("--level domain-block requires at least one --domain");
            }
            if !tools.is_empty() {
                bail!("--tool is not valid with --level domain-block");
            }
            Ok(security::EstopLevel::DomainBlock(domains))
        }
        EstopLevelArg::ToolFreeze => {
            if tools.is_empty() {
                bail!("--level tool-freeze requires at least one --tool");
            }
            if !domains.is_empty() {
                bail!("--domain is not valid with --level tool-freeze");
            }
            Ok(security::EstopLevel::ToolFreeze(tools))
        }
    }
}

fn build_resume_selector(
    network: bool,
    domains: Vec<String>,
    tools: Vec<String>,
) -> Result<security::ResumeSelector> {
    let selected =
        usize::from(network) + usize::from(!domains.is_empty()) + usize::from(!tools.is_empty());
    if selected > 1 {
        bail!("Use only one of --network, --domain, or --tool for estop resume");
    }
    if network {
        return Ok(security::ResumeSelector::Network);
    }
    if !domains.is_empty() {
        return Ok(security::ResumeSelector::Domains(domains));
    }
    if !tools.is_empty() {
        return Ok(security::ResumeSelector::Tools(tools));
    }
    Ok(security::ResumeSelector::KillAll)
}

fn print_estop_status(state: &security::EstopState) {
    println!("Estop status:");
    println!(
        "  engaged:        {}",
        if state.is_engaged() { "yes" } else { "no" }
    );
    println!(
        "  kill_all:       {}",
        if state.kill_all { "active" } else { "inactive" }
    );
    println!(
        "  network_kill:   {}",
        if state.network_kill {
            "active"
        } else {
            "inactive"
        }
    );
    if state.blocked_domains.is_empty() {
        println!("  domain_blocks:  (none)");
    } else {
        println!("  domain_blocks:  {}", state.blocked_domains.join(", "));
    }
    if state.frozen_tools.is_empty() {
        println!("  tool_freeze:    (none)");
    } else {
        println!("  tool_freeze:    {}", state.frozen_tools.join(", "));
    }
    if let Some(updated_at) = &state.updated_at {
        println!("  updated_at:     {updated_at}");
    }
}

fn write_shell_completion<W: Write>(shell: CompletionShell, writer: &mut W) -> Result<()> {
    use clap_complete::generate;
    use clap_complete::shells;

    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();

    match shell {
        CompletionShell::Bash => generate(shells::Bash, &mut cmd, bin_name.clone(), writer),
        CompletionShell::Fish => generate(shells::Fish, &mut cmd, bin_name.clone(), writer),
        CompletionShell::Zsh => generate(shells::Zsh, &mut cmd, bin_name.clone(), writer),
        CompletionShell::PowerShell => {
            generate(shells::PowerShell, &mut cmd, bin_name.clone(), writer);
        }
        CompletionShell::Elvish => generate(shells::Elvish, &mut cmd, bin_name, writer),
    }

    writer.flush()?;
    Ok(())
}

fn resolve_gateway_addr(config: &Config, port: Option<u16>, host: Option<String>) -> (u16, String) {
    let port = port.unwrap_or(config.gateway.port);
    let host = host.unwrap_or_else(|| config.gateway.host.clone());
    (port, host)
}

fn log_gateway_start(host: &str, port: u16) {
    if port == 0 {
        info!("\u{1F680} Starting SenWeaverCoding Gateway on {host} (random port)");
    } else {
        info!("\u{1F680} Starting SenWeaverCoding Gateway on {host}:{port}");
    }
}

async fn shutdown_gateway(host: &str, port: u16) -> Result<()> {
    let url = format!("http://{host}:{port}/admin/shutdown");
    let client = reqwest::Client::new();

    match client
        .post(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => Ok(()),
        Ok(response) => Err(anyhow::anyhow!(
            "Gateway responded with status: {}",
            response.status()
        )),
        Err(e) => Err(anyhow::anyhow!("Failed to connect to gateway: {e}")),
    }
}

async fn fetch_paircode(host: &str, port: u16, new: bool) -> Result<Option<String>> {
    let client = reqwest::Client::new();

    let response = if new {

        let url = format!("http://{host}:{port}/admin/paircode/new");
        client
            .post(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
    } else {

        let url = format!("http://{host}:{port}/admin/paircode");
        client
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
    };

    let response = response.map_err(|e| anyhow::anyhow!("Failed to connect to gateway: {e}"))?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Gateway responded with status: {}",
            response.status()
        ));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse response: {e}"))?;

    if json.get("success").and_then(|v| v.as_bool()) != Some(true) {
        return Ok(None);
    }

    Ok(json
        .get("pairing_code")
        .and_then(|v| v.as_str())
        .map(String::from))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingOAuthLogin {
    provider: String,
    profile: String,
    code_verifier: String,
    state: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingOAuthLoginFile {
    #[serde(default)]
    provider: Option<String>,
    profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code_verifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encrypted_code_verifier: Option<String>,
    state: String,
    created_at: String,
}

fn pending_oauth_login_path(config: &Config, provider: &str) -> std::path::PathBuf {
    let filename = format!("auth-{}-pending.json", provider);
    auth::state_dir_from_config(config).join(filename)
}

fn pending_oauth_secret_store(config: &Config) -> security::secrets::SecretStore {
    security::secrets::SecretStore::new(
        &auth::state_dir_from_config(config),
        config.secrets.encrypt,
    )
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

fn save_pending_oauth_login(config: &Config, pending: &PendingOAuthLogin) -> Result<()> {
    let path = pending_oauth_login_path(config, &pending.provider);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let secret_store = pending_oauth_secret_store(config);
    let encrypted_code_verifier = secret_store.encrypt(&pending.code_verifier)?;
    let persisted = PendingOAuthLoginFile {
        provider: Some(pending.provider.clone()),
        profile: pending.profile.clone(),
        code_verifier: None,
        encrypted_code_verifier: Some(encrypted_code_verifier),
        state: pending.state.clone(),
        created_at: pending.created_at.clone(),
    };
    let tmp = path.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let json = serde_json::to_vec_pretty(&persisted)?;
    std::fs::write(&tmp, json)?;
    set_owner_only_permissions(&tmp)?;
    std::fs::rename(tmp, &path)?;
    set_owner_only_permissions(&path)?;
    Ok(())
}

fn load_pending_oauth_login(config: &Config, provider: &str) -> Result<Option<PendingOAuthLogin>> {
    let path = pending_oauth_login_path(config, provider);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let persisted: PendingOAuthLoginFile = serde_json::from_slice(&bytes)?;
    let secret_store = pending_oauth_secret_store(config);
    let code_verifier = if let Some(encrypted) = persisted.encrypted_code_verifier {
        secret_store.decrypt(&encrypted)?
    } else if let Some(plaintext) = persisted.code_verifier {
        plaintext
    } else {
        bail!("Pending {} login is missing code verifier", provider);
    };
    Ok(Some(PendingOAuthLogin {
        provider: persisted.provider.unwrap_or_else(|| provider.to_string()),
        profile: persisted.profile,
        code_verifier,
        state: persisted.state,
        created_at: persisted.created_at,
    }))
}

fn clear_pending_oauth_login(config: &Config, provider: &str) {
    let path = pending_oauth_login_path(config, provider);
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&path) {
        let _ = file.set_len(0);
        let _ = file.sync_all();
    }
    let _ = std::fs::remove_file(path);
}

fn read_auth_input(prompt: &str) -> Result<String> {
    let input = Password::new()
        .with_prompt(prompt)
        .allow_empty_password(false)
        .interact()?;
    Ok(input.trim().to_string())
}

fn read_plain_input(prompt: &str) -> Result<String> {
    let input: String = cli::input::Input::new()
        .with_prompt(prompt)
        .interact_text()?;
    Ok(input.trim().to_string())
}

fn extract_openai_account_id_for_profile(access_token: &str) -> Option<String> {
    let account_id = auth::openai_oauth::extract_account_id_from_jwt(access_token);
    if account_id.is_none() {
        warn!(
            "Could not extract OpenAI account id from OAuth access token; \
             requests may fail until re-authentication."
        );
    }
    account_id
}

async fn import_openai_codex_auth_profile(
    auth_service: &auth::AuthService,
    profile: &str,
    import_path: &std::path::Path,
) -> Result<()> {
    #[derive(Deserialize)]
    struct CodexAuthTokens {
        access_token: String,
        #[serde(default)]
        refresh_token: Option<String>,
        #[serde(default)]
        id_token: Option<String>,
        #[serde(default)]
        account_id: Option<String>,
    }

    #[derive(Deserialize)]
    struct CodexAuthFile {
        tokens: CodexAuthTokens,
    }

    let raw = std::fs::read_to_string(import_path)
        .with_context(|| format!("Failed to read import file {}", import_path.display()))?;
    let imported: CodexAuthFile = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse import file {}", import_path.display()))?;
    let expires_at = auth::openai_oauth::extract_expiry_from_jwt(&imported.tokens.access_token);

    let token_set = auth::profiles::TokenSet {
        access_token: imported.tokens.access_token,
        refresh_token: imported.tokens.refresh_token,
        id_token: imported.tokens.id_token,
        expires_at,
        token_type: Some("Bearer".to_string()),
        scope: None,
    };

    let account_id = imported
        .tokens
        .account_id
        .or_else(|| extract_openai_account_id_for_profile(&token_set.access_token));

    auth_service
        .store_openai_tokens(profile, token_set, account_id, true)
        .await?;

    Ok(())
}

fn format_expiry(profile: &auth::profiles::AuthProfile) -> String {
    match profile
        .token_set
        .as_ref()
        .and_then(|token_set| token_set.expires_at)
    {
        Some(ts) => {
            let now = chrono::Utc::now();
            if ts <= now {
                format!("expired at {}", ts.to_rfc3339())
            } else {
                let mins = (ts - now).num_minutes();
                format!("expires in {mins}m ({})", ts.to_rfc3339())
            }
        }
        None => "n/a".to_string(),
    }
}

#[allow(clippy::too_many_lines)]
async fn handle_auth_command(auth_command: AuthCommands, config: &Config) -> Result<()> {
    let auth_service = auth::AuthService::from_config(config);

    match auth_command {
        AuthCommands::Login {
            provider,
            profile,
            device_code,
            import,
        } => {
            let provider = auth::normalize_provider(&provider)?;
            if import.is_some() && provider != "openai-codex" {
                bail!("`auth login --import` currently supports only --provider openai-codex");
            }
            let client = reqwest::Client::new();

            match provider.as_str() {
                "gemini" => {

                    if device_code {
                        match auth::gemini_oauth::start_device_code_flow(&client).await {
                            Ok(device) => {
                                println!("Google/Gemini device-code login started.");
                                println!("Visit: {}", device.verification_uri);
                                println!("Code:  {}", device.user_code);
                                if let Some(uri_complete) = &device.verification_uri_complete {
                                    println!("Fast link: {uri_complete}");
                                }

                                let token_set =
                                    auth::gemini_oauth::poll_device_code_tokens(&client, &device)
                                        .await?;
                                let account_id = token_set.id_token.as_deref().and_then(
                                    auth::gemini_oauth::extract_account_email_from_id_token,
                                );

                                auth_service
                                    .store_gemini_tokens(&profile, token_set, account_id, true)
                                    .await?;

                                println!("Saved profile {profile}");
                                println!("Active profile for gemini: {profile}");
                                return Ok(());
                            }
                            Err(e) => {
                                println!(
                                    "Device-code flow unavailable: {e}. Falling back to browser flow."
                                );
                            }
                        }
                    }

                    let pkce = auth::gemini_oauth::generate_pkce_state();
                    let authorize_url = auth::gemini_oauth::build_authorize_url(&pkce)?;

                    let pending = PendingOAuthLogin {
                        provider: "gemini".to_string(),
                        profile: profile.clone(),
                        code_verifier: pkce.code_verifier.clone(),
                        state: pkce.state.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                    };
                    save_pending_oauth_login(config, &pending)?;

                    println!("Open this URL in your browser and authorize access:");
                    println!("{authorize_url}");
                    println!();

                    let code = match auth::gemini_oauth::receive_loopback_code(
                        &pkce.state,
                        std::time::Duration::from_secs(180),
                    )
                    .await
                    {
                        Ok(code) => {
                            clear_pending_oauth_login(config, "gemini");
                            code
                        }
                        Err(e) => {
                            println!("Callback capture failed: {e}");
                            println!(
                                "Run `sen auth paste-redirect --provider gemini --profile {profile}`"
                            );
                            return Ok(());
                        }
                    };

                    let token_set =
                        auth::gemini_oauth::exchange_code_for_tokens(&client, &code, &pkce).await?;
                    let account_id = token_set
                        .id_token
                        .as_deref()
                        .and_then(auth::gemini_oauth::extract_account_email_from_id_token);

                    auth_service
                        .store_gemini_tokens(&profile, token_set, account_id, true)
                        .await?;

                    println!("Saved profile {profile}");
                    println!("Active profile for gemini: {profile}");
                    Ok(())
                }
                "openai-codex" => {
                    if let Some(import_path) = import.as_deref() {
                        import_openai_codex_auth_profile(&auth_service, &profile, import_path)
                            .await?;
                        println!("Imported auth profile from {}", import_path.display());
                        println!("Active profile for openai-codex: {profile}");
                        return Ok(());
                    }

                    if device_code {
                        match auth::openai_oauth::start_device_code_flow(&client).await {
                            Ok(device) => {
                                println!("OpenAI device-code login started.");
                                println!("Visit: {}", device.verification_uri);
                                println!("Code:  {}", device.user_code);
                                if let Some(uri_complete) = &device.verification_uri_complete {
                                    println!("Fast link: {uri_complete}");
                                }
                                if let Some(message) = &device.message {
                                    println!("{message}");
                                }

                                let token_set =
                                    auth::openai_oauth::poll_device_code_tokens(&client, &device)
                                        .await?;
                                let account_id =
                                    extract_openai_account_id_for_profile(&token_set.access_token);

                                auth_service
                                    .store_openai_tokens(&profile, token_set, account_id, true)
                                    .await?;
                                clear_pending_oauth_login(config, "openai");

                                println!("Saved profile {profile}");
                                println!("Active profile for openai-codex: {profile}");
                                return Ok(());
                            }
                            Err(e) => {
                                println!(
                                    "Device-code flow unavailable: {e}. Falling back to browser/paste flow."
                                );
                            }
                        }
                    }

                    let pkce = auth::openai_oauth::generate_pkce_state();
                    let pending = PendingOAuthLogin {
                        provider: "openai".to_string(),
                        profile: profile.clone(),
                        code_verifier: pkce.code_verifier.clone(),
                        state: pkce.state.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                    };
                    save_pending_oauth_login(config, &pending)?;

                    let authorize_url = auth::openai_oauth::build_authorize_url(&pkce);
                    println!("Open this URL in your browser and authorize access:");
                    println!("{authorize_url}");
                    println!();
                    println!("Waiting for callback at http://localhost:1455/auth/callback ...");

                    let code = match auth::openai_oauth::receive_loopback_code(
                        &pkce.state,
                        std::time::Duration::from_secs(180),
                    )
                    .await
                    {
                        Ok(code) => code,
                        Err(e) => {
                            println!("Callback capture failed: {e}");
                            println!(
                                "Run `sen auth paste-redirect --provider openai-codex --profile {profile}`"
                            );
                            return Ok(());
                        }
                    };

                    let token_set =
                        auth::openai_oauth::exchange_code_for_tokens(&client, &code, &pkce).await?;
                    let account_id = extract_openai_account_id_for_profile(&token_set.access_token);

                    auth_service
                        .store_openai_tokens(&profile, token_set, account_id, true)
                        .await?;
                    clear_pending_oauth_login(config, "openai");

                    println!("Saved profile {profile}");
                    println!("Active profile for openai-codex: {profile}");
                    Ok(())
                }
                _ => {
                    bail!(
                        "`auth login` supports --provider openai-codex or gemini, got: {provider}"
                    );
                }
            }
        }

        AuthCommands::PasteRedirect {
            provider,
            profile,
            input,
        } => {
            let provider = auth::normalize_provider(&provider)?;

            match provider.as_str() {
                "openai-codex" => {
                    let pending = load_pending_oauth_login(config, "openai")?.ok_or_else(|| {
                        anyhow::anyhow!(
                            "No pending OpenAI login found. Run `sen auth login --provider openai-codex` first."
                        )
                    })?;

                    if pending.profile != profile {
                        bail!(
                            "Pending login profile mismatch: pending={}, requested={}",
                            pending.profile,
                            profile
                        );
                    }

                    let redirect_input = match input {
                        Some(value) => value,
                        None => read_plain_input("Paste redirect URL or OAuth code")?,
                    };

                    let code = auth::openai_oauth::parse_code_from_redirect(
                        &redirect_input,
                        Some(&pending.state),
                    )?;

                    let pkce = auth::openai_oauth::PkceState {
                        code_verifier: pending.code_verifier.clone(),
                        code_challenge: String::new(),
                        state: pending.state.clone(),
                    };

                    let client = reqwest::Client::new();
                    let token_set =
                        auth::openai_oauth::exchange_code_for_tokens(&client, &code, &pkce).await?;
                    let account_id = extract_openai_account_id_for_profile(&token_set.access_token);

                    auth_service
                        .store_openai_tokens(&profile, token_set, account_id, true)
                        .await?;
                    clear_pending_oauth_login(config, "openai");

                    println!("Saved profile {profile}");
                    println!("Active profile for openai-codex: {profile}");
                }
                "gemini" => {
                    let pending = load_pending_oauth_login(config, "gemini")?.ok_or_else(|| {
                        anyhow::anyhow!(
                            "No pending Gemini login found. Run `sen auth login --provider gemini` first."
                        )
                    })?;

                    if pending.profile != profile {
                        bail!(
                            "Pending login profile mismatch: pending={}, requested={}",
                            pending.profile,
                            profile
                        );
                    }

                    let redirect_input = match input {
                        Some(value) => value,
                        None => read_plain_input("Paste redirect URL or OAuth code")?,
                    };

                    let code = auth::gemini_oauth::parse_code_from_redirect(
                        &redirect_input,
                        Some(&pending.state),
                    )?;

                    let pkce = auth::gemini_oauth::PkceState {
                        code_verifier: pending.code_verifier.clone(),
                        code_challenge: String::new(),
                        state: pending.state.clone(),
                    };

                    let client = reqwest::Client::new();
                    let token_set =
                        auth::gemini_oauth::exchange_code_for_tokens(&client, &code, &pkce).await?;
                    let account_id = token_set
                        .id_token
                        .as_deref()
                        .and_then(auth::gemini_oauth::extract_account_email_from_id_token);

                    auth_service
                        .store_gemini_tokens(&profile, token_set, account_id, true)
                        .await?;
                    clear_pending_oauth_login(config, "gemini");

                    println!("Saved profile {profile}");
                    println!("Active profile for gemini: {profile}");
                }
                _ => {
                    bail!("`auth paste-redirect` supports --provider openai-codex or gemini");
                }
            }
            Ok(())
        }

        AuthCommands::PasteToken {
            provider,
            profile,
            token,
            auth_kind,
        } => {
            let provider = auth::normalize_provider(&provider)?;
            let token = match token {
                Some(token) => token.trim().to_string(),
                None => read_auth_input("Paste token")?,
            };
            if token.is_empty() {
                bail!("Token cannot be empty");
            }

            let kind = auth::anthropic_token::detect_auth_kind(&token, auth_kind.as_deref());
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "auth_kind".to_string(),
                kind.as_metadata_value().to_string(),
            );

            auth_service
                .store_provider_token(&provider, &profile, &token, metadata, true)
                .await?;
            println!("Saved profile {profile}");
            println!("Active profile for {provider}: {profile}");
            Ok(())
        }

        AuthCommands::SetupToken { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;
            let token = read_auth_input("Paste token")?;
            if token.is_empty() {
                bail!("Token cannot be empty");
            }

            let kind = auth::anthropic_token::detect_auth_kind(&token, Some("authorization"));
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "auth_kind".to_string(),
                kind.as_metadata_value().to_string(),
            );

            auth_service
                .store_provider_token(&provider, &profile, &token, metadata, true)
                .await?;
            println!("Saved profile {profile}");
            println!("Active profile for {provider}: {profile}");
            Ok(())
        }

        AuthCommands::Refresh { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;

            match provider.as_str() {
                "openai-codex" => {
                    match auth_service
                        .get_valid_openai_access_token(profile.as_deref())
                        .await?
                    {
                        Some(_) => {
                            println!("OpenAI Codex token is valid (refresh completed if needed).");
                            Ok(())
                        }
                        None => {
                            bail!(
                                "No OpenAI Codex auth profile found. Run `sen auth login --provider openai-codex`."
                            )
                        }
                    }
                }
                "gemini" => {
                    match auth_service
                        .get_valid_gemini_access_token(profile.as_deref())
                        .await?
                    {
                        Some(_) => {
                            let profile_name = profile.as_deref().unwrap_or("default");
                            println!("\u{2705} Gemini token refreshed successfully");
                            println!("  Profile: gemini:{}", profile_name);
                            Ok(())
                        }
                        None => {
                            bail!(
                                "No Gemini auth profile found. Run `sen auth login --provider gemini`."
                            )
                        }
                    }
                }
                _ => bail!("`auth refresh` supports --provider openai-codex or gemini"),
            }
        }

        AuthCommands::Logout { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;
            let removed = auth_service.remove_profile(&provider, &profile).await?;
            if removed {
                println!("Removed auth profile {provider}:{profile}");
            } else {
                println!("Auth profile not found: {provider}:{profile}");
            }
            Ok(())
        }

        AuthCommands::Use { provider, profile } => {
            let provider = auth::normalize_provider(&provider)?;
            auth_service.set_active_profile(&provider, &profile).await?;
            println!("Active profile for {provider}: {profile}");
            Ok(())
        }

        AuthCommands::List => {
            let data = auth_service.load_profiles().await?;
            if data.profiles.is_empty() {
                println!("No auth profiles configured.");
                return Ok(());
            }

            for (id, profile) in &data.profiles {
                let active = data
                    .active_profiles
                    .get(&profile.provider)
                    .is_some_and(|active_id| active_id == id);
                let marker = if active { "*" } else { " " };
                println!("{marker} {id}");
            }

            Ok(())
        }

        AuthCommands::Status => {
            let data = auth_service.load_profiles().await?;
            if data.profiles.is_empty() {
                println!("No auth profiles configured.");
                return Ok(());
            }

            for (id, profile) in &data.profiles {
                let active = data
                    .active_profiles
                    .get(&profile.provider)
                    .is_some_and(|active_id| active_id == id);
                let marker = if active { "*" } else { " " };
                println!(
                    "{} {} kind={:?} account={} expires={}",
                    marker,
                    id,
                    profile.kind,
                    crate::security::redact(profile.account_id.as_deref().unwrap_or("unknown")),
                    format_expiry(profile)
                );
            }

            println!();
            println!("Active profiles:");
            for (provider, profile_id) in &data.active_profiles {
                println!("  {provider}: {profile_id}");
            }

            Ok(())
        }
    }
}

fn get_config_value(config: &Config, key: &str) -> Result<()> {
    let parts: Vec<&str> = key.split('.').collect();
    if parts.is_empty() {
        bail!("Key cannot be empty");
    }

    match key {
        "api_key" => {
            if let Some(ref api_key) = config.api_key {

                if api_key.len() > 8 {
                    println!("{}...{}", &api_key[..4], &api_key[api_key.len() - 4..]);
                } else {
                    println!("[API key set]");
                }
            } else {
                println!("(not set)");
            }
            return Ok(());
        }
        "default_provider" => {
            println!(
                "{}",
                config.default_provider.as_deref().unwrap_or("(not set)")
            );
            return Ok(());
        }
        "default_model" => {
            println!("{}", config.default_model.as_deref().unwrap_or("(not set)"));
            return Ok(());
        }
        "default_temperature" => {
            println!("{}", config.default_temperature);
            return Ok(());
        }
        "provider_timeout_secs" => {
            println!("{}", config.provider_timeout_secs);
            return Ok(());
        }
        "api_url" => {
            println!("{}", config.api_url.as_deref().unwrap_or("(not set)"));
            return Ok(());
        }
        "gateway.host" => {
            println!("{}", config.gateway.host);
            return Ok(());
        }
        "gateway.port" => {
            println!("{}", config.gateway.port);
            return Ok(());
        }
        "workspace_dir" => {
            println!("{}", config.workspace_dir.display());
            return Ok(());
        }
        "config_path" => {
            println!("{}", config.config_path.display());
            return Ok(());
        }
        _ => {}
    }

    match parts[0] {
        "gateway" => {
            if parts.len() == 2 {
                match parts[1] {
                    "host" => println!("{}", config.gateway.host),
                    "port" => println!("{}", config.gateway.port),
                    "require_pairing" => println!("{}", config.gateway.require_pairing),
                    _ => bail!("Unknown gateway key: {}", parts[1]),
                }
            } else {
                bail!("Invalid key format for gateway: {}", key);
            }
        }
        "runtime" => {
            if parts.len() == 2 {
                match parts[1] {
                    "reasoning_enabled" => println!("{:?}", config.runtime.reasoning_enabled),
                    "reasoning_effort" => println!("{:?}", config.runtime.reasoning_effort),
                    _ => bail!("Unknown runtime key: {}", parts[1]),
                }
            } else {
                bail!("Invalid key format for runtime: {}", key);
            }
        }
        "memory" => {
            if parts.len() == 2 {
                match parts[1] {
                    "backend" => println!("{}", config.memory.backend),
                    "auto_save" => println!("{}", config.memory.auto_save),
                    "embedding_provider" => println!("{}", config.memory.embedding_provider),
                    _ => bail!("Unknown memory key: {}", parts[1]),
                }
            } else {
                bail!("Invalid key format for memory: {}", key);
            }
        }
        "web_search" => {
            if parts.len() == 2 {
                match parts[1] {
                    "enabled" => println!("{}", config.web_search.enabled),
                    "provider" => println!("{}", config.web_search.provider),
                    "max_results" => println!("{}", config.web_search.max_results),
                    "timeout_secs" => println!("{}", config.web_search.timeout_secs),
                    "brave_api_key" => {
                        if let Some(ref key) = config.web_search.brave_api_key {
                            if key.len() > 8 {
                                println!("{}...{}", &key[..4], &key[key.len() - 4..]);
                            } else {
                                println!("[API key set]");
                            }
                        } else {
                            println!("(not set)");
                        }
                    }
                    _ => bail!("Unknown web_search key: {}", parts[1]),
                }
            } else {
                bail!("Invalid key format for web_search: {}", key);
            }
        }
        _ => bail!(
            "Unknown top-level config key: {}. Try 'sen config list' to see available keys.",
            parts[0]
        ),
    }
    Ok(())
}

async fn set_config_value(config: &Config, key: &str, value: &str) -> Result<()> {
    let mut cfg = Config::load_or_init()
        .await
        .context("Failed to load config")?;

    match key {
        "default_provider" | "provider" => cfg.default_provider = Some(value.to_string()),
        "default_model" | "model" => cfg.default_model = Some(value.to_string()),
        "api_key" => cfg.api_key = Some(value.to_string()),
        "api_url" => cfg.api_url = Some(value.to_string()),
        "default_temperature" | "temperature" => {
            cfg.default_temperature = value
                .parse::<f64>()
                .with_context(|| format!("Invalid temperature value: {value}"))?;
        }
        "provider_timeout_secs" | "timeout" => {
            cfg.provider_timeout_secs = value
                .parse::<u64>()
                .with_context(|| format!("Invalid timeout value: {value}"))?;
        }
        "gateway.host" => cfg.gateway.host = value.to_string(),
        "gateway.port" => {
            cfg.gateway.port = value
                .parse::<u16>()
                .with_context(|| format!("Invalid port value: {value}"))?;
        }
        "memory.backend" => cfg.memory.backend = value.to_string(),
        "memory.auto_save" => {
            cfg.memory.auto_save = value
                .parse::<bool>()
                .with_context(|| format!("Invalid bool value: {value}"))?;
        }
        "web_search.enabled" => {
            cfg.web_search.enabled = value
                .parse::<bool>()
                .with_context(|| format!("Invalid bool value: {value}"))?;
        }
        "web_search.provider" => cfg.web_search.provider = value.to_string(),
        _ => bail!(
            "Unknown or read-only config key: {key}\n\
             Run 'sen config list --keys-only' to see available keys.\n\
             Config file: {}",
            config.config_path.display()
        ),
    }

    cfg.save().await.context("Failed to save config")?;
    println!(
        "Set {key} = {value} (saved to {})",
        config.config_path.display()
    );
    Ok(())
}

fn list_config_values(config: &Config, keys_only: bool) -> Result<()> {
    println!("Configuration at: {}", config.config_path.display());
    println!("Workspace: {}", config.workspace_dir.display());
    println!();

    if keys_only {
        println!("Top-level keys:");
        println!("  default_provider, default_model, api_key, api_url");
        println!("  default_temperature, provider_timeout_secs, provider_max_tokens");
        println!("  gateway.host, gateway.port, gateway.require_pairing");
        println!("  runtime.reasoning_enabled, runtime.reasoning_effort");
        println!("  memory.backend, memory.auto_save, memory.embedding_provider");
        println!("  web_search.enabled, web_search.provider, web_search.max_results");
        println!("  workspace_dir, config_path");
        println!();
        println!("Nested keys (use dot notation):");
        println!("  gateway.<key>, runtime.<key>, memory.<key>, web_search.<key>");
        return Ok(());
    }

    println!("Provider Settings:");
    println!(
        "  default_provider     = {}",
        config.default_provider.as_deref().unwrap_or("(not set)")
    );
    println!(
        "  default_model       = {}",
        config.default_model.as_deref().unwrap_or("(not set)")
    );
    println!("  api_key             = [hidden - use 'sen config get api_key' to reveal]");
    println!(
        "  api_url             = {}",
        config.api_url.as_deref().unwrap_or("(not set)")
    );
    println!("  default_temperature = {}", config.default_temperature);
    println!("  provider_timeout_secs = {}", config.provider_timeout_secs);
    if let Some(max_tokens) = config.provider_max_tokens {
        println!("  provider_max_tokens = {}", max_tokens);
    } else {
        println!("  provider_max_tokens = (not set / no limit)");
    }
    println!();

    println!("Gateway Settings:");
    println!("  gateway.host              = {}", config.gateway.host);
    println!("  gateway.port              = {}", config.gateway.port);
    println!(
        "  gateway.require_pairing   = {}",
        config.gateway.require_pairing
    );
    println!();

    println!("Runtime Settings:");
    println!(
        "  runtime.reasoning_enabled = {:?}",
        config.runtime.reasoning_enabled
    );
    println!(
        "  runtime.reasoning_effort  = {:?}",
        config.runtime.reasoning_effort
    );
    println!();

    println!("Memory Settings:");
    println!("  memory.backend           = {}", config.memory.backend);
    println!("  memory.auto_save         = {}", config.memory.auto_save);
    println!(
        "  memory.embedding_provider = {}",
        config.memory.embedding_provider
    );
    println!();

    println!("Web Search:");
    println!("  web_search.enabled       = {}", config.web_search.enabled);
    println!(
        "  web_search.provider      = {}",
        config.web_search.provider
    );
    println!(
        "  web_search.max_results   = {}",
        config.web_search.max_results
    );
    println!();

    println!("Provider Profiles ({})", config.model_providers.len());
    for (name, profile) in &config.model_providers {
        println!("  {}:", name);
        if let Some(ref base_url) = profile.base_url {
            println!("    base_url = {}", base_url);
        }
        if let Some(ref api_path) = profile.api_path {
            println!("    api_path = {}", api_path);
        }
        if let Some(max_tokens) = profile.max_tokens {
            println!("    max_tokens = {}", max_tokens);
        }
    }
    println!();

    println!("Model Routes ({})", config.model_routes.len());
    for route in &config.model_routes {
        println!("  {} -> {}", route.hint, route.model);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_inline_complete_command(
    config: &Config,
    prefix: Option<String>,
    suffix: String,
    language: Option<String>,
    file_path: Option<PathBuf>,
    max_tokens: u32,
    top_k: u32,
    stop_sequences: Vec<String>,
    stream: bool,
) -> Result<()> {
    use std::io::Read;

    let prefix = match prefix {
        Some(p) => p,
        None => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };

    let registry = {
        let cfg = config.clone();
        tokio::task::spawn_blocking(move || inline_completion::registry::default_provider(&cfg))
            .await
            .ok()
            .flatten()
    }
    .ok_or_else(|| {
        anyhow::anyhow!(
            "inline completion is disabled: no provider configured. Run `sen onboard` first."
        )
    })?;

    let language_label = language.as_deref().unwrap_or("");
    let language_kind = parse_inline_language(language_label);
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let file_path = file_path.unwrap_or_else(|| workspace_root.join("<scratch>"));
    let context = inline_completion::context_builder::build_context_from_window(&prefix, &suffix);

    let req = inline_completion::InlineCompletionRequest {
        prefix,
        suffix,
        language: language_kind,
        file_path,
        workspace_root,
        context,
        max_tokens,
        stop_sequences,
        request_id: uuid::Uuid::new_v4(),
    };

    if stream {

        let resp = registry.request(req).await?;
        for s in &resp.suggestions {
            print!("{}", s.insert_text);
        }
        println!();
        return Ok(());
    }

    match registry.request(req).await {
        Ok(resp) => {
            let suggestions: Vec<serde_json::Value> = resp
                .suggestions
                .iter()
                .take(top_k.max(1) as usize)
                .map(|s| {
                    serde_json::json!({
                        "insert_text": s.insert_text,
                        "rationale": s.rationale,
                        "confidence": s.confidence,
                    })
                })
                .collect();
            let payload = serde_json::json!({
                "provider": resp.provider,
                "latency_ms": resp.latency_ms,
                "cached": resp.cached,
                "suggestions": suggestions,
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
            Ok(())
        }
        Err(inline_completion::InlineCompletionError::Empty { provider }) => {
            println!(
                "{}",
                serde_json::json!({
                    "provider": provider,
                    "latency_ms": 0,
                    "cached": false,
                    "suggestions": Vec::<serde_json::Value>::new(),
                })
            );
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!(e.to_string())),
    }
}

async fn run_inline_edit_command(
    config: &Config,
    file: PathBuf,
    instruction: String,
    apply: bool,
    show_applied: bool,
) -> Result<()> {
    let runner = {
        let cfg = config.clone();
        tokio::task::spawn_blocking(move || inline_edit::service::default_runner(&cfg))
            .await
            .ok()
            .flatten()
    }
    .ok_or_else(|| {
        anyhow::anyhow!(
            "inline-edit runner unavailable: no provider configured. Run `sen onboard` first."
        )
    })?;

    let source = tokio::fs::read_to_string(&file).await.map_err(|e| {
        anyhow::anyhow!("failed to read source file {}: {e}", file.display())
    })?;

    let len = source.len();
    let req = inline_edit::InlineEditRequest {
        file_path: file.clone(),
        selection: source.clone(),
        selection_bytes: (0, len),
        instruction,
        context_lines: None,
        request_id: uuid::Uuid::new_v4(),
    };

    let outcome = runner
        .run(&source, req)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let payload = serde_json::json!({
        "file": file.display().to_string(),
        "diff": outcome.diff,
        "hunks_exact": outcome.hunks_exact,
        "hunks_fuzzy": outcome.hunks_fuzzy,
        "validator_issues": outcome.validator_issues,
        "checkpoint_id": outcome.checkpoint_id,
        "applied_to_disk": apply,
        "applied_contents": if show_applied {
            serde_json::Value::String(outcome.applied.clone())
        } else {
            serde_json::Value::Null
        },
    });

    if apply {
        if let Some(parent) = file.parent()
            && !parent.as_os_str().is_empty()
        {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        tokio::fs::write(&file, outcome.applied.as_bytes())
            .await
            .map_err(|e| {
                anyhow::anyhow!("failed to write {}: {e}", file.display())
            })?;
    }

    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

async fn run_predict_next_command(
    config: &Config,
    file: PathBuf,
    cursor_line: Option<u32>,
    recent_diff: Option<PathBuf>,
    apply: bool,
) -> Result<()> {
    let source = tokio::fs::read_to_string(&file).await.map_err(|e| {
        anyhow::anyhow!("failed to read source file {}: {e}", file.display())
    })?;
    let total_lines = source.lines().count() as u32;
    let cursor = cursor_line.unwrap_or(total_lines.max(1));

    let mut recent_edits = Vec::new();
    if let Some(path) = recent_diff.as_ref() {
        let diff = tokio::fs::read_to_string(path).await.map_err(|e| {
            anyhow::anyhow!("failed to read recent diff {}: {e}", path.display())
        })?;
        recent_edits.push(inline_completion::nep::RecentEdit {
            file_path: file.clone(),
            diff,
            instruction: None,
            since_start_ms: 0,
        });
    }

    let req = inline_completion::nep::NepRequest {
        active_file: file.clone(),
        source,
        cursor_line: cursor,
        recent_edits,
        workspace_root: config.workspace_dir.clone(),
        request_id: uuid::Uuid::new_v4(),
    };

    let registry = {
        let cfg = config.clone();
        tokio::task::spawn_blocking(move || {
            inline_completion::nep::registry::default_registry(&cfg)
        })
        .await
        .map_err(|e| anyhow::anyhow!("nep registry init task failed: {e}"))?
    };
    let response = registry
        .predict(req)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let suggestion = response.suggestions.first();
    let payload = serde_json::json!({
        "file": file.display().to_string(),
        "provider": response.provider,
        "latency_ms": response.latency_ms,
        "diff": suggestion.map(|s| s.diff.clone()).unwrap_or_default(),
        "rationale": suggestion.map(|s| s.rationale.clone()).unwrap_or_default(),
        "confidence": suggestion.and_then(|s| s.confidence),
        "applied_to_disk": apply && suggestion.is_some(),
    });

    if apply
        && let Some(suggestion) = suggestion
    {
        let opts = crate::apply_model::ApplyOptions::default();
        let refiner = {
            let cfg = config.clone();
            tokio::task::spawn_blocking(move || inline_edit::service::default_fast_refiner(&cfg))
                .await
                .ok()
                .flatten()
        };
        let refiner_ref: Option<&crate::apply_model::FastApplyRefiner> =
            refiner.as_deref();
        let _ = inline_completion::nep::apply_suggestion(suggestion, refiner_ref, &opts)
            .await?;
    }

    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

async fn run_team_command(config: &Config, action: TeamAction) -> Result<()> {
    use std::sync::Arc;
    use std::time::Duration;

    use crate::agent::role_pipeline::{
        self, PipelineParams, RolePipeline,
    };
    use crate::memory::blackboard::Blackboard;

    let pick_pipeline = |name: &str| -> Result<RolePipeline> {
        match name {
            "default" => Ok(role_pipeline::default_pipeline()),
            other => Err(anyhow::anyhow!(
                "unknown pipeline `{other}`; today only `default` is registered"
            )),
        }
    };

    match action {
        TeamAction::List => {
            let names = ["default"];
            let listing: Vec<serde_json::Value> = names
                .iter()
                .map(|n| {
                    let p = role_pipeline::default_pipeline();
                    serde_json::json!({
                        "name": n,
                        "stages": p.stages.iter().map(|s| {
                            serde_json::json!({
                                "id": s.id,
                                "label": s.label,
                                "depends_on": s.depends_on,
                            })
                        }).collect::<Vec<_>>(),
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "pipelines": listing
                }))?
            );
            Ok(())
        }
        TeamAction::Run {
            goal,
            pipeline,
            temperature,
            stage_timeout_secs,
            json,
        } => {
            let pipeline_obj = pick_pipeline(&pipeline)?;
            pipeline_obj.validate().map_err(|e| {
                anyhow::anyhow!("pipeline `{pipeline}` failed validation: {e}")
            })?;

            let provider_name = config
                .default_provider
                .clone()
                .unwrap_or_else(|| "openrouter".to_string());
            let resolved_provider_name =
                crate::providers::resolve_runtime_provider_name(&provider_name, config);
            let model = crate::providers::resolve_default_model(config)?;
            let provider = crate::providers::create_provider_with_url_async(
                resolved_provider_name,
                config.api_key.clone(),
                config.api_url.clone(),
            )
            .await
            .map_err(|e| {
                anyhow::anyhow!("failed to build provider `{provider_name}`: {e}")
            })?;

            let mut params = PipelineParams {
                provider_name: provider_name.clone(),
                model,
                temperature: config.default_temperature,
                ..PipelineParams::default()
            };
            if let Some(t) = temperature {
                params.temperature = t;
            }
            if let Some(secs) = stage_timeout_secs {
                params.stage_timeout = Duration::from_secs(secs);
            }

            let blackboard = Arc::new(Blackboard::new());
            let report = pipeline_obj
                .run(&goal, provider.as_ref(), blackboard, params)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;

            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                for stage in &report.stages {
                    let status = if stage.success { "ok" } else { "fail" };
                    eprintln!(
                        "[{}] {} ({}ms){}",
                        status,
                        stage.label,
                        stage.elapsed_ms,
                        stage
                            .error
                            .as_deref()
                            .map(|e| format!(" \u{2717} {e}"))
                            .unwrap_or_default(),
                    );
                }
                println!("{}", report.final_answer);
            }
            Ok(())
        }
    }
}

async fn run_mcp_command(config: &Config, action: McpAction) -> Result<()> {
    use senweavercoding::entrypoints::mcp_server::{
        McpServerConfig, McpServerEntrypoint, McpServerTransport,
    };

    let McpAction::Serve {
        transport,
        bind,
        allow,
        deny,
        list_tools,
    } = action;

    let transport = match transport.trim().to_ascii_lowercase().as_str() {
        "stdio" => McpServerTransport::Stdio,
        "sse" => McpServerTransport::Sse,
        "streamable" | "http" | "streamable-http" => {
            McpServerTransport::Streamable
        }
        other => {
            return Err(anyhow::anyhow!(
                "unknown --transport `{other}`; expected stdio | sse | streamable"
            ));
        }
    };

    let bind = match bind {
        Some(raw) => Some(raw.parse::<std::net::SocketAddr>().map_err(|e| {
            anyhow::anyhow!("invalid --bind `{raw}`: {e}")
        })?),
        None => None,
    };

    let server_config = McpServerConfig {
        transport,
        cwd: config.workspace_dir.clone(),
        bind,
        allowed_tools: allow,
        denied_tools: deny,
    };

    if list_tools {
        let exposed = McpServerEntrypoint::list_default_tools(&server_config);
        let payload = serde_json::json!({
            "transport": format!("{:?}", server_config.transport).to_lowercase(),
            "bind": server_config.bind.map(|b| b.to_string()),
            "exposed_tools": exposed,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    McpServerEntrypoint::run_default(server_config).await
}

fn parse_inline_language(label: &str) -> inline_completion::Language {
    use inline_completion::Language;
    match label.trim().to_ascii_lowercase().as_str() {
        "rust" | "rs" => Language::Rust,
        "typescript" | "ts" | "tsx" => Language::TypeScript,
        "javascript" | "js" | "jsx" => Language::JavaScript,
        "python" | "py" => Language::Python,
        "go" | "golang" => Language::Go,
        "java" => Language::Java,
        "c++" | "cpp" | "cxx" | "cc" => Language::Cpp,
        "c" => Language::C,
        "csharp" | "cs" | "c#" => Language::CSharp,
        "ruby" | "rb" => Language::Ruby,
        "php" => Language::Php,
        "swift" => Language::Swift,
        "kotlin" | "kt" => Language::Kotlin,
        "scala" => Language::Scala,
        "shell" | "sh" | "bash" | "zsh" | "powershell" | "ps1" => Language::Shell,
        "html" => Language::Html,
        "css" => Language::Css,
        "json" => Language::Json,
        "yaml" | "yml" => Language::Yaml,
        "toml" => Language::Toml,
        "markdown" | "md" => Language::Markdown,
        "sql" => Language::Sql,
        _ => Language::Other,
    }
}
