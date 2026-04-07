// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
use crate::gui::state::AppState;
use crate::gui::theme;

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_model_picker {
        return;
    }

    egui::Area::new(egui::Id::new("model_picker"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(350.0, 480.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .corner_radius(theme::POPUP_ROUNDING)
                .shadow(theme::popup_shadow())
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_min_width(250.0);

                    // Search
                    ui.add(
                        egui::TextEdit::singleline(&mut state.model_search)
                            .hint_text("Search models")
                            .desired_width(f32::INFINITY)
                            .font(theme::font_small())
                            .margin(egui::Margin::symmetric(8, 4)),
                    );

                    ui.add_space(6.0);

                    // MAX Mode toggle
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("MAX Mode")
                                .font(theme::font_body())
                                .color(theme::TEXT_PRIMARY),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let mut max = state.max_mode;
                            ui.add(toggle_switch(&mut max));
                            state.max_mode = max;
                        });
                    });

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // Group models by tier
                    let search = state.model_search.to_lowercase();
                    let mut last_tier = String::new();

                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            for model in &state.available_models {
                                if !search.is_empty()
                                    && !model.name.to_lowercase().contains(&search)
                                {
                                    continue;
                                }

                                if model.tier != last_tier {
                                    let tier_label = match model.tier.as_str() {
                                        "Auto" => "Auto  Efficiency",
                                        "Premium" => "Premium  Intelligence",
                                        other => other,
                                    };
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new(tier_label)
                                            .font(theme::font_small())
                                            .color(theme::TEXT_SECONDARY),
                                    );
                                    ui.add_space(2.0);
                                    last_tier = model.tier.clone();
                                }

                                let is_selected = state.selected_model == model.name;
                                let btn = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(&format!(
                                            "{}{}",
                                            model.name,
                                            if is_selected { "  \u{2713}" } else { "" }
                                        ))
                                        .font(theme::font_body())
                                        .color(if is_selected {
                                            theme::ACCENT_BLUE
                                        } else {
                                            theme::TEXT_PRIMARY
                                        }),
                                    )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .min_size(egui::vec2(ui.available_width(), 28.0)),
                                );
                                if btn.clicked() {
                                    state.selected_model = model.name.clone();
                                    state.show_model_picker = false;
                                }
                            }
                        });
                });
        });

    // Close on outside click
    if ctx.input(|i| i.pointer.any_click()) {
        let popup_rect = egui::Rect::from_min_size(egui::pos2(350.0, 480.0), egui::vec2(270.0, 400.0));
        if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
            if !popup_rect.contains(pos) {
                state.show_model_picker = false;
            }
        }
    }
}

fn toggle_switch(on: &mut bool) -> impl egui::Widget + '_ {
    move |ui: &mut egui::Ui| {
        let desired_size = egui::vec2(36.0, 20.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
        if response.clicked() {
            *on = !*on;
        }

        if ui.is_rect_visible(rect) {
            let bg_color = if *on { theme::TOGGLE_ON } else { theme::TOGGLE_OFF };
            let circle_x = if *on {
                rect.right() - 10.0 - 1.0
            } else {
                rect.left() + 10.0 + 1.0
            };

            ui.painter().rect_filled(rect, 10.0, bg_color);
            ui.painter().circle_filled(
                egui::pos2(circle_x, rect.center().y),
                8.0,
                egui::Color32::WHITE,
            );
        }

        response
    }
}
