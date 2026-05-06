// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Output style constants — mirrors claude-code-typescript-src`constants/outputStyles.ts`.

use serde::{Deserialize, Serialize};

pub const STYLE_DEFAULT: &str = "default";
pub const STYLE_CONCISE: &str = "concise";
pub const STYLE_DETAILED: &str = "detailed";
pub const STYLE_FORMAL: &str = "formal";
pub const STYLE_CODE_ONLY: &str = "code-only";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputStyleDef {
    pub name: String,
    pub description: String,
    pub system_prompt_addition: String,
}

pub fn builtin_output_styles() -> Vec<OutputStyleDef> {
    vec![
        OutputStyleDef {
            name: STYLE_DEFAULT.to_string(),
            description: "Standard balanced output".to_string(),
            system_prompt_addition: String::new(),
        },
        OutputStyleDef {
            name: STYLE_CONCISE.to_string(),
            description: "Minimal, terse responses".to_string(),
            system_prompt_addition: "Be extremely concise. Use short sentences and bullet points. \
                 Omit pleasantries and explanations unless asked."
                .to_string(),
        },
        OutputStyleDef {
            name: STYLE_DETAILED.to_string(),
            description: "Thorough, explanatory responses".to_string(),
            system_prompt_addition:
                "Be thorough and detailed. Explain your reasoning, show alternatives, \
                 and provide context for every decision."
                    .to_string(),
        },
        OutputStyleDef {
            name: STYLE_FORMAL.to_string(),
            description: "Professional, formal tone".to_string(),
            system_prompt_addition:
                "Use a formal, professional tone. Avoid colloquialisms and casual language."
                    .to_string(),
        },
        OutputStyleDef {
            name: STYLE_CODE_ONLY.to_string(),
            description: "Only output code, minimal prose".to_string(),
            system_prompt_addition: "Output only code and minimal necessary prose. \
                 Do not explain unless explicitly asked. \
                 Prefer code blocks over descriptions."
                .to_string(),
        },
    ]
}
