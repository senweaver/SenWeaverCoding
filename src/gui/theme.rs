// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Cursor-inspired light theme for the SenWeaverCoding desktop GUI.

use egui::{Color32, CornerRadius, FontId, Stroke, Vec2};

// ---------------------------------------------------------------------------
// Color palette (sampled from Cursor screenshots)
// ---------------------------------------------------------------------------

pub const BG_PRIMARY: Color32 = Color32::WHITE;
pub const BG_SIDEBAR: Color32 = Color32::from_rgb(247, 247, 248);
pub const BG_INPUT: Color32 = Color32::WHITE;
pub const BG_HOVER: Color32 = Color32::from_rgb(243, 243, 243);
pub const BG_CARD: Color32 = Color32::WHITE;

pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(26, 26, 26);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(107, 107, 107);
pub const TEXT_PLACEHOLDER: Color32 = Color32::from_rgb(180, 180, 180);

pub const ACCENT_ORANGE: Color32 = Color32::from_rgb(249, 115, 22);
pub const ACCENT_GREEN: Color32 = Color32::from_rgb(34, 197, 94);
pub const ACCENT_BLUE: Color32 = Color32::from_rgb(59, 130, 246);
pub const ACCENT_RED: Color32 = Color32::from_rgb(239, 68, 68);

pub const BORDER: Color32 = Color32::from_rgb(229, 229, 229);
pub const BORDER_FOCUS: Color32 = Color32::from_rgb(59, 130, 246);

pub const TOGGLE_ON: Color32 = Color32::from_rgb(16, 185, 129);
pub const TOGGLE_OFF: Color32 = Color32::from_rgb(209, 213, 219);

pub const USER_BUBBLE: Color32 = Color32::from_rgb(239, 246, 255);
pub const ASSISTANT_BUBBLE: Color32 = Color32::from_rgb(249, 250, 251);

// ---------------------------------------------------------------------------
// Typography
// ---------------------------------------------------------------------------

pub fn font_heading() -> FontId {
    FontId::proportional(18.0)
}

pub fn font_body() -> FontId {
    FontId::proportional(14.0)
}

pub fn font_small() -> FontId {
    FontId::proportional(12.0)
}

pub fn font_mono() -> FontId {
    FontId::monospace(13.0)
}

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------

pub const SIDEBAR_WIDTH: f32 = 200.0;
pub const SETTINGS_NAV_WIDTH: f32 = 180.0;
pub const CARD_ROUNDING: CornerRadius = CornerRadius::same(8);
pub const BUTTON_ROUNDING: CornerRadius = CornerRadius::same(6);
pub const INPUT_ROUNDING: CornerRadius = CornerRadius::same(10);
pub const TAG_ROUNDING: CornerRadius = CornerRadius::same(12);
pub const POPUP_ROUNDING: CornerRadius = CornerRadius::same(10);
pub const SPACING: Vec2 = Vec2::new(8.0, 8.0);

pub fn card_stroke() -> Stroke {
    Stroke::new(1.0, BORDER)
}

pub fn input_stroke() -> Stroke {
    Stroke::new(1.0, BORDER)
}

pub fn popup_shadow() -> egui::epaint::Shadow {
    egui::epaint::Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 2,
        color: Color32::from_black_alpha(20),
    }
}

// ---------------------------------------------------------------------------
// Apply theme to egui context
// ---------------------------------------------------------------------------

pub fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    style.spacing.item_spacing = SPACING;
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.window_margin = egui::Margin::same(12);

    style.visuals.window_fill = BG_PRIMARY;
    style.visuals.panel_fill = BG_PRIMARY;
    style.visuals.window_stroke = card_stroke();

    style.visuals.widgets.noninteractive.bg_fill = BG_PRIMARY;
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    style.visuals.widgets.inactive.bg_fill = BG_PRIMARY;
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    style.visuals.widgets.hovered.bg_fill = BG_HOVER;
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    style.visuals.widgets.active.bg_fill = Color32::from_rgb(230, 230, 230);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    style.visuals.selection.bg_fill = Color32::from_rgba_premultiplied(59, 130, 246, 40);
    style.visuals.selection.stroke = Stroke::new(1.0, ACCENT_BLUE);

    ctx.set_style(style);
}
