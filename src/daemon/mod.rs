// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::config::Config;
use anyhow::Result;
use chrono::Utc;
use std::future::Future;
use std::path::PathBuf;
use tokio::task::JoinHandle;
use tokio::time::Duration;

const STATUS_FLUSH_SECONDS: u64 = 5;

const SHUTDOWN_GRACE_SECS: u64 = 10;

async fn wait_for_shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sighup = signal(SignalKind::hangup())?;

        loop {
            tokio::select! {
                _ = sigint.recv() => {
                    tracing::info!("Received SIGINT, shutting down...");
                    break;
                }
                _ = sigterm.recv() => {
                    tracing::info!("Received SIGTERM, shutting down...");
                    break;
                }
                _ = sighup.recv() => {
                    tracing::info!("Received SIGHUP, ignoring (daemon stays running)");
                }
            }
        }
    }

    #[cfg(windows)]
    {
        use tokio::signal::windows;

        let mut ctrl_c = windows::ctrl_c()?;
        let mut ctrl_break = windows::ctrl_break()?;
        let mut ctrl_close = windows::ctrl_close()?;
        let mut ctrl_shutdown = windows::ctrl_shutdown()?;

        tokio::select! {
            _ = ctrl_c.recv() => {
                tracing::info!("Received Ctrl+C, shutting down...");
            }
            _ = ctrl_break.recv() => {
                tracing::info!("Received Ctrl+Break, shutting down...");
            }
            _ = ctrl_close.recv() => {
                tracing::info!("Received console close, shutting down...");
            }
            _ = ctrl_shutdown.recv() => {
                tracing::info!("Received system shutdown, shutting down...");
            }
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        tokio::signal::ctrl_c().await?;
        tracing::info!("Received Ctrl+C, shutting down...");
    }

    Ok(())
}

