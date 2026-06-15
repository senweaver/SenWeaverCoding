// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderDeck {
    pub version: u32,
    pub title: String,
    pub theme: String,
    pub stage_w: u32,
    pub stage_h: u32,
    pub transition: String,
    pub accent: String,
    pub fonts: RenderFonts,
    pub slides: Vec<RenderSlide>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderFonts {
    pub heading_latin: String,
    pub heading_ea: String,
    pub body_latin: String,
    pub body_ea: String,
    pub heading_css: String,
    pub body_css: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderSlide {
    pub id: String,
    pub layout: String,
    pub background: RenderBackground,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub blocks: Vec<RenderBlock>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RenderBackground {
    Color { color: String },
    Gradient { from: String, to: String, angle: f64 },
    Image { src: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RenderBlock {
    Text(RenderTextBlock),
    Image(RenderImageBlock),
    Shape(RenderShapeBlock),
    Table(RenderTableBlock),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderTextBlock {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub align: String,
    pub valign: String,
    pub line_spacing: f64,
    pub paragraphs: Vec<RenderParagraph>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderParagraph {
    pub bullet: bool,
    pub level: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bullet_char: Option<String>,
    pub space_before: f64,
    pub runs: Vec<RenderRun>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub color: String,
    pub size: f64,
    pub font: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderImageBlock {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub src: String,
    pub fit: String,
    pub radius: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderShapeBlock {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub shape: String,
    pub radius: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<RenderPaint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<RenderStroke>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderPaint {
    pub color: String,
    pub alpha: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderStroke {
    pub color: String,
    pub width: f64,
    pub alpha: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderTableBlock {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub col_fracs: Vec<f64>,
    pub header_row: bool,
    pub rows: Vec<Vec<String>>,
    pub size: f64,
    pub text_color: String,
    pub header_fill: String,
    pub header_text: String,
    pub row_fill: String,
    pub hairline: String,
    pub font_css: String,
    pub font_latin: String,
    pub font_ea: String,
}
