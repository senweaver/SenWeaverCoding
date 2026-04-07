// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
use crate::gui::state::{ChatMessage, MessageRole};
use crate::gui::theme;

pub fn show(ui: &mut egui::Ui, msg: &ChatMessage) {
    let (bg, text_color, prefix) = match msg.role {
        MessageRole::User => (theme::USER_BUBBLE, theme::TEXT_PRIMARY, "You"),
        MessageRole::Assistant => (theme::ASSISTANT_BUBBLE, theme::TEXT_PRIMARY, "Agent"),
        MessageRole::ToolUse => (theme::BG_HOVER, theme::TEXT_SECONDARY, "\u{1F527} Tool"),
        MessageRole::ToolResult => (theme::BG_HOVER, theme::TEXT_SECONDARY, "\u{2705} Result"),
        MessageRole::System => (theme::BG_HOVER, theme::TEXT_SECONDARY, "\u{2139} System"),
    };

    let frame = egui::Frame::NONE
        .fill(bg)
        .corner_radius(theme::CARD_ROUNDING)
        .inner_margin(egui::Margin::same(12));

    frame.show(ui, |ui| {
        ui.set_min_width(ui.available_width());

        // Role header
        ui.label(
            egui::RichText::new(prefix)
                .font(theme::font_small())
                .color(theme::TEXT_SECONDARY)
                .strong(),
        );

        ui.add_space(4.0);

        // Content — handle code blocks with monospace font
        let content = &msg.content;
        if content.contains("```") {
            let mut in_code = false;
            for line in content.lines() {
                if line.starts_with("```") {
                    in_code = !in_code;
                    if in_code {
                        let lang = line.trim_start_matches('`');
                        if !lang.is_empty() {
                            ui.label(
                                egui::RichText::new(lang)
                                    .font(theme::font_small())
                                    .color(theme::TEXT_SECONDARY),
                            );
                        }
                    }
                } else if in_code {
                    ui.label(
                        egui::RichText::new(line)
                            .font(theme::font_mono())
                            .color(text_color),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(line)
                            .font(theme::font_body())
                            .color(text_color),
                    );
                }
            }
        } else {
            ui.label(
                egui::RichText::new(content)
                    .font(theme::font_body())
                    .color(text_color),
            );
        }
    });

    ui.add_space(8.0);
}
