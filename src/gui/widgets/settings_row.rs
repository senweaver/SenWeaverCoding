// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
use crate::gui::theme;

/// A settings row with a label, description, and a toggle switch.
pub fn toggle_row(ui: &mut egui::Ui, label: &str, description: &str, value: &mut bool) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(label)
                    .font(theme::font_body())
                    .color(theme::TEXT_PRIMARY),
            );
            if !description.is_empty() {
                ui.label(
                    egui::RichText::new(description)
                        .font(theme::font_small())
                        .color(theme::TEXT_SECONDARY),
                );
            }
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(toggle_widget(value));
        });
    });

    ui.add_space(4.0);
}

/// A settings section header.
pub fn section_header(ui: &mut egui::Ui, title: &str) {
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(title)
            .font(theme::font_small())
            .color(theme::TEXT_SECONDARY)
            .strong(),
    );
    ui.add_space(4.0);
}

/// A settings row with a button on the right.
pub fn button_row(ui: &mut egui::Ui, label: &str, description: &str, button_text: &str) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(label)
                    .font(theme::font_body())
                    .color(theme::TEXT_PRIMARY),
            );
            if !description.is_empty() {
                ui.label(
                    egui::RichText::new(description)
                        .font(theme::font_small())
                        .color(theme::TEXT_SECONDARY),
                );
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(button_text).font(theme::font_small()),
                    )
                    .corner_radius(theme::BUTTON_ROUNDING)
                    .stroke(theme::card_stroke()),
                )
                .clicked()
            {
                clicked = true;
            }
        });
    });
    ui.add_space(4.0);
    clicked
}

fn toggle_widget(on: &mut bool) -> impl egui::Widget + '_ {
    move |ui: &mut egui::Ui| {
        let desired_size = egui::vec2(40.0, 22.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
        if response.clicked() {
            *on = !*on;
        }
        if ui.is_rect_visible(rect) {
            let bg_color = if *on { theme::TOGGLE_ON } else { theme::TOGGLE_OFF };
            let circle_x = if *on {
                rect.right() - 11.0 - 1.0
            } else {
                rect.left() + 11.0 + 1.0
            };
            ui.painter().rect_filled(rect, 11.0, bg_color);
            ui.painter().circle_filled(
                egui::pos2(circle_x, rect.center().y),
                9.0,
                egui::Color32::WHITE,
            );
        }
        response
    }
}