pub async fn run(config: Config, host: String, port: u16) -> Result<()> {
    let initial_backoff = config.reliability.channel_initial_backoff_secs.max(1);
    let max_backoff = config
        .reliability
        .channel_max_backoff_secs
        .max(initial_backoff);

    let _event_bus = crate::event_bus::integration::init_global_bus(
        config
            .config_path
            .parent()
            .map(|p| p.join("event_audit.jsonl")),
    );
    let _multi_agent_rt = crate::agent::multi_agent_runtime::init_global_runtime();
    crate::agent::multi_agent_runtime::register_configured_agents(&_multi_agent_rt, &config);

    {
        let svc_data_dir = config
            .config_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| config.workspace_dir.join(".senweavercoding"));
        let _ = crate::services::init_services(crate::services::ServiceContainerConfig {
            data_dir: svc_data_dir,
            shared_config: None,
            team_sync_enabled: config.teams.sync_enabled,
            ..Default::default()
        });
    }

    {
        let workspace_root = if config.workspace_dir.as_os_str().is_empty() {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        } else {
            config.workspace_dir.clone()
        };
        crate::workers::init_global_supervisor(workspace_root.clone());
        crate::workers::scan_and_recover_at(&workspace_root);
    }
    crate::event_bus::integration::publish_system(
        "daemon",
        crate::event_bus::types::SystemCategory::Startup,
        "Daemon starting",
    )
    .await;

    crate::health::mark_component_ok("daemon");

    if config.heartbeat.enabled {
        let _ =
            crate::heartbeat::engine::HeartbeatEngine::ensure_heartbeat_file(&config.workspace_dir)
                .await;
    }

    let mut handles: Vec<JoinHandle<()>> = vec![spawn_state_writer(config.clone())];

    {
        let gateway_cfg = config.clone();
        let gateway_host = host.clone();
        handles.push(spawn_component_supervisor(
            "gateway",
            initial_backoff,
            max_backoff,
            move || {
                let cfg = gateway_cfg.clone();
                let host = gateway_host.clone();
                async move { Box::pin(crate::gateway::run_gateway(&host, port, cfg, None)).await }
            },
        ));
    }

    if config.rpc.enabled {
        let stdio_transport = matches!(
            crate::rpc::server::build_transport(&config.rpc),
            Ok(crate::rpc::RpcTransport::Stdio)
        );
        if stdio_transport {
            crate::health::mark_component_disabled("rpc");
            tracing::info!(
                "RPC stdio transport is not usable under the daemon (stdin is detached); \
                 configure rpc.unix_socket or rpc.http to serve RPC from the daemon"
            );
        } else {
            let rpc_config = config.clone();
            handles.push(spawn_component_supervisor(
                "rpc",
                initial_backoff,
                max_backoff,
                move || {
                    let cfg = rpc_config.clone();
                    async move {
                        let server: crate::rpc::RpcServer =
                            crate::rpc::RpcServer::new(&cfg).await?;
                        server.run().await
                    }
                },
            ));
        }
    } else {
        crate::health::mark_component_disabled("rpc");
    }

    {
        if has_supervised_channels(&config) {
            let channels_cfg = config.clone();
            handles.push(spawn_component_supervisor(
                "channels",
                initial_backoff,
                max_backoff,
                move || {
                    let cfg = channels_cfg.clone();
                    async move { Box::pin(crate::channels::start_channels(cfg)).await }
                },
            ));
        } else {
            crate::health::mark_component_disabled("channels");
            tracing::info!("No real-time channels configured; channel supervisor disabled");
        }
    }

    if config.heartbeat.enabled {
        let heartbeat_cfg = config.clone();
        handles.push(spawn_component_supervisor(
            "heartbeat",
            initial_backoff,
            max_backoff,
            move || {
                let cfg = heartbeat_cfg.clone();
                async move { Box::pin(run_heartbeat_worker(cfg)).await }
            },
        ));
    }

    if config.hands.enabled {
        let hands_cfg = config.clone();
        handles.push(spawn_component_supervisor(
            "hands",
            initial_backoff,
            max_backoff,
            move || {
                let cfg = hands_cfg.clone();
                async move { Box::pin(crate::hands::runner::run(cfg)).await }
            },
        ));
    } else {
        crate::health::mark_component_disabled("hands");
    }

    if config.cron.enabled {
        let scheduler_cfg = config.clone();
        handles.push(spawn_component_supervisor(
            "scheduler",
            initial_backoff,
            max_backoff,
            move || {
                let cfg = scheduler_cfg.clone();
                async move { Box::pin(crate::cron::scheduler::run(cfg)).await }
            },
        ));
    } else {
        crate::health::mark_component_disabled("scheduler");
        tracing::info!("Cron disabled; scheduler supervisor not started");
    }

    println!("🧠 SenWeaverCoding daemon started");
    println!("   Gateway:  http://{host}:{port}");
    println!("   Components: gateway, channels, heartbeat, scheduler");
    if config.gateway.require_pairing {
        println!("   Pairing:    enabled (code appears in gateway output above)");
    }
    println!("   Ctrl+C or SIGTERM to stop");

    wait_for_shutdown_signal().await?;

    crate::event_bus::integration::publish_system(
        "daemon",
        crate::event_bus::types::SystemCategory::Shutdown,
        "Daemon shutting down",
    )
    .await;

    crate::health::mark_component_error("daemon", "shutdown requested");

    if crate::gateway::request_shutdown() {
        let grace = Duration::from_secs(SHUTDOWN_GRACE_SECS);
        if crate::gateway::wait_embedded_stopped(grace).await {
            tracing::info!("Gateway stopped gracefully; aborting remaining components");
        } else {
            tracing::warn!(
                grace_secs = SHUTDOWN_GRACE_SECS,
                "Gateway did not stop within grace period; forcing component shutdown"
            );
        }
    }

    for handle in &handles {
        handle.abort();
    }
    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}

pub fn state_file_path(config: &Config) -> PathBuf {
    config
        .config_path
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("daemon_state.json")
}

fn spawn_state_writer(config: Config) -> JoinHandle<()> {
    crate::runtime::spawn_supervised("daemon.state_writer", async move {
        let path = state_file_path(&config);
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let mut interval = tokio::time::interval(Duration::from_secs(STATUS_FLUSH_SECONDS));
        loop {
            interval.tick().await;
            let mut json = crate::health::snapshot_json();
            if let Some(obj) = json.as_object_mut() {
                obj.insert(
                    "written_at".into(),
                    serde_json::json!(Utc::now().to_rfc3339()),
                );
            }
            let data = serde_json::to_vec_pretty(&json).unwrap_or_else(|_| b"{}".to_vec());
            let _ = tokio::fs::write(&path, data).await;
        }
    })
    .into_inner()
}

