// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
use crate::gui::state::{AppState, MarketCategory, Page};
use crate::gui::theme;
use crate::gui::widgets::{plugin_card, sidebar};

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    // Main sidebar
    sidebar::show(ctx, state);

    // Category side panel
    egui::SidePanel::left("market_categories")
        .exact_width(theme::SETTINGS_NAV_WIDTH)
        .frame(
            egui::Frame::NONE
                .fill(theme::BG_SIDEBAR)
                .inner_margin(egui::Margin::same(12)),
        )
        .show(ctx, |ui| {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Categories")
                    .font(theme::font_small())
                    .color(theme::TEXT_SECONDARY)
                    .strong(),
            );
            ui.add_space(8.0);

            for cat in MarketCategory::all() {
                let is_active = state.market_category == *cat;
                let btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new(cat.label())
                            .font(theme::font_body())
                            .color(if is_active {
                                theme::ACCENT_BLUE
                            } else {
                                theme::TEXT_PRIMARY
                            }),
                    )
                    .fill(if is_active {
                        theme::BG_HOVER
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .min_size(egui::vec2(ui.available_width(), 28.0)),
                );
                if btn.clicked() {
                    state.market_category = *cat;
                }
            }

            // Back to Home
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("\u{2190} Back to Home")
                                .font(theme::font_small())
                                .color(theme::TEXT_SECONDARY),
                        )
                        .fill(egui::Color32::TRANSPARENT),
                    )
                    .clicked()
                {
                    state.current_page = Page::Home;
                }
            });
        });

    // Main content — plugin grid
    egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .fill(theme::BG_PRIMARY)
                .inner_margin(egui::Margin::same(20)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Marketplace")
                        .font(theme::font_heading())
                        .color(theme::TEXT_PRIMARY),
                );
            });

            ui.add_space(8.0);

            // Search bar
            ui.add(
                egui::TextEdit::singleline(&mut state.market_search)
                    .hint_text("\u{1F50D} Search skills, rules, subagents, MCPs and hooks")
                    .desired_width(f32::INFINITY)
                    .font(theme::font_body())
                    .margin(egui::Margin::symmetric(12, 8)),
            );

            ui.add_space(16.0);

            // Cards in a two-column grid
            let search = state.market_search.to_lowercase();
            let selected_cat = state.market_category;
            let filtered: Vec<_> = state
                .plugins
                .iter()
                .filter(|p| {
                    (selected_cat == MarketCategory::Featured
                        || selected_cat == MarketCategory::AllPlugins
                        || p.category.to_lowercase() == selected_cat.label().to_lowercase())
                        && (search.is_empty() || p.name.to_lowercase().contains(&search))
                })
                .cloned()
                .collect();

            egui::ScrollArea::vertical().show(ui, |ui| {
                let col_width = (ui.available_width() - 12.0) / 2.0;
                let mut col = 0;

                ui.horizontal_wrapped(|ui| {
                    for plugin in &filtered {
                        if col >= 2 {
                            col = 0;
                        }
                        ui.allocate_ui(egui::vec2(col_width, 80.0), |ui| {
                            plugin_card::show(ui, plugin);
                        });
                        col += 1;
                    }
                });
            });
        });
}
