// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, bail, Result};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::hook::{self, HookHandle};
use super::types::{
    MouseButton, RawInputEvent, RecordedStep, RecorderEvent, RecorderStepEvent, RecordingManifest,
    RecordingSummary,
};
use crate::computer::capture::{capture_recorder_frame, RecorderFrame};
use crate::computer::input;
use crate::config::Config;

const DRAG_THRESHOLD_PX: f64 = 10.0;
const QUICK_CLICK_MS: u128 = 250;
const QUICK_CLICK_SLOP_PX: f64 = 14.0;
const DOUBLE_CLICK_DIST: i32 = 8;
const CLICK_FINALIZE_MS: u64 = 260;
const SCROLL_FINALIZE_MS: u64 = 400;
const SCREENSHOT_INTERVAL_MS: u64 = 700;
const FIRST_FRAME_WAIT_MS: u64 = 300;
const MAX_STEPS: usize = 500;
const MAX_DELAY_MS: u64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Recording,
    Stopped,
}

struct RecordingSession {
    generation: u64,
    dir: PathBuf,
    name: String,
    phase: Phase,
    stop_tx: Option<oneshot::Sender<()>>,
    join: Option<JoinHandle<RecordingManifest>>,
    input_lease: Option<crate::computer::input::lock::InputLease>,
    activity: Option<crate::computer::activity::ActivityCapture>,
}

static SESSION: Lazy<Mutex<Option<RecordingSession>>> = Lazy::new(|| Mutex::new(None));
static START_LOCK: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));
static NEXT_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static GENERATING: Lazy<Mutex<std::collections::HashSet<String>>> =
    Lazy::new(|| Mutex::new(std::collections::HashSet::new()));

pub fn is_recording() -> bool {
    SESSION
        .lock()
        .as_ref()
        .is_some_and(|s| matches!(s.phase, Phase::Recording))
}

pub fn last_saved_recording() -> Option<String> {
    SESSION
        .lock()
        .as_ref()
        .filter(|s| matches!(s.phase, Phase::Stopped))
        .map(|s| s.name.clone())
}

fn auto_recording_name(task: &str) -> String {
    let lowered = task.trim().to_lowercase();
    let mut slug = String::new();
    let mut prev_sep = false;
    for c in lowered.chars() {
        if c.is_alphanumeric() {
            slug.push(c);
            prev_sep = false;
        } else if !prev_sep && !slug.is_empty() {
            slug.push('-');
            prev_sep = true;
        }
        if slug.chars().count() >= 40 {
            break;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    if slug.is_empty() {
        format!("recording-{ts}")
    } else {
        format!("{slug}-{ts}")
    }
}

async fn unique_recording_dir(workspace_dir: &Path, base: &str) -> (PathBuf, String) {
    let skills_root = workspace_dir.join("skills");
    let mut name = base.to_string();
    let mut counter = 2u32;
    loop {
        let candidate = skills_root.join(&name);
        if !tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
            return (candidate, name);
        }
        name = format!("{base}-{counter}");
        counter += 1;
    }
}

pub async fn start_recording(
    workspace_dir: PathBuf,
    task: String,
    event_tx: UnboundedSender<RecorderEvent>,
) -> Result<u64> {
    let _start_guard = START_LOCK.lock().await;
    if is_recording() {
        bail!("a recording is already in progress");
    }
    SESSION.lock().take();

    let input_lease =
        crate::computer::input::lock::try_acquire(crate::computer::input::lock::InputActivity::Recording)
            .map_err(|e| anyhow!(e))?;

    let legacy_root = workspace_dir.join(".sen").join("computer_recordings");
    if let Ok(mut entries) = tokio::fs::read_dir(&legacy_root).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let _ = tokio::fs::remove_dir_all(entry.path()).await;
        }
    }

    let base = auto_recording_name(&task);
    let (dir, name) = unique_recording_dir(&workspace_dir, &base).await;
    tokio::fs::create_dir_all(dir.join("shots")).await?;

    let (dw, dh) = input::core::main_display_size().await.unwrap_or((0, 0));
    let monitors = crate::computer::capture::list_monitors().await;
    let (hook_handle, raw_rx) = match hook::start_capture() {
        Ok(pair) => pair,
        Err(e) => {
            let _ = tokio::fs::remove_dir_all(&dir).await;
            return Err(e);
        }
    };
    let (stop_tx, stop_rx) = oneshot::channel();

    let manifest = RecordingManifest {
        rec_id: name.clone(),
        task: task.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        display_w: dw,
        display_h: dh,
        steps: Vec::new(),
        skill_name: None,
        run_config: None,
    };

    let activity =
        crate::computer::activity::ActivityCapture::start(&dir, event_tx.clone(), &task);
    let activity_hub = std::sync::Arc::clone(&activity.hub);

    let consumer = Consumer::new(dir.clone(), event_tx, dw, dh, monitors, manifest, activity_hub);
    let join = tokio::spawn(consumer.run(raw_rx, stop_rx, hook_handle));

    let generation = NEXT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut guard = SESSION.lock();
    *guard = Some(RecordingSession {
        generation,
        dir,
        name,
        phase: Phase::Recording,
        stop_tx: Some(stop_tx),
        join: Some(join),
        input_lease: Some(input_lease),
        activity: Some(activity),
    });
    Ok(generation)
}

