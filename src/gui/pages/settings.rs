// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
use crate::gui::state::{AppState, Page, SettingsCategory};
use crate::gui::theme;
use crate::gui::widgets::{settings_row, sidebar};

pub fn show(ctx: &egui::Context, state: &mut AppState) {
    // Main sidebar
    sidebar::show(ctx, state);

    // Settings categories panel
    egui::SidePanel::left("settings_categories")
        .exact_width(theme::SETTINGS_NAV_WIDTH)
        .frame(
            egui::Frame::NONE
                .fill(theme::BG_SIDEBAR)
                .inner_margin(egui::Margin::same(12)),
        )
        .show(ctx, |ui| {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Settings")
                    .font(theme::font_heading())
                    .color(theme::TEXT_PRIMARY),
            );
            ui.add_space(12.0);

            for cat in SettingsCategory::all() {
                let is_active = state.settings_category == *cat;
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
                    state.settings_category = *cat;
                }
            }

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

    // Settings content
    egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .fill(theme::BG_PRIMARY)
                .inner_margin(egui::Margin::same(24)),
        )
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(state.settings_category.label())
                    .font(theme::font_heading())
                    .color(theme::TEXT_PRIMARY),
            );
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(12.0);

            egui::ScrollArea::vertical().show(ui, |ui| {
                match state.settings_category {
                    SettingsCategory::General => show_general(ui, state),
                    SettingsCategory::Appearance => show_appearance(ui),
                    SettingsCategory::Models => show_models(ui, state),
                    SettingsCategory::Plugins => show_plugins(ui),
                    SettingsCategory::RulesSkills => show_rules_skills(ui),
                    SettingsCategory::ToolsMcps => show_tools_mcps(ui),
                    SettingsCategory::Hooks => show_hooks(ui),
                    SettingsCategory::Network => show_network(ui),
                }
            });
        });
}

fn show_general(ui: &mut egui::Ui, state: &mut AppState) {
    settings_row::section_header(ui, "NOTIFICATIONS");
    settings_row::toggle_row(
        ui,
        "System Notifications",
        "Send native system notifications",
        &mut state.setting_notifications,
    );
    settings_row::toggle_row(
        ui,
        "Warning Notifications",
        "Show warning-level notifications",
        &mut state.setting_warning_notifications,
    );
    settings_row::toggle_row(
        ui,
        "Completion Sound",
        "Play a sound when agent completes",
        &mut state.setting_completion_sound,
    );

    settings_row::section_header(ui, "SYSTEM");
    settings_row::toggle_row(
        ui,
        "System Tray",
        "Minimize to system tray instead of closing",
        &mut state.setting_system_tray,
    );
    settings_row::button_row(
        ui,
        "Config Directory",
        "Open the configuration directory",
        "Open",
    );
    settings_row::button_row(ui, "Account", "Sign out of your account", "Log Out");
}

fn show_appearance(ui: &mut egui::Ui) {
    settings_row::section_header(ui, "THEME");
    ui.label(
        egui::RichText::new("Light theme is the default. Dark theme coming soon.")
            .font(theme::font_body())
            .color(theme::TEXT_SECONDARY),
    );
}

fn show_models(ui: &mut egui::Ui, state: &mut AppState) {
    settings_row::section_header(ui, "MODEL CONFIGURATION");
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Default Model")
                .font(theme::font_body())
                .color(theme::TEXT_PRIMARY),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(&state.selected_model)
                    .font(theme::font_body())
                    .color(theme::ACCENT_BLUE),
            );
        });
    });
    settings_row::toggle_row(
        ui,
        "MAX Mode",
        "Use maximum context window (higher cost)",
        &mut state.max_mode,
    );
}

fn show_plugins(ui: &mut egui::Ui) {
    settings_row::section_header(ui, "INSTALLED PLUGINS");
    ui.label(
        egui::RichText::new("No plugins installed. Visit the Marketplace to discover plugins.")
            .font(theme::font_body())
            .color(theme::TEXT_SECONDARY),
    );
}

fn show_rules_skills(ui: &mut egui::Ui) {
    settings_row::section_header(ui, "RULES & SKILLS");
    ui.label(
        egui::RichText::new("Rules and skills are loaded from .senweavercoding/ directory.")
            .font(theme::font_body())
            .color(theme::TEXT_SECONDARY),
    );
    settings_row::button_row(ui, "Rules Directory", "", "Open");
    settings_row::button_row(ui, "Skills Directory", "", "Open");
}

fn show_tools_mcps(ui: &mut egui::Ui) {
    settings_row::section_header(ui, "TOOLS");
    ui.label(
        egui::RichText::new("Built-in tools: Shell, FileRead, FileWrite, FileEdit, MultiEdit, Grep, Glob, LSP, Browser")
            .font(theme::font_body())
            .color(theme::TEXT_SECONDARY),
    );
    settings_row::section_header(ui, "MCP SERVERS");
    ui.label(
        egui::RichText::new("Configure MCP servers in your .senweavercoding/config.json file.")
            .font(theme::font_body())
            .color(theme::TEXT_SECONDARY),
    );
}

fn show_hooks(ui: &mut egui::Ui) {
    settings_row::section_header(ui, "HOOKS");
    ui.label(
        egui::RichText::new("Hooks allow custom logic before/after agent actions. Configure in .senweavercoding/hooks/.")
            .font(theme::font_body())
            .color(theme::TEXT_SECONDARY),
    );
}

fn show_network(ui: &mut egui::Ui) {
    settings_row::section_header(ui, "PROXY SETTINGS");
    ui.label(
        egui::RichText::new("HTTP/HTTPS proxy and API endpoint configuration.")
            .font(theme::font_body())
            .color(theme::TEXT_SECONDARY),
    );
}