fn spawn_component_supervisor<F, Fut>(
    name: &'static str,
    initial_backoff_secs: u64,
    max_backoff_secs: u64,
    mut run_component: F,
) -> JoinHandle<()>
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    crate::runtime::spawn_supervised(format!("daemon.supervisor.{name}"), async move {
        let mut backoff = initial_backoff_secs.max(1);
        let max_backoff = max_backoff_secs.max(backoff);

        loop {
            crate::health::mark_component_starting(name);
            let started = std::time::Instant::now();
            use futures_util::FutureExt as _;
            let mut run_fut = std::pin::pin!(std::panic::AssertUnwindSafe(run_component()).catch_unwind());
            let outcome = tokio::select! {
                biased;
                outcome = &mut run_fut => outcome,
                () = tokio::time::sleep(Duration::from_secs(60)) => {
                    crate::health::mark_component_ok(name);
                    run_fut.await
                }
            };
            match outcome {
                Ok(Ok(())) => {
                    crate::health::mark_component_error(name, "component exited unexpectedly");
                    tracing::warn!("Daemon component '{name}' exited unexpectedly");
                }
                Ok(Err(e)) => {
                    crate::health::mark_component_error(name, e.to_string());
                    tracing::error!("Daemon component '{name}' failed: {e}");
                }
                Err(panic) => {
                    let msg = crate::util::describe_panic(&*panic);
                    crate::health::mark_component_error(
                        name,
                        format!("component panicked: {msg}"),
                    );
                    tracing::error!("Daemon component '{name}' panicked: {msg}");
                }
            }

            let stable_run = started.elapsed() >= Duration::from_secs(60);
            crate::health::bump_component_restart(name);
            if stable_run {
                backoff = initial_backoff_secs.max(1);
            }
            tokio::time::sleep(Duration::from_secs(backoff)).await;
            backoff = backoff.saturating_mul(2).min(max_backoff);
        }
    })
    .into_inner()
}