pub fn record_marker(note: &str) -> Result<()> {
    let guard = SESSION.lock();
    let session = guard
        .as_ref()
        .filter(|s| matches!(s.phase, Phase::Recording))
        .ok_or_else(|| anyhow!("no active recording"))?;
    let Some(activity) = session.activity.as_ref() else {
        bail!("activity capture unavailable");
    };
    activity.marker(note);
    Ok(())
}

fn clear_session(generation: u64) -> Option<PathBuf> {
    let mut guard = SESSION.lock();
    if guard.as_ref().is_some_and(|s| s.generation == generation) {
        return guard.take().map(|s| s.dir);
    }
    None
}

pub async fn stop_recording(generation: u64) -> Result<RecordingSummary> {
    let (stop_tx, join, dir, name, activity) = {
        let mut guard = SESSION.lock();
        let session = guard
            .as_mut()
            .ok_or_else(|| anyhow!("no active recording"))?;
        if session.generation != generation || !matches!(session.phase, Phase::Recording) {
            bail!("no active recording");
        }
        (
            session.stop_tx.take(),
            session.join.take(),
            session.dir.clone(),
            session.name.clone(),
            session.activity.take(),
        )
    };

    if let Some(tx) = stop_tx {
        let _ = tx.send(());
    }
    let manifest = match join {
        Some(handle) => match handle.await {
            Ok(manifest) => manifest,
            Err(e) => {
                if let Some(activity) = activity {
                    activity.stop().await;
                }
                if let Some(dir) = clear_session(generation) {
                    let _ = tokio::fs::remove_dir_all(&dir).await;
                }
                return Err(anyhow!("recording task failed: {e}"));
            }
        },
        None => {
            if let Some(activity) = activity {
                activity.stop().await;
            }
            clear_session(generation);
            bail!("recording task missing");
        }
    };

    if let Some(activity) = activity {
        activity.stop().await;
    }

    if manifest.steps.is_empty() {
        clear_session(generation);
        let _ = tokio::fs::remove_dir_all(&dir).await;
        return Ok(RecordingSummary {
            name: String::new(),
            task: manifest.task.clone(),
            created_at: manifest.created_at.clone(),
            step_count: 0,
            has_skill: false,
            has_trace: false,
            ..RecordingSummary::default()
        });
    }

    let persisted = async {
        let bytes = serde_json::to_vec_pretty(&manifest)?;
        tokio::fs::write(dir.join("recording.json"), bytes).await?;
        anyhow::Ok(())
    }
    .await;
    if let Err(e) = persisted {
        clear_session(generation);
        let _ = tokio::fs::remove_dir_all(&dir).await;
        return Err(anyhow!("failed to save recording: {e}"));
    }

    let summary = RecordingSummary {
        name: name.clone(),
        task: manifest.task.clone(),
        created_at: manifest.created_at.clone(),
        step_count: manifest.steps.len(),
        has_skill: false,
        has_trace: true,
        ..RecordingSummary::default()
    };

    {
        let mut guard = SESSION.lock();
        if let Some(session) = guard.as_mut().filter(|s| s.generation == generation) {
            session.phase = Phase::Stopped;
            session.input_lease = None;
        }
    }

    let post_dir = dir.clone();
    let post_name = name.clone();
    let post_task = manifest.task.clone();
    tokio::spawn(async move {
        if let Err(e) =
            crate::computer::timeline::process_recording(&post_dir, &post_name, &post_task).await
        {
            tracing::warn!("recording post-processing failed for '{post_name}': {e}");
        }
    });

    Ok(summary)
}

pub async fn generate_skill(
    config: &Config,
    provider: &str,
    model: &str,
    workspace_dir: &Path,
    name: &str,
    event_tx: &UnboundedSender<RecorderEvent>,
) -> Result<String> {
    let safe = sanitize_name(name)?;
    if !GENERATING.lock().insert(safe.clone()) {
        bail!("skill generation for '{safe}' is already running");
    }
    let result =
        super::skillgen::generate(config, provider, model, workspace_dir, &safe, event_tx).await;
    GENERATING.lock().remove(&safe);
    result
}

