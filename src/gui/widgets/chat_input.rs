// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
use crate::gui::state::{AgentMode, AppState};
use crate::gui::theme;
use crate::gui::widgets::tag_chip;

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let frame = egui::Frame::NONE
        .fill(theme::BG_INPUT)
        .corner_radius(theme::INPUT_ROUNDING)
        .stroke(theme::input_stroke())
        .inner_margin(egui::Margin::same(12));

    frame.show(ui, |ui| {
        // Multi-line text input
        let response = ui.add(
            egui::TextEdit::multiline(&mut state.chat_input)
                .hint_text("Plan and design before coding...")
                .desired_width(f32::INFINITY)
                .desired_rows(2)
                .font(theme::font_body())
                .frame(false)
                .margin(egui::Margin::ZERO),
        );
        let _ = response;

        ui.add_space(4.0);

        // Toolbar row
        ui.horizontal(|ui| {
            // "+" button to open mode popup
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("+")
                            .font(theme::font_body())
                            .strong()
                            .color(theme::TEXT_SECONDARY),
                    )
                    .corner_radius(egui::CornerRadius::same(14))
                    .min_size(egui::vec2(28.0, 28.0)),
                )
                .clicked()
            {
                state.show_mode_popup = !state.show_mode_popup;
            }

            // Active mode tags
            let mut to_remove = Vec::new();
            for (i, mode) in state.active_modes.iter().enumerate() {
                let label = match mode {
                    AgentMode::Plan => "\u{2699} Plan",
                    AgentMode::Debug => "\u{1F41B} Debug",
                    AgentMode::Ask => "\u{2753} Ask",
                    AgentMode::Image => "\u{1F5BC} Image",
                };
                let color = match mode {
                    AgentMode::Plan => theme::ACCENT_ORANGE,
                    AgentMode::Debug => theme::ACCENT_RED,
                    AgentMode::Ask => theme::ACCENT_BLUE,
                    AgentMode::Image => theme::ACCENT_GREEN,
                };
                if tag_chip::show(ui, label, color) {
                    to_remove.push(i);
                }
            }
            for i in to_remove.into_iter().rev() {
                state.active_modes.remove(i);
            }

            // Model selector button
            let model_btn = ui.add(
                egui::Button::new(
                    egui::RichText::new(&format!("{} \u{25BE}", state.selected_model))
                        .font(theme::font_small())
                        .color(theme::TEXT_SECONDARY),
                )
                .fill(egui::Color32::TRANSPARENT),
            );
            if model_btn.clicked() {
                state.show_model_picker = !state.show_model_picker;
            }

            // Spacer
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Voice button (mic icon)
                ui.add(
                    egui::Button::new(
                        egui::RichText::new("\u{1F3A4}")
                            .font(theme::font_body())
                            .color(theme::TEXT_SECONDARY),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .corner_radius(egui::CornerRadius::same(14)),
                );
            });
        });
    });
}