async fn run_heartbeat_worker(config: Config) -> Result<()> {
    use crate::heartbeat::engine::{
        HeartbeatEngine, HeartbeatTask, TaskPriority, TaskStatus, compute_adaptive_interval,
    };
    use std::sync::Arc;

    let observer: std::sync::Arc<dyn crate::observability::Observer> =
        std::sync::Arc::from(crate::observability::create_observer(&config.observability));
    let engine = HeartbeatEngine::new(config.workspace_dir.clone(), observer);
    let metrics = engine.metrics();
    let delivery = resolve_heartbeat_delivery(&config)?;
    let two_phase = config.heartbeat.decision_before_execute;
    let adaptive = config.heartbeat.adaptive;
    let start_time = std::time::Instant::now();

    let deadman_timeout = config.heartbeat.deadman_timeout_minutes;
    if deadman_timeout > 0 {
        let dm_metrics = Arc::downgrade(&metrics);
        let dm_config = config.clone();
        let dm_delivery = delivery.clone();
        crate::runtime::spawn_supervised("daemon.deadman_watcher", async move {
            let check_interval = Duration::from_secs(60);
            let timeout = chrono::Duration::minutes(i64::from(deadman_timeout));
            loop {
                tokio::time::sleep(check_interval).await;
                let Some(dm_metrics) = dm_metrics.upgrade() else {
                    tracing::debug!(
                        "heartbeat metrics dropped; deadman watcher exiting (a fresh watcher \
                         starts with the next heartbeat worker)"
                    );
                    break;
                };
                let last_tick = dm_metrics.lock().last_tick_at;
                if let Some(last) = last_tick {
                    if chrono::Utc::now() - last > timeout {
                        let alert = format!(
                            "⚠️ Heartbeat dead-man's switch: no tick in {deadman_timeout} minutes"
                        );
                        let (channel, target) =
                            if let Some(ch) = &dm_config.heartbeat.deadman_channel {
                                let to = dm_config
                                    .heartbeat
                                    .deadman_to
                                    .as_deref()
                                    .or(dm_config.heartbeat.to.as_deref())
                                    .unwrap_or_default();
                                (ch.clone(), to.to_string())
                            } else if let Some((ch, to)) = &dm_delivery {
                                (ch.clone(), to.clone())
                            } else {
                                continue;
                            };
                        let _ = crate::cron::scheduler::deliver_announcement(
                            &dm_config, &channel, &target, &alert,
                        )
                        .await;
                    }
                }
            }
        });
    }

    let base_interval = config.heartbeat.interval_minutes.max(5);
    let mut sleep_mins = base_interval;

    loop {
        tokio::time::sleep(Duration::from_secs(u64::from(sleep_mins) * 60)).await;

        {
            let mut m = metrics.lock();
            m.uptime_secs = start_time.elapsed().as_secs();
            m.last_tick_at = Some(chrono::Utc::now());
        }
        engine.record_tick_event();

        let tick_start = std::time::Instant::now();

        let mut tasks = engine.collect_runnable_tasks().await?;
        let has_high_priority = tasks.iter().any(|t| t.priority == TaskPriority::High);

        if tasks.is_empty() {
            if let Some(fallback) = config
                .heartbeat
                .message
                .as_deref()
                .map(str::trim)
                .filter(|m| !m.is_empty())
            {
                tasks.push(HeartbeatTask {
                    text: fallback.to_string(),
                    priority: TaskPriority::Medium,
                    status: TaskStatus::Active,
                });
            } else {
                #[allow(clippy::cast_precision_loss)]
                let elapsed = tick_start.elapsed().as_millis() as f64;
                metrics.lock().record_success(elapsed);
                continue;
            }
        }

        let tasks_to_run = if two_phase {
            let decision_prompt = format!(
                "[Heartbeat Task | decision] {}",
                HeartbeatEngine::build_decision_prompt(&tasks),
            );
            match Box::pin(crate::agent::run(
                config.clone(),
                Some(decision_prompt),
                None,
                None,
                0.0,
                vec![],
                false,
                None,
                None,
                None,
            ))
            .await
            {
                Ok(response) => {
                    let indices = HeartbeatEngine::parse_decision_response(&response, tasks.len());
                    if indices.is_empty() {
                        tracing::info!("💓 Heartbeat: skip (nothing to do)");
                        crate::health::mark_component_ok("heartbeat");
                        #[allow(clippy::cast_precision_loss)]
                        let elapsed = tick_start.elapsed().as_millis() as f64;
                        metrics.lock().record_success(elapsed);
                        continue;
                    }
                    tracing::info!(
                        "💓 Heartbeat: run {} of {} tasks",
                        indices.len(),
                        tasks.len()
                    );
                    indices
                        .into_iter()
                        .filter_map(|i| tasks.get(i).cloned())
                        .collect()
                }
                Err(e) => {
                    tracing::warn!("💓 Heartbeat failed, running all tasks: {e}");
                    tasks
                }
            }
        } else {
            tasks
        };

        let session_context = if config.heartbeat.load_session_context {
            load_heartbeat_session_context(&config)
        } else {
            None
        };

        let mut tick_had_error = false;
        for task in &tasks_to_run {
            metrics.lock().last_tick_at = Some(chrono::Utc::now());
            let task_start = std::time::Instant::now();
            let task_prompt = format!("[Heartbeat Task | {}] {}", task.priority, task.text);
            let prompt = match &session_context {
                Some(ctx) => format!("{ctx}\n\n{task_prompt}"),
                None => task_prompt,
            };
            let temp = config.default_temperature;
            // Bound each heartbeat task so a single hung provider call cannot
            // freeze the whole periodic loop indefinitely (deadman only alerts, it
            // does not unblock a stuck await).
            let heartbeat_task_timeout = std::time::Duration::from_secs(
                u64::from(config.heartbeat.deadman_timeout_minutes)
                    .saturating_mul(60)
                    .max(1800),
            );
            let run_fut = Box::pin(crate::agent::run(
                config.clone(),
                Some(prompt),
                None,
                None,
                temp,
                vec![],
                false,
                None,
                None,
                None,
            ));
            let run_result = match tokio::time::timeout(heartbeat_task_timeout, run_fut).await {
                Ok(r) => r,
                Err(_) => Err(anyhow::anyhow!(
                    "heartbeat task exceeded {}s wall-clock budget and was abandoned",
                    heartbeat_task_timeout.as_secs()
                )),
            };
            match run_result {
                Ok(output) => {
                    crate::health::mark_component_ok("heartbeat");
                    #[allow(clippy::cast_possible_truncation)]
                    let duration_ms = task_start.elapsed().as_millis() as i64;
                    let now = chrono::Utc::now();
                    let _ = crate::heartbeat::store::record_run(
                        &config.workspace_dir,
                        &task.text,
                        &task.priority.to_string(),
                        now - chrono::Duration::milliseconds(duration_ms),
                        now,
                        "ok",
                        Some(output.as_str()),
                        duration_ms,
                        config.heartbeat.max_run_history,
                    );
                    let announcement = if output.trim().is_empty() {
                        format!("💓 heartbeat task completed: {}", task.text)
                    } else {
                        output
                    };
                    if let Some((channel, target)) = &delivery {
                        if let Err(e) = crate::cron::scheduler::deliver_announcement(
                            &config,
                            channel,
                            target,
                            &announcement,
                        )
                        .await
                        {
                            crate::health::mark_component_error(
                                "heartbeat",
                                format!("delivery failed: {e}"),
                            );
                            tracing::warn!("Heartbeat delivery failed: {e}");
                        }
                    }
                }
                Err(e) => {
                    tick_had_error = true;
                    #[allow(clippy::cast_possible_truncation)]
                    let duration_ms = task_start.elapsed().as_millis() as i64;
                    let now = chrono::Utc::now();
                    let _ = crate::heartbeat::store::record_run(
                        &config.workspace_dir,
                        &task.text,
                        &task.priority.to_string(),
                        now - chrono::Duration::milliseconds(duration_ms),
                        now,
                        "error",
                        Some(&e.to_string()),
                        duration_ms,
                        config.heartbeat.max_run_history,
                    );
                    crate::health::mark_component_error("heartbeat", e.to_string());
                    engine.record_error_event(&e.to_string());
                    tracing::warn!("Heartbeat task failed: {e}");
                }
            }
        }

        #[allow(clippy::cast_precision_loss)]
        let tick_elapsed = tick_start.elapsed().as_millis() as f64;
        {
            let mut m = metrics.lock();
            if tick_had_error {
                m.record_failure(tick_elapsed);
            } else {
                m.record_success(tick_elapsed);
            }
        }

        if adaptive {
            let failures = metrics.lock().consecutive_failures;
            sleep_mins = compute_adaptive_interval(
                base_interval,
                config.heartbeat.min_interval_minutes,
                config.heartbeat.max_interval_minutes,
                failures,
                has_high_priority,
            );
        } else {
            sleep_mins = base_interval;
        }
    }
}

