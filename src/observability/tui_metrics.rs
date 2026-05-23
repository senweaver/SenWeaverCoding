// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

pub const STREAMING_TOKEN_LATENCY_BUCKETS_MS: [f64; 9] =
    [1.0, 5.0, 10.0, 20.0, 30.0, 50.0, 100.0, 200.0, 500.0];

#[derive(Debug, Default)]
pub struct TuiMetrics {

    pub frame_draws: AtomicU64,

    pub frame_skipped_dirty: AtomicU64,

    pub input_events: AtomicU64,

    pub session_deltas: AtomicU64,

    pub legacy_loop_activated: AtomicU64,

    pub highlight_cache_hit: AtomicU64,
    pub highlight_cache_miss: AtomicU64,

    pub chat_lines_rendered: AtomicU64,

    pub viewport_render: AtomicU64,

    pub diff_review_opened: AtomicU64,
    pub diff_review_apply_hunk: AtomicU64,
    pub diff_review_reject_hunk: AtomicU64,
    pub diff_review_apply_file: AtomicU64,
    pub diff_review_reject_file: AtomicU64,
    pub diff_review_comment: AtomicU64,

    pub file_viewer_opened: AtomicU64,
    pub file_viewer_file_open: AtomicU64,
    pub file_viewer_mark_focus: AtomicU64,
    pub file_viewer_search: AtomicU64,

    pub inline_edit_triggered: AtomicU64,
    pub inline_edit_success: AtomicU64,
    pub inline_edit_failed: AtomicU64,

    pub chat_reconcile_full: AtomicU64,
    pub chat_reconcile_incremental: AtomicU64,
    pub chat_reconcile_noop: AtomicU64,

    pub chat_messages_version: AtomicU64,

    pub streaming_token_latency_ms: Mutex<LatencyHistogram>,
}

#[derive(Debug, Clone, Default)]
pub struct LatencyHistogram {
    pub bucket_counts: [u64; STREAMING_TOKEN_LATENCY_BUCKETS_MS.len()],
    pub inf_count: u64,
    pub sum_ms: f64,
    pub count: u64,
}

impl LatencyHistogram {
    pub fn observe(&mut self, millis: f64) {
        for (idx, boundary) in STREAMING_TOKEN_LATENCY_BUCKETS_MS.iter().enumerate() {
            if millis <= *boundary {
                self.bucket_counts[idx] += 1;
            }
        }
        self.inf_count += 1;
        self.sum_ms += millis;
        self.count += 1;
    }
}

