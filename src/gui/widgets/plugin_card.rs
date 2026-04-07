// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
use crate::gui::state::PluginCard;
use crate::gui::theme;

pub fn show(ui: &mut egui::Ui, plugin: &PluginCard) -> bool {
    let mut clicked = false;

    let frame = egui::Frame::NONE
        .fill(theme::BG_CARD)
        .corner_radius(theme::CARD_ROUNDING)
        .stroke(theme::card_stroke())
        .inner_margin(egui::Margin::same(12));

    let response = frame
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                // Icon circle
                let (rect, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 8.0, theme::BG_HOVER);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    plugin.icon_char,
                    theme::font_body(),
                    theme::TEXT_PRIMARY,
                );

                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(&plugin.name)
                            .font(theme::font_body())
                            .color(theme::TEXT_PRIMARY)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(&plugin.description)
                            .font(theme::font_small())
                            .color(theme::TEXT_SECONDARY),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("\u{25B8}")
                            .font(theme::font_body())
                            .color(theme::TEXT_SECONDARY),
                    );
                });
            });
        })
        .response;

    if response.clicked() {
        clicked = true;
    }

    clicked
}