pub async fn discard_recording(generation: u64) -> Result<()> {
    let (stop_tx, join, dir, phase, activity) = {
        let mut guard = SESSION.lock();
        match guard.as_mut().filter(|s| s.generation == generation) {
            Some(session) => (
                session.stop_tx.take(),
                session.join.take(),
                session.dir.clone(),
                session.phase,
                session.activity.take(),
            ),
            None => return Ok(()),
        }
    };

    if matches!(phase, Phase::Recording) {
        if let Some(tx) = stop_tx {
            let _ = tx.send(());
        }
        if let Some(handle) = join {
            let _ = handle.await;
        }
        if let Some(activity) = activity {
            activity.stop().await;
        }
        let _ = tokio::fs::remove_dir_all(&dir).await;
    } else if let Some(activity) = activity {
        activity.stop().await;
    }
    let mut guard = SESSION.lock();
    if guard.as_ref().is_some_and(|s| s.generation == generation) {
        *guard = None;
    }
    Ok(())
}

pub fn list_recordings(workspace_dir: &Path) -> Vec<RecordingSummary> {
    let skills_dir = workspace_dir.join("skills");
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&skills_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("recording.json");
            if !manifest_path.exists() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let (task, created_at, step_count) = std::fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|s| serde_json::from_str::<RecordingManifest>(&s).ok())
                .map(|m| (m.task, m.created_at, m.steps.len()))
                .unwrap_or_default();
            let has_skill = path.join("SKILL.md").exists() || path.join("SKILL.toml").exists();

            let processed = path.join("bundle.json").exists();
            let has_narration = path.join("narration.json").exists();
            let has_audio = path.join("audio.json").exists();
            let has_automation = path.join("built-automation.json").exists();
            let analysis = read_analysis_summary(&path);
            let (duration_ms, event_count) = read_bundle_stats(&path);
            let size_bytes = directory_size(&path, 3);

            out.push(RecordingSummary {
                name,
                task,
                created_at,
                step_count,
                has_skill,
                has_trace: true,
                processed,
                has_narration,
                has_audio,
                has_analysis: analysis.is_some(),
                has_automation,
                duration_ms,
                event_count,
                size_bytes,
                analysis,
            });
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    out
}

fn read_analysis_summary(dir: &Path) -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(dir.join("analysis.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    Some(serde_json::json!({
        "revision": value.get("revision").cloned().unwrap_or(serde_json::json!(1)),
        "createdAt": value.get("createdAt").cloned().unwrap_or(serde_json::Value::Null),
        "title": value.get("title").cloned().unwrap_or(serde_json::json!("")),
        "intent": value.get("intent").cloned().unwrap_or(serde_json::json!("")),
        "intentConfidence": value
            .get("intentConfidence")
            .cloned()
            .unwrap_or(serde_json::json!("medium")),
        "stepCount": value
            .get("steps")
            .and_then(|s| s.as_array())
            .map(|s| s.len())
            .unwrap_or(0),
        "approved": value.get("approved").cloned().unwrap_or(serde_json::json!(false)),
        "narrationSourceUpdatedAt": value
            .get("narrationSourceUpdatedAt")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    }))
}

fn read_bundle_stats(dir: &Path) -> (Option<i64>, Option<u64>) {
    let Some(content) = std::fs::read_to_string(dir.join("bundle.json")).ok() else {
        return (None, None);
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return (None, None);
    };
    let duration = value
        .get("session")
        .and_then(|s| s.get("durationMs"))
        .and_then(|v| v.as_i64());
    let events = value
        .get("stats")
        .and_then(|s| s.get("meaningfulEventCount"))
        .and_then(|v| v.as_u64());
    (duration, events)
}

fn directory_size(dir: &Path, max_depth: u32) -> Option<u64> {
    fn walk(dir: &Path, depth: u32, total: &mut u64) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_file() {
                *total += meta.len();
            } else if meta.is_dir() && depth > 0 {
                walk(&entry.path(), depth - 1, total);
            }
        }
    }
    let mut total = 0u64;
    walk(dir, max_depth, &mut total);
    Some(total)
}

pub async fn delete_recording(workspace_dir: &Path, name: &str) -> Result<()> {
    let safe = sanitize_name(name)?;
    let dir = workspace_dir.join("skills").join(&safe);
    if dir.exists() {
        tokio::fs::remove_dir_all(&dir).await?;
    }
    Ok(())
}

