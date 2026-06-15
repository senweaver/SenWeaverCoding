// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckManifest {
    #[serde(default)]
    pub version: Option<u32>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub aspect: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub palette: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub fonts: Option<FontSpec>,
    #[serde(default)]
    pub footer: Option<String>,
    #[serde(default)]
    pub page_numbers: Option<bool>,
    #[serde(default)]
    pub transition: Option<String>,
    #[serde(default)]
    pub slides: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontSpec {
    #[serde(default)]
    pub heading: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlideSpec {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub layout: Option<String>,
    #[serde(default)]
    pub background: Option<BackgroundSpec>,
    #[serde(default)]
    pub blocks: Vec<BlockSpec>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundSpec {
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub gradient: Option<Vec<String>>,
    #[serde(default)]
    pub angle: Option<f64>,
    #[serde(default)]
    pub image: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct FrameSpec {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockSpec {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub slot: Option<String>,
    #[serde(default)]
    pub frame: Option<FrameSpec>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub align: Option<String>,
    #[serde(default)]
    pub valign: Option<String>,
    #[serde(default)]
    pub line_spacing: Option<f64>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub size: Option<f64>,
    #[serde(default)]
    pub bold: Option<bool>,
    #[serde(default)]
    pub italic: Option<bool>,
    #[serde(default)]
    pub font: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub runs: Option<Vec<RunSpec>>,
    #[serde(default)]
    pub items: Option<Vec<BulletItemSpec>>,
    #[serde(default)]
    pub marker: Option<String>,
    #[serde(default)]
    pub gap: Option<f64>,
    #[serde(default)]
    pub src: Option<String>,
    #[serde(default)]
    pub fit: Option<String>,
    #[serde(default)]
    pub radius: Option<f64>,
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub fill: Option<PaintSpec>,
    #[serde(default)]
    pub stroke: Option<StrokeSpec>,
    #[serde(default)]
    pub columns: Option<Vec<f64>>,
    #[serde(default)]
    pub rows: Option<Vec<Vec<String>>>,
    #[serde(default)]
    pub header_row: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSpec {
    pub text: String,
    #[serde(default)]
    pub bold: Option<bool>,
    #[serde(default)]
    pub italic: Option<bool>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub size: Option<f64>,
    #[serde(default)]
    pub font: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum BulletItemSpec {
    Plain(String),
    Detailed {
        text: String,
        #[serde(default)]
        level: Option<u8>,
        #[serde(default)]
        bold: Option<bool>,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        size: Option<f64>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaintSpec {
    pub color: String,
    #[serde(default)]
    pub alpha: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrokeSpec {
    pub color: String,
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub alpha: Option<f64>,
}

pub fn stage_for_aspect(aspect: Option<&str>) -> (u32, u32) {
    match aspect.map(str::trim) {
        Some("4:3") => (1440, 1080),
        _ => (1920, 1080),
    }
}
