// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CuratorTemplateKind {
    #[default]
    PaperImrad,
    PaperApa,
    PaperMla,
    PaperChicago,
    PaperGb7714,
    SolutionFunctional,
    SolutionGb8567_2006,
    SolutionGb8567_1988,
    SolutionIeee830,
    SolutionIso29148,
    SolutionIso42010,
    SolutionIeee1016,
    SolutionIso12207,
    TechReport,
}

impl CuratorTemplateKind {
    pub fn from_str_loose(raw: &str) -> Self {
        let key = raw.trim().to_ascii_lowercase().replace('-', "_");
        match key.as_str() {
            "paper" | "paper_imrad" | "imrad" => Self::PaperImrad,
            "paper_apa" | "apa" | "apa7" | "apa_7" => Self::PaperApa,
            "paper_mla" | "mla" | "mla9" | "mla_9" => Self::PaperMla,
            "paper_chicago" | "chicago" | "cms" => Self::PaperChicago,
            "paper_gb7714" | "gb7714" | "gbt7714" | "gb_t_7714" | "guobiao" => {
                Self::PaperGb7714
            }
            "solution" | "solution_functional" | "functional" => Self::SolutionFunctional,
            "solution_gb8567_2006"
            | "gb8567_2006"
            | "gbt8567"
            | "gb_t_8567"
            | "gb_t_8567_2006" => Self::SolutionGb8567_2006,
            "solution_gb8567_1988" | "gb8567_1988" | "gb_t_8567_1988" => {
                Self::SolutionGb8567_1988
            }
            "solution_ieee830" | "ieee830" | "ieee_830" | "srs" => Self::SolutionIeee830,
            "solution_iso29148" | "iso29148" | "iso_29148" | "ieee_29148" => {
                Self::SolutionIso29148
            }
            "solution_iso42010" | "iso42010" | "iso_42010" | "ieee_42010" | "sad" => {
                Self::SolutionIso42010
            }
            "solution_ieee1016" | "ieee1016" | "ieee_1016" | "sdd" => Self::SolutionIeee1016,
            "solution_iso12207" | "iso12207" | "iso_12207" | "ieee_12207" | "lcm" => {
                Self::SolutionIso12207
            }
            "tech_report" | "techreport" | "report" | "tr" => Self::TechReport,
            _ => Self::PaperImrad,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::PaperImrad => "paper_imrad",
            Self::PaperApa => "paper_apa",
            Self::PaperMla => "paper_mla",
            Self::PaperChicago => "paper_chicago",
            Self::PaperGb7714 => "paper_gb7714",
            Self::SolutionFunctional => "solution_functional",
            Self::SolutionGb8567_2006 => "solution_gb8567_2006",
            Self::SolutionGb8567_1988 => "solution_gb8567_1988",
            Self::SolutionIeee830 => "solution_ieee830",
            Self::SolutionIso29148 => "solution_iso29148",
            Self::SolutionIso42010 => "solution_iso42010",
            Self::SolutionIeee1016 => "solution_ieee1016",
            Self::SolutionIso12207 => "solution_iso12207",
            Self::TechReport => "tech_report",
        }
    }

    pub fn is_paper(&self) -> bool {
        matches!(
            self,
            Self::PaperImrad
                | Self::PaperApa
                | Self::PaperMla
                | Self::PaperChicago
                | Self::PaperGb7714
        )
    }

    pub fn is_solution(&self) -> bool {
        matches!(
            self,
            Self::SolutionFunctional
                | Self::SolutionGb8567_2006
                | Self::SolutionGb8567_1988
                | Self::SolutionIeee830
                | Self::SolutionIso29148
                | Self::SolutionIso42010
                | Self::SolutionIeee1016
                | Self::SolutionIso12207
        )
    }

    pub fn enum_values() -> &'static [&'static str] {
        &[
            "paper_imrad",
            "paper_apa",
            "paper_mla",
            "paper_chicago",
            "paper_gb7714",
            "solution_functional",
            "solution_gb8567_2006",
            "solution_gb8567_1988",
            "solution_ieee830",
            "solution_iso29148",
            "solution_iso42010",
            "solution_ieee1016",
            "solution_iso12207",
            "tech_report",
            "paper",
            "solution",
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuratorActive {
    pub slug: String,
    pub intent: String,
    pub template: CuratorTemplateKind,
    pub root_dir: PathBuf,
    pub started_at: String,
}

fn curator_session_key() -> String {
    crate::session::current_session_context()
        .map(|c| c.session_id)
        .unwrap_or_else(|| "default".to_string())
}

#[derive(Clone, Default)]
pub struct CuratorState {
    inner: Arc<RwLock<std::collections::HashMap<String, CuratorActive>>>,
}

impl CuratorState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self) -> Option<CuratorActive> {
        self.inner.read().get(&curator_session_key()).cloned()
    }

    pub fn set(&self, active: CuratorActive) {
        self.inner.write().insert(curator_session_key(), active);
    }

    pub fn set_template(&self, template: CuratorTemplateKind) {
        if let Some(active) = self.inner.write().get_mut(&curator_session_key()) {
            active.template = template;
        }
    }

    pub fn clear(&self) {
        self.inner.write().remove(&curator_session_key());
    }
}

pub fn new_curator_state() -> CuratorState {
    CuratorState::new()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingCuratorPayload {
    pub slug: String,
    pub template: CuratorTemplateKind,
    pub final_md_path: String,
    pub impl_blueprint_path: String,
    pub docx_path: Option<String>,
    pub root_dir: String,
    pub final_md_body: String,
    pub impl_blueprint_body: String,
}

#[derive(Clone, Default)]
pub struct PendingCurator {
    inner: Arc<RwLock<std::collections::HashMap<String, PendingCuratorPayload>>>,
}

impl PendingCurator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, payload: PendingCuratorPayload) {
        self.inner.write().insert(curator_session_key(), payload);
    }

    pub fn get(&self) -> Option<PendingCuratorPayload> {
        self.inner.read().get(&curator_session_key()).cloned()
    }

    pub fn take(&self) -> Option<PendingCuratorPayload> {
        self.inner.write().remove(&curator_session_key())
    }
}

pub fn new_pending_curator() -> PendingCurator {
    PendingCurator::new()
}