pub async fn rename_recording(
    workspace_dir: &Path,
    old_name: &str,
    new_name: &str,
) -> Result<String> {
    let old_safe = sanitize_name(old_name)?;
    let new_safe = sanitize_name(new_name)?;
    if new_safe.chars().count() > 64 {
        bail!("new name is too long (max 64 characters)");
    }
    if old_safe == new_safe {
        return Ok(new_safe);
    }

    {
        let guard = SESSION.lock();
        if guard
            .as_ref()
            .is_some_and(|s| s.name == old_safe && matches!(s.phase, Phase::Recording))
        {
            bail!("recording '{old_safe}' is still in progress");
        }
    }
    if GENERATING.lock().contains(&old_safe) {
        bail!("skill generation for '{old_safe}' is running; retry after it finishes");
    }

    let skills_root = workspace_dir.join("skills");
    let old_dir = skills_root.join(&old_safe);
    let new_dir = skills_root.join(&new_safe);
    if !old_dir.join("recording.json").is_file() {
        bail!("recording '{old_safe}' not found");
    }
    if tokio::fs::try_exists(&new_dir).await.unwrap_or(false) {
        bail!("a recording or skill named '{new_safe}' already exists");
    }

    tokio::fs::rename(&old_dir, &new_dir).await?;

    let manifest_path = new_dir.join("recording.json");
    if let Ok(data) = tokio::fs::read_to_string(&manifest_path).await {
        if let Ok(mut manifest) = serde_json::from_str::<RecordingManifest>(&data) {
            manifest.rec_id = new_safe.clone();
            if manifest.skill_name.as_deref() == Some(old_safe.as_str()) {
                manifest.skill_name = Some(new_safe.clone());
            }
            if let Ok(bytes) = serde_json::to_vec_pretty(&manifest) {
                let _ = tokio::fs::write(&manifest_path, bytes).await;
            }
        }
    }

    let skill_md_path = new_dir.join("SKILL.md");
    if let Ok(content) = tokio::fs::read_to_string(&skill_md_path).await {
        let old_line = format!("name: {old_safe}");
        let new_line = format!("name: {new_safe}");
        if content.contains(&old_line) {
            let updated = content.replacen(&old_line, &new_line, 1);
            let _ = tokio::fs::write(&skill_md_path, updated.as_bytes()).await;
        }
    }

    {
        let mut guard = SESSION.lock();
        if let Some(session) = guard.as_mut().filter(|s| s.name == old_safe) {
            session.name = new_safe.clone();
            session.dir = new_dir;
        }
    }

    Ok(new_safe)
}

pub async fn load_recording(workspace_dir: &Path, name: &str) -> Result<RecordingManifest> {
    let safe = sanitize_name(name)?;
    let path = workspace_dir
        .join("skills")
        .join(&safe)
        .join("recording.json");
    let data = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| anyhow!("recording '{name}' not found: {e}"))?;
    serde_json::from_str(&data).map_err(|e| anyhow!("invalid recording '{name}': {e}"))
}

pub async fn save_recording_manifest(
    workspace_dir: &Path,
    name: &str,
    manifest: &RecordingManifest,
) -> Result<()> {
    let safe = sanitize_name(name)?;
    let path = workspace_dir
        .join("skills")
        .join(&safe)
        .join("recording.json");
    if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
        bail!("recording '{name}' not found");
    }
    let bytes = serde_json::to_vec_pretty(manifest)?;
    tokio::fs::write(&path, bytes).await?;
    Ok(())
}

pub fn load_skill_instructions(workspace_dir: &Path, name: &str) -> Option<String> {
    let safe = sanitize_name(name).ok()?;
    let path = workspace_dir.join("skills").join(&safe).join("SKILL.md");
    let content = std::fs::read_to_string(&path).ok()?;
    Some(strip_frontmatter(&content))
}

fn sanitize_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
    {
        bail!("invalid recording name");
    }
    Ok(trimmed.to_string())
}

pub(crate) fn strip_frontmatter(content: &str) -> String {
    let trimmed = content.trim_start();
    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let after = &rest[end + 4..];
            return after.trim_start_matches(['\r', '\n']).to_string();
        }
    }
    content.to_string()
}

struct LeftDown {
    x: i32,
    y: i32,
    own: bool,
    moved: bool,
    down_at: Instant,
}

struct PendingClick {
    x: i32,
    y: i32,
    deadline: Instant,
}

struct PendingScroll {
    dir: String,
    amount: i32,
    x: i32,
    y: i32,
    deadline: Instant,
}

struct Consumer {
    dir: PathBuf,
    event_tx: UnboundedSender<RecorderEvent>,
    display_w: i32,
    display_h: i32,
    monitors: Vec<crate::computer::coordinates::MonitorRect>,
    manifest: RecordingManifest,
    activity: std::sync::Arc<crate::computer::activity::events::ActivityHub>,
    frame_rx: Option<tokio::sync::watch::Receiver<Option<RecorderFrame>>>,
    shot_writes: tokio::task::JoinSet<()>,
    waited_first_frame: bool,
    max_steps_notified: bool,
    last_commit: Instant,
    type_buffer: String,
    type_baseline: Option<Option<String>>,
    left_down: Option<LeftDown>,
    pending_click: Option<PendingClick>,
    pending_scroll: Option<PendingScroll>,
    own_filter_notified: bool,
}