impl TuiMetrics {
    pub fn snapshot(&self) -> TuiSnapshot {
        TuiSnapshot {
            frame_draws: self.frame_draws.load(Ordering::Relaxed),
            frame_skipped_dirty: self.frame_skipped_dirty.load(Ordering::Relaxed),
            input_events: self.input_events.load(Ordering::Relaxed),
            session_deltas: self.session_deltas.load(Ordering::Relaxed),
            legacy_loop_activated: self.legacy_loop_activated.load(Ordering::Relaxed),

            highlight_cache_hit: self.highlight_cache_hit.load(Ordering::Relaxed),
            highlight_cache_miss: self.highlight_cache_miss.load(Ordering::Relaxed),
            chat_lines_rendered: self.chat_lines_rendered.load(Ordering::Relaxed),
            viewport_render: self.viewport_render.load(Ordering::Relaxed),

            diff_review_opened: self.diff_review_opened.load(Ordering::Relaxed),
            diff_review_apply_hunk: self.diff_review_apply_hunk.load(Ordering::Relaxed),
            diff_review_reject_hunk: self.diff_review_reject_hunk.load(Ordering::Relaxed),
            diff_review_apply_file: self.diff_review_apply_file.load(Ordering::Relaxed),
            diff_review_reject_file: self.diff_review_reject_file.load(Ordering::Relaxed),
            diff_review_comment: self.diff_review_comment.load(Ordering::Relaxed),

            file_viewer_opened: self.file_viewer_opened.load(Ordering::Relaxed),
            file_viewer_file_open: self.file_viewer_file_open.load(Ordering::Relaxed),
            file_viewer_mark_focus: self.file_viewer_mark_focus.load(Ordering::Relaxed),
            file_viewer_search: self.file_viewer_search.load(Ordering::Relaxed),

            inline_edit_triggered: self.inline_edit_triggered.load(Ordering::Relaxed),
            inline_edit_success: self.inline_edit_success.load(Ordering::Relaxed),
            inline_edit_failed: self.inline_edit_failed.load(Ordering::Relaxed),

            chat_reconcile_full: self.chat_reconcile_full.load(Ordering::Relaxed),
            chat_reconcile_incremental: self.chat_reconcile_incremental.load(Ordering::Relaxed),
            chat_reconcile_noop: self.chat_reconcile_noop.load(Ordering::Relaxed),
            chat_messages_version: self.chat_messages_version.load(Ordering::Relaxed),

            streaming_token_latency_ms: self
                .streaming_token_latency_ms
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TuiSnapshot {
    pub frame_draws: u64,
    pub frame_skipped_dirty: u64,
    pub input_events: u64,
    pub session_deltas: u64,
    pub legacy_loop_activated: u64,

    pub highlight_cache_hit: u64,
    pub highlight_cache_miss: u64,
    pub chat_lines_rendered: u64,
    pub viewport_render: u64,

    pub diff_review_opened: u64,
    pub diff_review_apply_hunk: u64,
    pub diff_review_reject_hunk: u64,
    pub diff_review_apply_file: u64,
    pub diff_review_reject_file: u64,
    pub diff_review_comment: u64,

    pub file_viewer_opened: u64,
    pub file_viewer_file_open: u64,
    pub file_viewer_mark_focus: u64,
    pub file_viewer_search: u64,

    pub inline_edit_triggered: u64,
    pub inline_edit_success: u64,
    pub inline_edit_failed: u64,

    pub chat_reconcile_full: u64,
    pub chat_reconcile_incremental: u64,
    pub chat_reconcile_noop: u64,
    pub chat_messages_version: u64,

    pub streaming_token_latency_ms: LatencyHistogram,
}

impl TuiSnapshot {
    pub fn render_prometheus_text(&self) -> String {
        let mut out = String::new();
        macro_rules! counter {
            ($metric:literal, $val:expr) => {
                out.push_str(&format!(
                    "# TYPE {name} counter\n{name} {val}\n",
                    name = $metric,
                    val = $val
                ));
            };
        }

        counter!("sen_tui_frame_draws_total", self.frame_draws);
        counter!(
            "sen_tui_frame_skipped_dirty_total",
            self.frame_skipped_dirty
        );
        counter!("sen_tui_input_events_total", self.input_events);
        counter!("sen_tui_session_deltas_total", self.session_deltas);
        counter!(
            "sen_tui_legacy_loop_activated_total",
            self.legacy_loop_activated
        );

        counter!(
            "sen_tui_highlight_cache_hit_total",
            self.highlight_cache_hit
        );
        counter!(
            "sen_tui_highlight_cache_miss_total",
            self.highlight_cache_miss
        );
        counter!(
            "sen_tui_chat_lines_rendered_total",
            self.chat_lines_rendered
        );
        counter!("sen_tui_viewport_render_total", self.viewport_render);

        counter!("sen_tui_diff_review_opened_total", self.diff_review_opened);
        counter!(
            "sen_tui_diff_review_apply_hunk_total",
            self.diff_review_apply_hunk
        );
        counter!(
            "sen_tui_diff_review_reject_hunk_total",
            self.diff_review_reject_hunk
        );
        counter!(
            "sen_tui_diff_review_apply_file_total",
            self.diff_review_apply_file
        );
        counter!(
            "sen_tui_diff_review_reject_file_total",
            self.diff_review_reject_file
        );
        counter!(
            "sen_tui_diff_review_comment_total",
            self.diff_review_comment
        );

        counter!("sen_tui_file_viewer_opened_total", self.file_viewer_opened);
        counter!(
            "sen_tui_file_viewer_file_open_total",
            self.file_viewer_file_open
        );
        counter!(
            "sen_tui_file_viewer_mark_focus_total",
            self.file_viewer_mark_focus
        );
        counter!("sen_tui_file_viewer_search_total", self.file_viewer_search);

        counter!(
            "sen_tui_inline_edit_triggered_total",
            self.inline_edit_triggered
        );
        counter!(
            "sen_tui_inline_edit_success_total",
            self.inline_edit_success
        );
        counter!("sen_tui_inline_edit_failed_total", self.inline_edit_failed);

        counter!(
            "sen_tui_chat_reconcile_full_total",
            self.chat_reconcile_full
        );
        counter!(
            "sen_tui_chat_reconcile_incremental_total",
            self.chat_reconcile_incremental
        );
        counter!(
            "sen_tui_chat_reconcile_noop_total",
            self.chat_reconcile_noop
        );
        out.push_str(&format!(
            "# TYPE sen_tui_chat_messages_version gauge\nsen_tui_chat_messages_version {}\n",
            self.chat_messages_version
        ));

        let h = &self.streaming_token_latency_ms;
        out.push_str("# TYPE sen_tui_streaming_token_latency_ms histogram\n");
        for (idx, boundary) in STREAMING_TOKEN_LATENCY_BUCKETS_MS.iter().enumerate() {
            out.push_str(&format!(
                "sen_tui_streaming_token_latency_ms_bucket{{le=\"{boundary}\"}} {}\n",
                h.bucket_counts[idx]
            ));
        }
        out.push_str(&format!(
            "sen_tui_streaming_token_latency_ms_bucket{{le=\"+Inf\"}} {}\n",
            h.inf_count
        ));
        out.push_str(&format!(
            "sen_tui_streaming_token_latency_ms_sum {}\n",
            h.sum_ms
        ));
        out.push_str(&format!(
            "sen_tui_streaming_token_latency_ms_count {}\n",
            h.count
        ));

        out
    }
}

static METRICS: OnceLock<TuiMetrics> = OnceLock::new();

pub fn global() -> &'static TuiMetrics {
    METRICS.get_or_init(TuiMetrics::default)
}

pub fn incr_tui_frame_draws() {
    global().frame_draws.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_frame_skipped_dirty() {
    global().frame_skipped_dirty.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_input_events() {
    global().input_events.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_session_deltas() {
    global().session_deltas.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_legacy_loop_activated() {
    global()
        .legacy_loop_activated
        .fetch_add(1, Ordering::Relaxed);
}
pub fn observe_tui_streaming_token_latency_ms(millis: f64) {
    if let Ok(mut guard) = global().streaming_token_latency_ms.lock() {
        guard.observe(millis);
    }
}

pub fn incr_tui_highlight_cache_hit() {
    global().highlight_cache_hit.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_highlight_cache_miss() {
    global()
        .highlight_cache_miss
        .fetch_add(1, Ordering::Relaxed);
}
pub fn add_tui_chat_lines_rendered(n: u64) {
    global().chat_lines_rendered.fetch_add(n, Ordering::Relaxed);
}
pub fn incr_tui_viewport_render() {
    global().viewport_render.fetch_add(1, Ordering::Relaxed);
}

pub fn incr_tui_diff_review_opened() {
    global().diff_review_opened.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_diff_review_apply_hunk() {
    global()
        .diff_review_apply_hunk
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_diff_review_reject_hunk() {
    global()
        .diff_review_reject_hunk
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_diff_review_apply_file() {
    global()
        .diff_review_apply_file
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_diff_review_reject_file() {
    global()
        .diff_review_reject_file
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_diff_review_comment() {
    global().diff_review_comment.fetch_add(1, Ordering::Relaxed);
}

pub fn incr_tui_file_viewer_opened() {
    global().file_viewer_opened.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_file_viewer_file_open() {
    global()
        .file_viewer_file_open
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_file_viewer_mark_focus() {
    global()
        .file_viewer_mark_focus
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_file_viewer_search() {
    global().file_viewer_search.fetch_add(1, Ordering::Relaxed);
}

pub fn incr_tui_inline_edit_triggered() {
    global()
        .inline_edit_triggered
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_inline_edit_success() {
    global().inline_edit_success.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_inline_edit_failed() {
    global().inline_edit_failed.fetch_add(1, Ordering::Relaxed);
}

pub fn incr_tui_chat_reconcile_full() {
    global().chat_reconcile_full.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_chat_reconcile_incremental() {
    global()
        .chat_reconcile_incremental
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_tui_chat_reconcile_noop() {
    global().chat_reconcile_noop.fetch_add(1, Ordering::Relaxed);
}

pub fn set_tui_chat_messages_version(v: u64) {
    global()
        .chat_messages_version
        .store(v, Ordering::Relaxed);
}
