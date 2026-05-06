// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! TUI theme and color palette for SenWeaverCoding.
//!
//! Provides a consistent visual style across all TUI screens with
//! semantic color assignments for different UI elements.

use ratatui::style::{Color, Modifier, Style};

pub const ACCENT: Color = Color::Rgb(0, 180, 120);
pub const ACCENT_DIM: Color = Color::Rgb(0, 120, 80);
pub const BG: Color = Color::Rgb(20, 20, 30);
pub const BG_HIGHLIGHT: Color = Color::Rgb(30, 30, 45);
pub const FG: Color = Color::Rgb(220, 220, 230);
pub const FG_DIM: Color = Color::Rgb(120, 120, 140);
pub const ERROR: Color = Color::Rgb(220, 50, 50);
pub const WARNING: Color = Color::Rgb(220, 180, 50);
pub const SUCCESS: Color = Color::Rgb(50, 200, 80);
pub const INFO: Color = Color::Rgb(80, 160, 220);

pub fn title() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn selected() -> Style {
    Style::default()
        .fg(BG)
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::default().fg(FG_DIM)
}

pub fn normal() -> Style {
    Style::default().fg(FG)
}

pub fn error_style() -> Style {
    Style::default().fg(ERROR).add_modifier(Modifier::BOLD)
}

pub fn warning_style() -> Style {
    Style::default().fg(WARNING)
}

pub fn success_style() -> Style {
    Style::default().fg(SUCCESS)
}

pub fn info_style() -> Style {
    Style::default().fg(INFO)
}

pub fn tab_active() -> Style {
    Style::default()
        .fg(ACCENT)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}

pub fn tab_inactive() -> Style {
    Style::default().fg(FG_DIM)
}

pub fn status_badge(ok: bool) -> Style {
    if ok {
        Style::default().fg(SUCCESS)
    } else {
        Style::default().fg(ERROR)
    }
}

pub fn style_for_role(role: &str) -> Style {
    match role {
        "user" => info_style(),
        "assistant" => success_style(),
        "system" => Style::default().fg(ACCENT).add_modifier(Modifier::ITALIC),
        "tool" => Style::default().fg(WARNING),
        "error" => error_style(),
        _ => dim(),
    }
}

pub fn style_for_tool_result(success: bool) -> Style {
    if success {
        success_style()
    } else {
        error_style()
    }
}

pub fn thinking_style() -> Style {
    Style::default()
        .fg(ACCENT_DIM)
        .add_modifier(Modifier::ITALIC)
}

pub const SPINNER_FRAMES: &[&str] = &["◐", "◓", "◑", "◒", "◐", "◓", "◑", "◒", "◐", "◓"];
