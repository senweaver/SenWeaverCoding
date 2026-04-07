// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
use crate::gui::state::{AgentMode, AppState};
use crate::gui::theme;

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_mode_popup {
        return;
    }

    egui::Area::new(egui::Id::new("mode_popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(220.0, 500.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .corner_radius(theme::POPUP_ROUNDING)
                .shadow(theme::popup_shadow())
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_min_width(220.0);

                    ui.label(
                        egui::RichText::new("Add agents, context, tools...")
                            .font(theme::font_small())
                            .color(theme::TEXT_PLACEHOLDER),
                    );
                    ui.add_space(4.0);

                    let modes = [
                        (AgentMode::Plan, "\u{2699}", "Plan"),
                        (AgentMode::Debug, "\u{1F41B}", "Debug"),
                        (AgentMode::Ask, "\u{2753}", "Ask"),
                        (AgentMode::Image, "\u{1F5BC}", "Image"),
                    ];

                    for (mode, icon, label) in &modes {
                        let is_active = state.active_modes.contains(mode);
                        let btn = ui.add(
                            egui::Button::new(
                                egui::RichText::new(format!(
                                    "{icon}  {label}{}",
                                    if is_active { "  \u{2713}" } else { "" }
                                ))
                                .font(theme::font_body())
                                .color(if is_active {
                                    theme::ACCENT_ORANGE
                                } else {
                                    theme::TEXT_PRIMARY
                                }),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .min_size(egui::vec2(ui.available_width(), 30.0)),
                        );
                        if btn.clicked() {
                            if is_active {
                                state.active_modes.retain(|m| m != mode);
                            } else {
                                state.active_modes.push(*mode);
                            }
                        }
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // Skills and MCP Servers as sub-menu items
                    for label in &["Skills  \u{25B8}", "MCP Servers  \u{25B8}"] {
                        ui.add(
                            egui::Button::new(
                                egui::RichText::new(*label)
                                    .font(theme::font_body())
                                    .color(theme::TEXT_PRIMARY),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .min_size(egui::vec2(ui.available_width(), 30.0)),
                        );
                    }
                });
        });

    // Close popup when clicking outside
    if ctx.input(|i| i.pointer.any_click()) {
        let popup_rect = egui::Rect::from_min_size(egui::pos2(220.0, 500.0), egui::vec2(240.0, 250.0));
        if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
            if !popup_rect.contains(pos) {
                state.show_mode_popup = false;
            }
        }
    }
}