impl Consumer {
    fn new(
        dir: PathBuf,
        event_tx: UnboundedSender<RecorderEvent>,
        display_w: i32,
        display_h: i32,
        monitors: Vec<crate::computer::coordinates::MonitorRect>,
        manifest: RecordingManifest,
        activity: std::sync::Arc<crate::computer::activity::events::ActivityHub>,
    ) -> Self {
        Self {
            dir,
            event_tx,
            display_w,
            display_h,
            monitors,
            manifest,
            activity,
            frame_rx: None,
            shot_writes: tokio::task::JoinSet::new(),
            waited_first_frame: false,
            max_steps_notified: false,
            last_commit: Instant::now(),
            type_buffer: String::new(),
            type_baseline: None,
            left_down: None,
            pending_click: None,
            pending_scroll: None,
            own_filter_notified: false,
        }
    }

    async fn run(
        mut self,
        mut raw_rx: UnboundedReceiver<RawInputEvent>,
        mut stop_rx: oneshot::Receiver<()>,
        hook_handle: HookHandle,
    ) -> RecordingManifest {
        let (frame_tx, frame_rx) =
            tokio::sync::watch::channel::<Option<RecorderFrame>>(None);
        self.frame_rx = Some(frame_rx);
        let capture_cancel = tokio_util::sync::CancellationToken::new();
        let capture_stop = capture_cancel.clone();
        let frame_log_dir = self.dir.clone();
        let started_epoch_ms = self.activity.started_epoch_ms();
        let capture_task = tokio::spawn(async move {
            let _ = tokio::fs::create_dir_all(frame_log_dir.join("frames")).await;
            let mut frame_log =
                crate::computer::frames::FrameLog::new(frame_log_dir, started_epoch_ms);
            let mut interval =
                tokio::time::interval(Duration::from_millis(SCREENSHOT_INTERVAL_MS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = capture_stop.cancelled() => break,
                    _ = interval.tick() => {}
                }
                if frame_tx.is_closed() {
                    break;
                }
                if let Ok(frame) = capture_recorder_frame().await {
                    frame_log.offer(
                        frame.phash,
                        frame.transport_width,
                        frame.transport_height,
                        std::sync::Arc::clone(&frame.shot_jpeg_bytes),
                    );
                    if frame_tx.send(Some(frame)).is_err() {
                        break;
                    }
                }
            }
            frame_log.finish().await;
        });
        self.last_commit = Instant::now();

        loop {
            let deadline = self.next_deadline();
            let timer = async {
                match deadline {
                    Some(d) => tokio::time::sleep_until(d).await,
                    None => std::future::pending::<()>().await,
                }
            };

            tokio::select! {
                _ = &mut stop_rx => break,
                _ = timer => {
                    self.finalize_deadlines().await;
                }
                event = raw_rx.recv() => {
                    match event {
                        Some(event) => self.handle(event).await,
                        None => {
                            let _ = self.event_tx.send(RecorderEvent::error_code(
                                "recorder_capture_stopped",
                                "input capture stopped unexpectedly; press stop to save the steps recorded so far",
                            ));
                            break;
                        }
                    }
                }
            }
        }

        self.flush_type().await;
        self.flush_pending_scroll().await;
        self.flush_pending_click().await;

        while self.shot_writes.join_next().await.is_some() {}
        self.frame_rx = None;
        capture_cancel.cancel();
        let _ = capture_task.await;

        let _ = tokio::task::spawn_blocking(move || hook_handle.stop()).await;
        self.manifest
    }

    fn next_deadline(&self) -> Option<Instant> {
        match (&self.pending_click, &self.pending_scroll) {
            (Some(c), Some(s)) => Some(c.deadline.min(s.deadline)),
            (Some(c), None) => Some(c.deadline),
            (None, Some(s)) => Some(s.deadline),
            (None, None) => None,
        }
    }

    async fn finalize_deadlines(&mut self) {
        let now = Instant::now();
        if self.pending_scroll.as_ref().is_some_and(|s| s.deadline <= now) {
            self.flush_pending_scroll().await;
        }
        if self.pending_click.as_ref().is_some_and(|c| c.deadline <= now) {
            self.flush_pending_click().await;
        }
    }

    fn norm(&self, x: i32, y: i32) -> (f64, f64) {
        let xn = (f64::from(x) / f64::from(self.display_w.max(1)) * 1000.0).clamp(0.0, 1000.0);
        let yn = (f64::from(y) / f64::from(self.display_h.max(1)) * 1000.0).clamp(0.0, 1000.0);
        (xn, yn)
    }

    async fn flush_type(&mut self) {
        if self.type_buffer.is_empty() {
            self.type_baseline = None;
            return;
        }
        let fallback = std::mem::take(&mut self.type_buffer);
        let baseline = self.type_baseline.take().flatten();
        let current = super::text_capture::focused_text().await;
        let text = super::text_capture::typed_delta(
            baseline.as_deref(),
            current.as_deref(),
            &fallback,
        );
        self.commit(StepInput {
            action_type: "type",
            value: Some(text),
            ..StepInput::default()
        })
        .await;
    }