fn resolve_heartbeat_delivery(config: &Config) -> Result<Option<(String, String)>> {
    let channel = config
        .heartbeat
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let target = config
        .heartbeat
        .to
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match (channel, target) {

        (Some(channel), Some(target)) => {
            validate_heartbeat_channel_config(config, channel)?;
            Ok(Some((channel.to_string(), target.to_string())))
        }

        (Some(_), None) => anyhow::bail!("heartbeat.to is required when heartbeat.target is set"),
        (None, Some(_)) => anyhow::bail!("heartbeat.target is required when heartbeat.to is set"),

        (None, None) => Ok(auto_detect_heartbeat_channel(config)),
    }
}

const HEARTBEAT_SESSION_CONTEXT_MESSAGES: usize = 20;

fn load_heartbeat_session_context(config: &Config) -> Option<String> {
    use crate::providers::traits::ChatMessage;

    let channel = config
        .heartbeat
        .target
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    let to = config
        .heartbeat
        .to
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())?;

    if channel.contains('/') || channel.contains('\\') || to.contains('/') || to.contains('\\') {
        tracing::warn!("heartbeat session context: channel/to contains path separators, skipping");
        return None;
    }

    let sessions_dir = config.workspace_dir.join("sessions");

    let prefix = format!("{channel}_");
    let suffix = format!("_{to}.jsonl");
    let exact = format!("{channel}_{to}.jsonl");
    let mid_prefix = format!("{channel}_{to}_");

    let path = std::fs::read_dir(&sessions_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.ends_with(".jsonl")
                && (name == exact
                    || (name.starts_with(&prefix) && name.ends_with(&suffix))
                    || name.starts_with(&mid_prefix))
        })
        .max_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        })
        .map(|e| e.path())?;

    if !path.exists() {
        tracing::debug!("💓 Heartbeat session context: no session file found for {channel}/{to}");
        return None;
    }

    let messages = load_jsonl_messages(&path);
    if messages.is_empty() {
        return None;
    }

    let recent: Vec<&ChatMessage> = messages
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .rev()
        .take(HEARTBEAT_SESSION_CONTEXT_MESSAGES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    let has_user_message = recent.iter().any(|m| m.role == "user");
    if !has_user_message {
        tracing::debug!(
            "💓 Heartbeat session context: no user messages in recent history  -  skipping"
        );
        return None;
    }

    let last_message_age = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|mtime| mtime.elapsed().ok());

    let silence_note = match last_message_age {
        Some(age) => {
            let mins = age.as_secs() / 60;
            if mins < 60 {
                format!("(last message ~{mins} minutes ago)\n")
            } else {
                let hours = mins / 60;
                let rem = mins % 60;
                if rem == 0 {
                    format!("(last message ~{hours}h ago)\n")
                } else {
                    format!("(last message ~{hours}h {rem}m ago)\n")
                }
            }
        }
        None => String::new(),
    };

    tracing::debug!(
        "💓 Heartbeat session context: {} messages from {}, silence: {}",
        recent.len(),
        path.display(),
        silence_note.trim(),
    );

    let mut ctx = format!(
        "[Recent conversation history  -  use this for context when composing your message] {silence_note}",
    );
    for msg in &recent {
        let label = if msg.role == "user" { "User" } else { "You" };

        let content = if msg.content.len() > 500 {
            let truncate_at = msg
                .content
                .char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i <= 500)
                .last()
                .unwrap_or(0);
            format!("{}…", &msg.content[..truncate_at])
        } else {
            msg.content.clone()
        };
        ctx.push_str(label);
        ctx.push_str(": ");
        ctx.push_str(&content);
        ctx.push('\n');
    }

    Some(ctx)
}

