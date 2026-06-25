// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Result};

use super::action::extract_json_object;
use super::coordinates::Box2d;
use super::vision::VisionClient;

const GROUNDING_SYSTEM: &str = "You are a precise GUI visual grounding model. \
Given a screenshot and the description of a single on-screen element, you return the \
bounding box of that element. Coordinates are normalized to the range 0-1000 relative to \
the image, in the format [ymin, xmin, ymax, xmax] where the origin is the top-left corner. \
Respond with raw JSON only, no markdown, no explanation.";

#[derive(Debug, Clone, Copy)]
pub struct GroundingResult {
    pub x_norm: f64,
    pub y_norm: f64,
    pub confidence: f64,
}

pub async fn locate(
    client: &VisionClient,
    image_data_uri: &str,
    element_description: &str,
) -> Result<GroundingResult> {
    let user = format!(
        "Locate this element: \"{element_description}\".\n\
         Return the grounding coordinates normalized to 0-1000 as \
         [ymin, xmin, ymax, xmax].\n\
         Return JSON only (no markdown):\n\
         {{\"box_2d\": [ymin, xmin, ymax, xmax], \"confidence\": <0-100>}}"
    );

    let raw = client
        .complete_with_image(GROUNDING_SYSTEM, &user, image_data_uri)
        .await?;
    parse_grounding(&raw)
}

fn parse_grounding(raw: &str) -> Result<GroundingResult> {
    let json_str = extract_json_object(raw)
        .ok_or_else(|| anyhow!("grounding response missing JSON object: {raw}"))?;
    let value: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| anyhow!("failed to parse grounding JSON: {e}; raw: {json_str}"))?;

    let coords = value
        .get("box_2d")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("grounding JSON missing 'box_2d' array"))?;
    let numbers: Vec<f64> = coords
        .iter()
        .filter_map(serde_json::Value::as_f64)
        .collect();
    let box2d = Box2d::from_slice(&numbers)
        .ok_or_else(|| anyhow!("grounding box_2d must contain exactly 4 numbers"))?;

    let confidence = value
        .get("confidence")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);

    let (x_norm, y_norm) = box2d.center_normalized();
    Ok(GroundingResult {
        x_norm,
        y_norm,
        confidence,
    })
}
