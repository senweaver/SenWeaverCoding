// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
use crate::gui::bridge::GuiBridge;
use crate::gui::state::AppState;
use crate::gui::theme;
use crate::gui::widgets::{chat_input, message, mode_popup, model_picker, sidebar};

pub fn show(ctx: &egui::Context, state: &mut AppState, _bridge: &GuiBridge) {
    // Sidebar
    sidebar::show(ctx, state);

    // Popups (drawn on foreground layer)
    mode_popup::show(ctx, state);
    model_picker::show(ctx, state);

    // Central panel
    egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .fill(theme::BG_PRIMARY)
                .inner_margin(egui::Margin::same(0)),
        )
        .show(ctx, |ui| {
            // Top bar: Home label + settings icon
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new("Home")
                        .font(theme::font_heading())
                        .color(theme::TEXT_PRIMARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(16.0);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("\u{2699}")
                                    .font(theme::font_body())
                                    .color(theme::TEXT_SECONDARY),
                            )
                            .fill(egui::Color32::TRANSPARENT),
                        )
                        .clicked()
                    {
                        state.current_page = crate::gui::state::Page::Settings;
                    }
                });
            });

            ui.separator();

            // Message area
            let input_height = 120.0;
            let available = ui.available_height() - input_height;

            egui::ScrollArea::vertical()
                .max_height(available)
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.add_space(12.0);
                    let margin = 40.0;
                    ui.set_min_width(ui.available_width() - margin * 2.0);

                    if state.messages.is_empty() {
                        // Empty state — centered prompt
                        ui.vertical_centered(|ui| {
                            ui.add_space(available * 0.3);
                            ui.label(
                                egui::RichText::new("What can I help you ship?")
                                    .font(egui::FontId::proportional(24.0))
                                    .color(theme::TEXT_PRIMARY),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("Ask Sen to build, debug, or analyze your codebase.")
                                    .font(theme::font_body())
                                    .color(theme::TEXT_SECONDARY),
                            );
                        });
                    } else {
                        for msg in &state.messages {
                            message::show(ui, msg);
                        }

                        if state.is_agent_busy {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(
                                    egui::RichText::new("Thinking...")
                                        .font(theme::font_small())
                                        .color(theme::TEXT_SECONDARY),
                                );
                            });
                        }
                    }
                });

            // Chat input at bottom
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.add_space(8.0);
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(16, 8))
                    .show(ui, |ui| {
                        chat_input::show(ui, state);
                    });
            });
        });
}
