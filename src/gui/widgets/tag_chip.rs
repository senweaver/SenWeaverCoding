// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
use crate::gui::theme;

/// A removable tag chip (like the "Plan X" tag in Cursor).
pub fn show(ui: &mut egui::Ui, label: &str, color: egui::Color32) -> bool {
    let mut removed = false;

    let frame = egui::Frame::NONE
        .fill(color.linear_multiply(0.15))
        .corner_radius(theme::TAG_ROUNDING)
        .inner_margin(egui::Margin::symmetric(8, 3));

    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(
                egui::RichText::new(label)
                    .font(theme::font_small())
                    .color(color),
            );
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("\u{2715}")
                            .font(egui::FontId::proportional(10.0))
                            .color(color),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .frame(false),
                )
                .clicked()
            {
                removed = true;
            }
        });
    });

    removed
}