    async fn flush_pending_click(&mut self) {
        if let Some(click) = self.pending_click.take() {
            let (xn, yn) = self.norm(click.x, click.y);
            self.commit(StepInput {
                action_type: "click",
                x_norm: Some(xn),
                y_norm: Some(yn),
                x_abs: Some(click.x),
                y_abs: Some(click.y),
                ..StepInput::default()
            })
            .await;
        }
    }

    async fn flush_pending_scroll(&mut self) {
        if let Some(scroll) = self.pending_scroll.take() {
            let (xn, yn) = self.norm(scroll.x, scroll.y);
            self.commit(StepInput {
                action_type: "scroll",
                x_norm: Some(xn),
                y_norm: Some(yn),
                x_abs: Some(scroll.x),
                y_abs: Some(scroll.y),
                value: Some(scroll.dir),
                amount: Some(scroll.amount),
                ..StepInput::default()
            })
            .await;
        }
    }

    async fn flush_all_pending(&mut self) {
        self.flush_type().await;
        self.flush_pending_scroll().await;
        self.flush_pending_click().await;
    }

    fn notify_own_filter(&mut self) {
        if self.own_filter_notified {
            return;
        }
        self.own_filter_notified = true;
        let _ = self.event_tx.send(RecorderEvent::status_code(
            super::types::RecorderStatus::Recording,
            "recorder_own_filter",
            "clicks on the assistant's own window are not recorded",
        ));
    }

