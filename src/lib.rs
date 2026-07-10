// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
#![recursion_limit = "512"]
#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::assigning_clones,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::needless_return,
    clippy::collapsible_if,
    clippy::collapsible_else_if,
    clippy::default_trait_access,
    clippy::derivable_impls,
    clippy::doc_markdown,
    clippy::enum_glob_use,
    clippy::explicit_iter_loop,
    clippy::format_push_string,
    clippy::if_not_else,
    clippy::if_same_then_else,
    clippy::ignored_unit_patterns,
    clippy::implicit_clone,
    clippy::items_after_statements,
    clippy::large_futures,
    clippy::let_and_return,
    clippy::manual_let_else,
    clippy::manual_strip,
    clippy::map_unwrap_or,
    clippy::match_same_arms,
    clippy::match_wildcard_for_single_variants,
    clippy::missing_const_for_fn,
    clippy::missing_errors_doc,
    clippy::missing_fields_in_debug,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::multiple_crate_versions,
    clippy::must_use_candidate,
    clippy::needless_bool,
    clippy::needless_continue,
    clippy::needless_pass_by_value,
    clippy::new_without_default,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::ref_option,
    clippy::return_self_not_must_use,
    clippy::semicolon_if_nothing_returned,
    clippy::shadow_unrelated,
    clippy::similar_names,
    clippy::single_match_else,
    clippy::struct_excessive_bools,
    clippy::struct_field_names,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::uninlined_format_args,
    clippy::unnecessary_cast,
    clippy::unnecessary_literal_bound,
    clippy::unnecessary_map_or,
    clippy::unnecessary_wraps,
    clippy::unnested_or_patterns,
    clippy::unused_async,
    clippy::unused_self,
    clippy::unwrap_or_default,
    clippy::used_underscore_binding,
    clippy::wildcard_imports,
)]

use clap::Subcommand;
use serde::{Deserialize, Serialize};

pub mod agent;
pub mod session;

pub mod approval;
pub mod auth;
pub mod bench_diff;
pub mod channels;
pub mod cli;

pub mod cli_entry;
pub use cli::input::Input;
pub mod commands;

pub use commands::registry::{
    CommandCategory, CommandContext, CommandRegistry, CommandResult, SlashCommand,
    StaticSlashCommand,
};

pub mod apply_model;
pub mod code_intel;
#[cfg(feature = "computer-use")]
pub mod computer;
pub mod config;

pub mod context_resolver;
pub mod cost;
pub mod cron;
pub mod daemon;

pub mod diff_session;
pub mod doctor;
pub mod editor_core;
pub mod error;

pub mod evals;

pub mod evolution;

pub mod gateway;
pub mod guardrails;
pub mod hands;
pub mod hardware;
pub mod health;
pub mod heartbeat;
pub mod hooks;
pub mod i18n;
pub mod identity;

pub mod inline_completion;
#[cfg(feature = "lan-comms")]
pub mod lan;
pub mod lsp;

pub mod inline_edit;
pub mod integrations;
pub mod memdir;
pub mod memory;
pub mod migration;
pub mod multimodal;
pub mod observability;
pub mod onboard;
pub mod peripherals;
pub mod providers;
pub mod python_env;
pub mod rag;
pub mod routines;
pub mod rpc;
pub mod runtime;
pub mod security;
pub mod services;
pub mod skillforge;
pub mod skills;
pub mod sop;
pub mod user_rules;
pub mod token_saver;
pub mod tools;
pub mod trust;
pub mod tunnel;
pub mod util;
pub mod verifiable_intent;

pub mod write_mode;

pub mod event_bus;
pub mod workflows;

pub mod bootstrap;
pub mod buddy;
pub mod constants;
pub mod context;
pub mod coordinator;

#[cfg(feature = "crdt-coordination")]
pub mod crdt;
pub mod entrypoints;

pub mod keybindings;
pub mod remote;
pub mod schemas;
pub mod workers;

pub mod vim_mode;

#[cfg(feature = "tui")]
pub mod tui;

#[cfg(feature = "plugins-wasm")]
pub mod plugins;

pub use config::Config;

pub mod embed;

