// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;
use std::time::Duration;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::inline_completion::{
    InlineCompletionError, InlineCompletionRequest, Language, RegistryHandle,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhostText {
    pub insert_text: String,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhostState {

    Idle,

    Pending,

    Visible(GhostText),

    Accepted(GhostText),
}

impl Default for GhostState {
    fn default() -> Self {
        Self::Idle
    }
}

struct Inner {
    state: GhostState,
    pending_prefix: String,
    pending_token: Option<CancellationToken>,
    pending_join: Option<JoinHandle<()>>,
    last_request_id: u64,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            state: GhostState::Idle,
            pending_prefix: String::new(),
            pending_token: None,
            pending_join: None,
            last_request_id: 0,
        }
    }
}

pub struct GhostOverlay {
    registry: Option<RegistryHandle>,
    inner: Arc<Mutex<Inner>>,

    min_prefix_chars: usize,

    debounce: Duration,
}

impl std::fmt::Debug for GhostOverlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GhostOverlay")
            .field("has_registry", &self.registry.is_some())
            .field("min_prefix_chars", &self.min_prefix_chars)
            .field("debounce", &self.debounce)
            .finish()
    }
}

impl Default for GhostOverlay {
    fn default() -> Self {
        Self {
            registry: None,
            inner: Arc::new(Mutex::new(Inner::default())),
            min_prefix_chars: 3,
            debounce: Duration::from_millis(120),
        }
    }
}

impl GhostOverlay {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_registry(mut self, registry: RegistryHandle) -> Self {
        self.registry = Some(registry);
        self
    }

    #[must_use]
    pub fn with_min_prefix_chars(mut self, n: usize) -> Self {
        self.min_prefix_chars = n;
        self
    }

    #[must_use]
    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }

    pub async fn is_visible(&self) -> bool {
        matches!(self.inner.lock().await.state, GhostState::Visible(_))
    }

    pub async fn current(&self) -> Option<GhostText> {
        match &self.inner.lock().await.state {
            GhostState::Visible(g) | GhostState::Accepted(g) => Some(g.clone()),
            _ => None,
        }
    }

    pub async fn dismiss(&self) {
        let mut g = self.inner.lock().await;
        if let Some(tok) = g.pending_token.take() {
            tok.cancel();
        }
        if let Some(h) = g.pending_join.take() {
            h.abort();
        }
        g.state = GhostState::Idle;
    }

    pub async fn accept(&self) -> Option<GhostText> {
        let mut g = self.inner.lock().await;
        match std::mem::replace(&mut g.state, GhostState::Idle) {
            GhostState::Visible(t) | GhostState::Accepted(t) => {
                let copy = t.clone();
                g.state = GhostState::Accepted(t);
                Some(copy)
            }
            other => {
                g.state = other;
                None
            }
        }
    }

    pub async fn on_after_render(&self) {
        let mut g = self.inner.lock().await;
        if matches!(g.state, GhostState::Accepted(_)) {
            g.state = GhostState::Idle;
        }
    }

    pub async fn on_typing(
        &self,
        prefix: String,
        suffix: String,
        language: Language,
        file_path: std::path::PathBuf,
        workspace_root: std::path::PathBuf,
    ) {
        let Some(registry) = self.registry.clone() else {
            return;
        };
        if prefix.chars().count() < self.min_prefix_chars {
            self.dismiss().await;
            return;
        }

        let mut guard = self.inner.lock().await;

        if let Some(tok) = guard.pending_token.take() {
            tok.cancel();
        }
        if let Some(h) = guard.pending_join.take() {
            h.abort();
        }

        guard.last_request_id = guard.last_request_id.wrapping_add(1);
        let req_id = guard.last_request_id;
        let token = CancellationToken::new();
        guard.pending_token = Some(token.clone());
        guard.pending_prefix = prefix.clone();
        guard.state = GhostState::Pending;
        drop(guard);

        let inner = self.inner.clone();
        let debounce = self.debounce;

        let join = tokio::spawn(async move {
            tokio::select! {
                _ = token.cancelled() => return,
                _ = tokio::time::sleep(debounce) => {}
            }

            let context =
                crate::inline_completion::context_builder::build_context_from_window(
                    &prefix, &suffix,
                );
            let req = InlineCompletionRequest {
                prefix,
                suffix,
                language,
                file_path,
                workspace_root,
                context,
                max_tokens: 96,
                stop_sequences: Vec::new(),
                request_id: uuid::Uuid::new_v4(),
            };

            let outcome = tokio::select! {
                _ = token.cancelled() => return,
                r = registry.request(req) => r,
            };

            let mut guard = inner.lock().await;
            if guard.last_request_id != req_id {
                return;
            }
            match outcome {
                Ok(resp) => {
                    if let Some(first) = resp.suggestions.first() {
                        let ghost = GhostText {
                            insert_text: first.insert_text.clone(),
                            provider: resp.provider,
                        };
                        guard.state = GhostState::Visible(ghost);
                    } else {
                        guard.state = GhostState::Idle;
                    }
                }
                Err(InlineCompletionError::Empty { .. }) => {
                    guard.state = GhostState::Idle;
                }
                Err(_) => {
                    guard.state = GhostState::Idle;
                }
            }
        });

        let mut g2 = self.inner.lock().await;
        g2.pending_join = Some(join);
    }

    pub async fn render_line(&self, area: Rect) -> Option<Line<'static>> {
        let g = self.inner.lock().await;
        let GhostState::Visible(ghost) = &g.state else {
            return None;
        };
        if area.width == 0 || area.height == 0 {
            return None;
        }
        let mut text = ghost.insert_text.clone();

        if let Some(idx) = text.find('\n') {
            text.truncate(idx);
        }
        let style = Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC);
        Some(Line::from(vec![Span::styled(text, style)]))
    }
}