    async fn handle(&mut self, event: RawInputEvent) {
        match event {
            RawInputEvent::Key {
                down,
                vk,
                scan,
                ctrl,
                alt,
                shift,
                win,
                caps,
            } => {
                let ch = if down {
                    hook::translate_vk(vk, scan, shift, caps)
                } else {
                    None
                };
                self.handle_key(down, vk, ch, ctrl, alt, shift, win).await;
            }
            RawInputEvent::MouseButton {
                button,
                down,
                x,
                y,
            } => {
                let own = hook::point_in_own_process(x, y);
                self.handle_button(button, down, x, y, own).await;
            }
            RawInputEvent::MouseMove { x, y } => {
                if let Some(ld) = self.left_down.as_mut() {
                    let dx = f64::from(x - ld.x);
                    let dy = f64::from(y - ld.y);
                    if (dx * dx + dy * dy).sqrt() > DRAG_THRESHOLD_PX {
                        ld.moved = true;
                    }
                }
            }
            RawInputEvent::Wheel {
                delta,
                horizontal,
                x,
                y,
            } => self.handle_wheel(delta, horizontal, x, y).await,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_key(
        &mut self,
        down: bool,
        vk: u16,
        ch: Option<char>,
        ctrl: bool,
        alt: bool,
        shift: bool,
        win: bool,
    ) {
        if !down {
            return;
        }
        if let Some(c) = ch {
            if !ctrl && !alt && !win {
                if self.type_buffer.is_empty() && self.type_baseline.is_none() {
                    self.type_baseline = Some(super::text_capture::focused_text().await);
                }
                self.type_buffer.push(c);
                return;
            }
        }
        self.flush_all_pending().await;
        if let Some(combo) = build_combo(vk, ctrl, alt, shift, win, ch) {
            self.commit(StepInput {
                action_type: "key_press",
                value: Some(combo),
                ..StepInput::default()
            })
            .await;
        }
    }

    async fn handle_button(
        &mut self,
        button: MouseButton,
        down: bool,
        x: i32,
        y: i32,
        own: bool,
    ) {
        match button {
            MouseButton::Left => {
                if down {
                    self.flush_type().await;
                    self.flush_pending_scroll().await;
                    self.left_down = Some(LeftDown {
                        x,
                        y,
                        own,
                        moved: false,
                        down_at: Instant::now(),
                    });
                } else if let Some(ld) = self.left_down.take() {
                    if ld.own || own {
                        self.notify_own_filter();
                        return;
                    }
                    let dx = f64::from(x - ld.x);
                    let dy = f64::from(y - ld.y);
                    let final_dist = (dx * dx + dy * dy).sqrt();
                    let elapsed = ld.down_at.elapsed().as_millis();
                    let quick_tap = elapsed < QUICK_CLICK_MS && final_dist < QUICK_CLICK_SLOP_PX;
                    let is_drag = ld.moved && final_dist >= DRAG_THRESHOLD_PX && !quick_tap;
                    if is_drag {
                        self.flush_pending_click().await;
                        let (fxn, fyn) = self.norm(ld.x, ld.y);
                        let (txn, tyn) = self.norm(x, y);
                        self.commit(StepInput {
                            action_type: "drag",
                            x_norm: Some(fxn),
                            y_norm: Some(fyn),
                            to_x_norm: Some(txn),
                            to_y_norm: Some(tyn),
                            x_abs: Some(ld.x),
                            y_abs: Some(ld.y),
                            to_x_abs: Some(x),
                            to_y_abs: Some(y),
                            ..StepInput::default()
                        })
                        .await;
                    } else if let Some(pc) = self.pending_click.take() {
                        if (pc.x - x).abs() <= DOUBLE_CLICK_DIST
                            && (pc.y - y).abs() <= DOUBLE_CLICK_DIST
                        {
                            let (xn, yn) = self.norm(x, y);
                            self.commit(StepInput {
                                action_type: "double_click",
                                x_norm: Some(xn),
                                y_norm: Some(yn),
                                x_abs: Some(x),
                                y_abs: Some(y),
                                ..StepInput::default()
                            })
                            .await;
                        } else {
                            let (pxn, pyn) = self.norm(pc.x, pc.y);
                            self.commit(StepInput {
                                action_type: "click",
                                x_norm: Some(pxn),
                                y_norm: Some(pyn),
                                x_abs: Some(pc.x),
                                y_abs: Some(pc.y),
                                ..StepInput::default()
                            })
                            .await;
                            self.pending_click = Some(PendingClick {
                                x,
                                y,
                                deadline: Instant::now()
                                    + Duration::from_millis(CLICK_FINALIZE_MS),
                            });
                        }
                    } else {
                        self.pending_click = Some(PendingClick {
                            x,
                            y,
                            deadline: Instant::now() + Duration::from_millis(CLICK_FINALIZE_MS),
                        });
                    }
                }
            }
            MouseButton::Right => {
                if !down {
                    if own {
                        self.notify_own_filter();
                        return;
                    }
                    self.flush_all_pending().await;
                    let (xn, yn) = self.norm(x, y);
                    self.commit(StepInput {
                        action_type: "right_click",
                        x_norm: Some(xn),
                        y_norm: Some(yn),
                        x_abs: Some(x),
                        y_abs: Some(y),
                        ..StepInput::default()
                    })
                    .await;
                }
            }
            MouseButton::Middle => {}
        }
    }

    async fn handle_wheel(&mut self, delta: i32, horizontal: bool, x: i32, y: i32) {
        self.flush_type().await;
        self.flush_pending_click().await;
        let dir = if horizontal {
            if delta > 0 {
                "right"
            } else {
                "left"
            }
        } else if delta > 0 {
            "up"
        } else {
            "down"
        };

        if let Some(scroll) = self.pending_scroll.as_mut() {
            if scroll.dir == dir {
                scroll.amount += 1;
                scroll.x = x;
                scroll.y = y;
                scroll.deadline = Instant::now() + Duration::from_millis(SCROLL_FINALIZE_MS);
                return;
            }
        }
        self.flush_pending_scroll().await;
        self.pending_scroll = Some(PendingScroll {
            dir: dir.to_string(),
            amount: 1,
            x,
            y,
            deadline: Instant::now() + Duration::from_millis(SCROLL_FINALIZE_MS),
        });
    }

    async fn commit(&mut self, input: StepInput) {
        if self.manifest.steps.len() >= MAX_STEPS {
            if !self.max_steps_notified {
                self.max_steps_notified = true;
                let _ = self.event_tx.send(RecorderEvent::status_code(
                    super::types::RecorderStatus::Recording,
                    "recorder_step_limit",
                    format!(
                        "step limit reached ({MAX_STEPS}); further actions will not be recorded"
                    ),
                ));
            }
            return;
        }
        let now = Instant::now();
        let delay = now
            .saturating_duration_since(self.last_commit)
            .as_millis()
            .min(u128::from(MAX_DELAY_MS)) as u64;
        self.last_commit = now;
        let index = self.manifest.steps.len() as u32;

        let allow_wait = !self.waited_first_frame;
        let frame = match self.frame_rx.as_mut() {
            Some(rx) => {
                let current = rx.borrow().clone();
                if current.is_some() {
                    current
                } else if allow_wait {
                    self.waited_first_frame = true;
                    match tokio::time::timeout(
                        Duration::from_millis(FIRST_FRAME_WAIT_MS),
                        rx.wait_for(|f| f.is_some()),
                    )
                    .await
                    {
                        Ok(Ok(guard)) => guard.clone(),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            None => None,
        };
        let mut screenshot_file = None;
        let mut screenshot_base64 = String::new();
        let mut frame_monitor = None;
        if let Some(frame) = frame {
            screenshot_base64 = frame.preview_jpeg_base64.to_string();
            let file = format!("shots/{index}.jpg");
            let path = self.dir.join(&file);
            let bytes = frame.shot_jpeg_bytes.clone();
            self.shot_writes.spawn(async move {
                let _ = tokio::fs::write(path, bytes.as_slice()).await;
            });
            screenshot_file = Some(file);
            frame_monitor = Some(frame.monitor);
        }

        let hit_monitor = input.x_abs.zip(input.y_abs).and_then(|(x, y)| {
            self.monitors
                .iter()
                .find(|m| m.contains(x, y))
                .copied()
        });
        let monitor = hit_monitor.or(frame_monitor);

        let (x_norm, y_norm, to_x_norm, to_y_norm) = match monitor {
            Some(rect) => {
                let primary = input
                    .x_abs
                    .zip(input.y_abs)
                    .map(|(x, y)| rect.normalize(x, y));
                let secondary = input
                    .to_x_abs
                    .zip(input.to_y_abs)
                    .map(|(x, y)| rect.normalize(x, y));
                (
                    primary.map(|p| p.0).or(input.x_norm),
                    primary.map(|p| p.1).or(input.y_norm),
                    secondary.map(|p| p.0).or(input.to_x_norm),
                    secondary.map(|p| p.1).or(input.to_y_norm),
                )
            }
            None => (input.x_norm, input.y_norm, input.to_x_norm, input.to_y_norm),
        };

        let step = RecordedStep {
            index,
            action_type: input.action_type.to_string(),
            x_norm,
            y_norm,
            to_x_norm,
            to_y_norm,
            x_abs: input.x_abs,
            y_abs: input.y_abs,
            to_x_abs: input.to_x_abs,
            to_y_abs: input.to_y_abs,
            value: input.value.clone(),
            amount: input.amount,
            delay_ms: delay,
            screenshot_file,
            element_description: None,
            monitor,
        };
        self.manifest.steps.push(step);

        let mut activity_payload = serde_json::json!({
            "action": input.action_type,
            "stepIndex": index,
        });
        if let Some(map) = activity_payload.as_object_mut() {
            if let Some(value) = input.value.as_deref() {
                map.insert("value".to_string(), serde_json::json!(value));
            }
            if let Some(amount) = input.amount {
                map.insert("amount".to_string(), serde_json::json!(amount));
            }
            if let (Some(x), Some(y)) = (x_norm, y_norm) {
                map.insert("xNorm".to_string(), serde_json::json!(x));
                map.insert("yNorm".to_string(), serde_json::json!(y));
            }
            if let Some(rect) = monitor {
                map.insert("monitor".to_string(), serde_json::json!(rect.id));
            }
        }
        self.activity.publish(
            &format!("input.{}", input.action_type),
            "input",
            activity_payload,
        );

        let _ = self.event_tx.send(RecorderEvent::Step {
            step: RecorderStepEvent {
                index,
                action_type: input.action_type.to_string(),
                element_description: None,
                value: input.value,
                screenshot_base64,
                target_x_norm: x_norm,
                target_y_norm: y_norm,
                to_x_norm,
                to_y_norm,
            },
        });
    }
}

#[derive(Default)]
struct StepInput {
    action_type: &'static str,
    x_norm: Option<f64>,
    y_norm: Option<f64>,
    to_x_norm: Option<f64>,
    to_y_norm: Option<f64>,
    x_abs: Option<i32>,
    y_abs: Option<i32>,
    to_x_abs: Option<i32>,
    to_y_abs: Option<i32>,
    value: Option<String>,
    amount: Option<i32>,
}

fn build_combo(
    vk: u16,
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
    ch: Option<char>,
) -> Option<String> {
    let key = key_name(vk).or_else(|| ch.map(|c| c.to_ascii_lowercase().to_string()))?;
    let mut parts: Vec<String> = Vec::new();
    if ctrl {
        parts.push("ctrl".to_string());
    }
    if alt {
        parts.push("alt".to_string());
    }
    if shift {
        parts.push("shift".to_string());
    }
    if win {
        parts.push("win".to_string());
    }
    parts.push(key);
    Some(parts.join("+"))
}

fn key_name(vk: u16) -> Option<String> {
    let name = match vk {
        0x0D => "enter",
        0x09 => "tab",
        0x1B => "esc",
        0x20 => "space",
        0x08 => "backspace",
        0x2E => "delete",
        0x26 => "up",
        0x28 => "down",
        0x25 => "left",
        0x27 => "right",
        0x24 => "home",
        0x23 => "end",
        0x21 => "pageup",
        0x22 => "pagedown",
        0x70 => "f1",
        0x71 => "f2",
        0x72 => "f3",
        0x73 => "f4",
        0x74 => "f5",
        0x75 => "f6",
        0x76 => "f7",
        0x77 => "f8",
        0x78 => "f9",
        0x79 => "f10",
        0x7A => "f11",
        0x7B => "f12",
        0x30..=0x39 => return Some(((vk as u8) as char).to_string()),
        0x41..=0x5A => return Some(((vk as u8) as char).to_ascii_lowercase().to_string()),
        _ => return None,
    };
    Some(name.to_string())
}