pub use agent::coordination::{Coordinator, CoordinatorHandle};
pub use agent::registry::{AgentRegistry, AgentRegistryHandle};
pub use agent::supervisor::{Supervisor, SupervisorHandle};
pub use agent::task_orchestrator::queue::{TaskQueue, TaskQueueHandle};
pub use entrypoints::{
    HookEvent, PermissionMode, SdkConfig, SdkEntrypoint, SdkHookCallback, SdkMcpServer, SdkMessage,
    SdkModelUsage, SdkSession, SdkStatus, SdkToolCall, SdkToolCallBuilder, SdkTurnEvent,
    SdkTurnResult,
};
pub use error::{
    BlackboardError, CoordinatorError, EventBusError, RegistryError, SchedulerError, SenError,
    SupervisorError, TaskQueueError,
};
pub use memory::blackboard::{Blackboard, BlackboardHandle};

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GatewayCommands {

    #[command(long_about = "\
Start the gateway server (webhooks, websockets).

Runs the HTTP/WebSocket gateway that accepts incoming webhook events \
and WebSocket connections. Bind address defaults to the values in \
your config file (gateway.host / gateway.port).

Examples:
  sen gateway start              # use config defaults
  sen gateway start -p 8080      # listen on port 8080
  sen gateway start --host 0.0.0.0   # requires [gateway].allow_public_bind=true or a tunnel
  sen gateway start -p 0         # random available port")]
    Start {

        #[arg(short, long)]
        port: Option<u16>,

        #[arg(long)]
        host: Option<String>,
    },

    #[command(long_about = "\
Restart the gateway server.

Stops the running gateway if present, then starts a new instance \
with the current configuration.

Examples:
  sen gateway restart            # restart with config defaults
  sen gateway restart -p 8080    # restart on port 8080")]
    Restart {

        #[arg(short, long)]
        port: Option<u16>,

        #[arg(long)]
        host: Option<String>,
    },

    #[command(long_about = "\
Show or generate the gateway pairing code.

Displays the pairing code for connecting new clients without \
restarting the gateway. Requires the gateway to be running.

With --new, generates a fresh pairing code even if the gateway \
was previously paired (useful for adding additional clients).

Examples:
  sen gateway get-paircode       # show current pairing code
  sen gateway get-paircode --new # generate a new pairing code")]
    GetPaircode {

        #[arg(long)]
        new: bool,
    },
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServiceCommands {

    Install,

    Start,

    Stop,

    Restart,

    Status,

    Uninstall,

    Logs {

        #[arg(short = 'n', long, default_value = "50")]
        lines: usize,

        #[arg(short, long)]
        follow: bool,
    },
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChannelCommands {

    List,

    Start,

    Doctor,

    #[command(long_about = "\
Add a new channel configuration.

Provide the channel type and a JSON object with the required \
configuration keys for that channel type.

Supported types: telegram, discord, slack, whatsapp, matrix, imessage, email.

Examples:
  sen channel add telegram '{\"bot_token\":\"...\",\"name\":\"my-bot\"}'
  sen channel add discord '{\"bot_token\":\"...\",\"name\":\"my-discord\"}'")]
    Add {

        channel_type: String,

        config: String,
    },

    Remove {

        name: String,
    },

    #[command(long_about = "\
Bind a Telegram identity into the allowlist.

Adds a Telegram username (without the '@' prefix) or numeric user \
ID to the channel allowlist so the agent will respond to messages \
from that identity.

Examples:
  sen channel bind-telegram sen_user
  sen channel bind-telegram 123456789")]
    BindTelegram {

        identity: String,
    },

    #[command(long_about = "\
Send a one-off message to a configured channel.

Sends a text message through the specified channel without starting \
the full agent loop. Useful for scripted notifications, hardware \
sensor alerts, and automation pipelines.

The --channel-id selects the channel by its config section name \
(e.g. 'telegram', 'discord', 'slack'). The --recipient is the \
platform-specific destination (e.g. a Telegram chat ID).

Examples:
  sen channel send 'Someone is near your device.' --channel-id telegram --recipient 123456789
  sen channel send 'Build succeeded!' --channel-id discord --recipient 987654321")]
    Send {

        message: String,

        #[arg(long)]
        channel_id: String,

        #[arg(long)]
        recipient: String,
    },
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillCommands {

    List,

    Audit {

        source: String,
    },

    Install {

        source: String,
    },

    Remove {

        name: String,
    },

    Test {

        name: Option<String>,

        #[arg(long)]
        verbose: bool,
    },

    Search {

        query: String,
    },
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MigrateCommands {

    #[command(name = "legacy-memory")]
    LegacyMemory {

        #[arg(long)]
        source: Option<std::path::PathBuf>,

        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CronCommands {

    List,

    #[command(long_about = "\
Add a new recurring scheduled task.

Uses standard 5-field cron syntax: 'min hour day month weekday'. \
Times are evaluated in UTC by default; use --tz with an IANA \
timezone name to override.

Examples:
  sen cron add '0 9 * * 1-5' 'Good morning' --tz America/New_York --agent
  sen cron add '*/30 * * * *' 'Check system health' --agent
  sen cron add '*/5 * * * *' 'echo ok'")]
    Add {

        expression: String,

        #[arg(long)]
        tz: Option<String>,

        #[arg(long)]
        agent: bool,

        #[arg(long = "allowed-tool")]
        allowed_tools: Vec<String>,

        command: String,
    },

    #[command(long_about = "\
Add a one-shot task that fires at a specific UTC timestamp.

The timestamp must be in RFC 3339 format (e.g. 2025-01-15T14:00:00Z).

Examples:
  sen cron add-at 2025-01-15T14:00:00Z 'Send reminder'
  sen cron add-at 2025-12-31T23:59:00Z 'Happy New Year!'")]
    AddAt {

        at: String,

        #[arg(long)]
        agent: bool,

        #[arg(long = "allowed-tool")]
        allowed_tools: Vec<String>,

        command: String,
    },

    #[command(long_about = "\
Add a task that repeats at a fixed interval.

Interval is specified in milliseconds. For example, 60000 = 1 minute.

Examples:
  sen cron add-every 60000 'Ping heartbeat'     # every minute
  sen cron add-every 3600000 'Hourly report'    # every hour")]
    AddEvery {

        every_ms: u64,

        #[arg(long)]
        agent: bool,

        #[arg(long = "allowed-tool")]
        allowed_tools: Vec<String>,

        command: String,
    },

    #[command(long_about = "\
Add a one-shot task that fires after a delay from now.

Accepts human-readable durations: s (seconds), m (minutes), \
h (hours), d (days).

Examples:
  sen cron once 30m 'Run backup in 30 minutes'
  sen cron once 2h 'Follow up on deployment'
  sen cron once 1d 'Daily check'")]
    Once {

        delay: String,

        #[arg(long)]
        agent: bool,

        #[arg(long = "allowed-tool")]
        allowed_tools: Vec<String>,

        command: String,
    },

    Remove {

        id: String,
    },

    #[command(long_about = "\
Update one or more fields of an existing scheduled task.

Only the fields you specify are changed; others remain unchanged.

Examples:
  sen cron update <task-id> --expression '0 8 * * *'
  sen cron update <task-id> --tz Europe/London --name 'Morning check'
  sen cron update <task-id> --command 'Updated message'")]
    Update {

        id: String,

        #[arg(long)]
        expression: Option<String>,

        #[arg(long)]
        tz: Option<String>,

        #[arg(long)]
        command: Option<String>,

        #[arg(long)]
        name: Option<String>,

        #[arg(long = "allowed-tool")]
        allowed_tools: Vec<String>,
    },

    Pause {

        id: String,
    },

    Resume {

        id: String,
    },
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryCommands {

    List {

        #[arg(long)]
        category: Option<String>,

        #[arg(long)]
        session: Option<String>,

        #[arg(long, default_value = "50")]
        limit: usize,

        #[arg(long, default_value = "0")]
        offset: usize,
    },

    Get {

        key: String,
    },

    Stats,

    Clear {

        #[arg(long)]
        key: Option<String>,

        #[arg(long)]
        category: Option<String>,

        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum IntegrationCommands {

    Info {

        name: String,
    },
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HardwareCommands {

    #[command(long_about = "\
Enumerate USB devices and show known boards.

Scans connected USB devices by VID/PID and matches them against \
known development boards (STM32 Nucleo, Arduino, ESP32).

Examples:
  sen hardware discover")]
    Discover,

    #[command(long_about = "\
Introspect a device by its serial or device path.

Opens the specified device path and queries for board information, \
firmware version, and supported capabilities.

Examples:
  sen hardware introspect /dev/ttyACM0
  sen hardware introspect COM3")]
    Introspect {

        path: String,
    },

    #[command(long_about = "\
Get chip info via USB using probe-rs over ST-Link.

Queries the target MCU directly through the debug probe without \
requiring any firmware on the target board.

Examples:
  sen hardware info
  sen hardware info --chip STM32F401RETx")]
    Info {

        #[arg(long, default_value = "STM32F401RETx")]
        chip: String,
    },
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PeripheralCommands {

    List,

    #[command(long_about = "\
Add a peripheral by board type and transport path.

Registers a hardware board so the agent can use its tools (GPIO, \
sensors, actuators). Use 'native' as path for local GPIO on \
single-board computers like Raspberry Pi.

Supported boards: nucleo-f401re, rpi-gpio, esp32, arduino-uno.

Examples:
  sen peripheral add nucleo-f401re /dev/ttyACM0
  sen peripheral add rpi-gpio native
  sen peripheral add esp32 /dev/ttyUSB0")]
    Add {

        board: String,

        path: String,
    },

    #[command(long_about = "\
Flash SenWeaverCoding firmware to an Arduino board.

Generates the .ino sketch, installs arduino-cli if it is not \
already available, compiles, and uploads the firmware.

Examples:
  sen peripheral flash
  sen peripheral flash --port /dev/cu.usbmodem12345
  sen peripheral flash -p COM3")]
    Flash {

        #[arg(short, long)]
        port: Option<String>,
    },

    SetupUnoQ {

        #[arg(long)]
        host: Option<String>,
    },

    FlashNucleo,
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SopCommands {

    List,

    Validate {

        name: Option<String>,
    },

    Show {

        name: String,
    },
}