fn load_jsonl_messages(path: &std::path::Path) -> Vec<crate::providers::traits::ChatMessage> {
    use std::collections::VecDeque;
    use std::io::BufRead;

    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = std::io::BufReader::new(file);
    let mut window: VecDeque<crate::providers::traits::ChatMessage> =
        VecDeque::with_capacity(HEARTBEAT_SESSION_CONTEXT_MESSAGES + 1);
    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(msg) = serde_json::from_str::<crate::providers::traits::ChatMessage>(trimmed) {
            window.push_back(msg);
            if window.len() > HEARTBEAT_SESSION_CONTEXT_MESSAGES {
                window.pop_front();
            }
        }
    }
    window.into_iter().collect()
}

fn auto_detect_heartbeat_channel(config: &Config) -> Option<(String, String)> {

    if let Some(tg) = &config.channels_config.telegram {

        let target = tg.allowed_users.first().cloned().unwrap_or_default();
        if !target.is_empty() {
            return Some(("telegram".to_string(), target));
        }
    }
    if config.channels_config.discord.is_some() {

        return None;
    }
    if config.channels_config.slack.is_some() {

        return None;
    }
    if config.channels_config.mattermost.is_some() {

        return None;
    }
    None
}

fn validate_heartbeat_channel_config(config: &Config, channel: &str) -> Result<()> {
    match channel.to_ascii_lowercase().as_str() {
        "telegram" => {
            if config.channels_config.telegram.is_none() {
                anyhow::bail!(
                    "heartbeat.target is set to telegram but channels_config.telegram is not configured"
                );
            }
        }
        "discord" => {
            if config.channels_config.discord.is_none() {
                anyhow::bail!(
                    "heartbeat.target is set to discord but channels_config.discord is not configured"
                );
            }
        }
        "slack" => {
            if config.channels_config.slack.is_none() {
                anyhow::bail!(
                    "heartbeat.target is set to slack but channels_config.slack is not configured"
                );
            }
        }
        "mattermost" => {
            if config.channels_config.mattermost.is_none() {
                anyhow::bail!(
                    "heartbeat.target is set to mattermost but channels_config.mattermost is not configured"
                );
            }
        }
        other => anyhow::bail!("unsupported heartbeat.target channel: {other}"),
    }

    Ok(())
}

fn has_supervised_channels(config: &Config) -> bool {
    config
        .channels_config
        .channels_except_webhook()
        .iter()
        .any(|(_, ok)| *ok)
}
