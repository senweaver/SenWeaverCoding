// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod index;
pub mod store;

pub use index::{BaselineIndex, BaselineRecord};
pub use store::TemplateLibraryStore;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TemplateKind {
    DesignSystem,
    DesignerTemplate,
    PromptTemplate,
    CuratorTemplate,
}

impl TemplateKind {
    pub fn dir_prefix(self) -> &'static str {
        match self {
            TemplateKind::DesignSystem => "design-systems",
            TemplateKind::DesignerTemplate => "designer-templates",
            TemplateKind::PromptTemplate => "prompt-templates",
            TemplateKind::CuratorTemplate => "curator-templates",
        }
    }

    pub fn from_id(raw: &str) -> Option<TemplateKind> {
        match raw.trim() {
            "design-system" | "design-systems" | "designSystem" => Some(TemplateKind::DesignSystem),
            "designer-template" | "designer-templates" | "htmlTemplate" => {
                Some(TemplateKind::DesignerTemplate)
            }
            "prompt-template" | "prompt-templates" | "promptTemplate" => {
                Some(TemplateKind::PromptTemplate)
            }
            "curator-template" | "curator-templates" | "curator" => {
                Some(TemplateKind::CuratorTemplate)
            }
            _ => None,
        }
    }

    pub fn as_id(self) -> &'static str {
        match self {
            TemplateKind::DesignSystem => "design-system",
            TemplateKind::DesignerTemplate => "designer-template",
            TemplateKind::PromptTemplate => "prompt-template",
            TemplateKind::CuratorTemplate => "curator-template",
        }
    }
}

pub fn content_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest.iter() {
        out.push_str(&format!("{b:02x}"));
    }
    out
}
