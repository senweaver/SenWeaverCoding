// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Desktop GUI application for SenWeaverCoding — powered by egui/eframe.
//!
//! Enable with `--features gui` and run with `sen gui`.

mod app;
mod bridge;
pub mod state;
pub mod theme;
mod widgets;
mod pages;

/// Launch the desktop GUI (blocking — takes over the main thread).
pub fn run_gui() {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("SenWeaverCoding"),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "SenWeaverCoding",
        native_options,
        Box::new(|cc| Ok(Box::new(app::SenApp::new(cc)))),
    );
}
