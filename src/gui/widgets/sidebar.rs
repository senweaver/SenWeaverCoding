// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
use crate::gui::state::{AppState, Page};
use crate::gui::theme;

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    egui::SidePanel::left("sidebar")
        .exact_width(theme::SIDEBAR_WIDTH)
        .frame(
            egui::Frame::NONE
                .fill(theme::BG_SIDEBAR)
                .inner_margin(egui::Margin::same(12)),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);

            // Search
            let search_response = ui.add(
                egui::TextEdit::singleline(&mut state.sidebar_search)
                    .hint_text("\u{1F50D} Search")
                    .desired_width(f32::INFINITY)
                    .font(theme::font_small())
                    .margin(egui::Margin::symmetric(8, 4)),
            );
            let _ = search_response;

            ui.add_space(4.0);

            // New Agent button
            let new_btn = ui.add(
                egui::Button::new(
                    egui::RichText::new("\u{2207}  New Agent")
                        .font(theme::font_body())
                        .color(theme::TEXT_PRIMARY),
                )
                .fill(egui::Color32::TRANSPARENT)
                .min_size(egui::vec2(ui.available_width(), 28.0)),
            );
            if new_btn.clicked() {
                state.messages.clear();
                state.chat_input.clear();
                state.current_page = Page::Home;
            }

            // Marketplace button
            let mkt_btn = ui.add(
                egui::Button::new(
                    egui::RichText::new("\u{1F6D2}  Marketplace")
                        .font(theme::font_body())
                        .color(if state.current_page == Page::Marketplace {
                            theme::ACCENT_BLUE
                        } else {
                            theme::TEXT_PRIMARY
                        }),
                )
                .fill(if state.current_page == Page::Marketplace {
                    theme::BG_HOVER
                } else {
                    egui::Color32::TRANSPARENT
                })
                .min_size(egui::vec2(ui.available_width(), 28.0)),
            );
            if mkt_btn.clicked() {
                state.current_page = Page::Marketplace;
            }

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);

            // Workspace name
            ui.label(
                egui::RichText::new("senos-cli")
                    .font(theme::font_small())
                    .color(theme::TEXT_SECONDARY),
            );

            ui.add_space(4.0);

            // Conversation history
            egui::ScrollArea::vertical()
                .max_height(ui.available_height() - 40.0)
                .show(ui, |ui| {
                    for conv in &state.conversations {
                        let is_active = state
                            .active_conversation_id
                            .as_ref()
                            .map_or(false, |id| id == &conv.id);
                        let btn = ui.add(
                            egui::Button::new(
                                egui::RichText::new(&format!("\u{2022}  {}", conv.title))
                                    .font(theme::font_small())
                                    .color(if is_active {
                                        theme::TEXT_PRIMARY
                                    } else {
                                        theme::TEXT_SECONDARY
                                    }),
                            )
                            .fill(if is_active {
                                theme::BG_HOVER
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .min_size(egui::vec2(ui.available_width(), 24.0)),
                        );
                        if btn.clicked() {
                            state.active_conversation_id = Some(conv.id.clone());
                            state.current_page = Page::Home;
                        }
                    }
                });

            // Bottom: Open Workspace
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                let ws_btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new("\u{1F4C2}  Open Workspace")
                            .font(theme::font_small())
                            .color(theme::TEXT_SECONDARY),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .min_size(egui::vec2(ui.available_width(), 28.0)),
                );
                if ws_btn.clicked() {
                    state.current_page = Page::Home;
                }

                // Settings gear
                let gear_btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new("\u{2699}")
                            .font(theme::font_body())
                            .color(theme::TEXT_SECONDARY),
                    )
                    .fill(egui::Color32::TRANSPARENT),
                );
                if gear_btn.clicked() {
                    state.current_page = Page::Settings;
                }
            });
        });
}
