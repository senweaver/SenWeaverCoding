// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MultimodalConfig {

    #[serde(default = "default_multimodal_max_images")]
    pub max_images: usize,

    #[serde(default = "default_multimodal_max_image_size_mb")]
    pub max_image_size_mb: usize,

    #[serde(default)]
    pub allow_remote_fetch: bool,

    #[serde(default)]
    pub vision_provider: Option<String>,

    #[serde(default)]
    pub vision_model: Option<String>,
}

pub(crate) fn default_multimodal_max_images() -> usize {
    4
}

pub(crate) fn default_multimodal_max_image_size_mb() -> usize {
    5
}

impl MultimodalConfig {

    pub fn effective_limits(&self) -> (usize, usize) {
        let max_images = self.max_images.clamp(1, 16);
        let max_image_size_mb = self.max_image_size_mb.clamp(1, 20);
        (max_images, max_image_size_mb)
    }

    pub fn has_vision_route(&self) -> bool {
        self.vision_provider
            .as_ref()
            .map_or(false, |p| !p.is_empty())
            && self.vision_model.as_ref().map_or(false, |m| !m.is_empty())
    }

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.max_images == 0 {
            errors.push("multimodal.max_images must be >= 1".into());
        }
        if self.max_images > 16 {
            errors.push("multimodal.max_images > 16 is likely a misconfiguration".into());
        }
        if self.max_image_size_mb == 0 {
            errors.push("multimodal.max_image_size_mb must be >= 1".into());
        }
        if self.max_image_size_mb > 20 {
            errors.push("multimodal.max_image_size_mb > 20 exceeds safe upload bounds".into());
        }

        match (
            self.vision_provider.as_deref(),
            self.vision_model.as_deref(),
        ) {
            (Some(p), None) if !p.is_empty() => {
                errors.push("multimodal.vision_provider set but vision_model missing".into())
            }
            (None, Some(m)) if !m.is_empty() => {
                errors.push("multimodal.vision_model set but vision_provider missing".into())
            }
            _ => {}
        }
        errors
    }
}

impl Default for MultimodalConfig {
    fn default() -> Self {
        Self {
            max_images: default_multimodal_max_images(),
            max_image_size_mb: default_multimodal_max_image_size_mb(),
            allow_remote_fetch: false,
            vision_provider: None,
            vision_model: None,
        }
    }
}
