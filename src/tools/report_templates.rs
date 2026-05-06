// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Report template engine for project delivery intelligence.
//!
//! Provides built-in templates for weekly status, sprint review, risk register,
//! and milestone reports with multi-language support (EN, DE, FR, IT).

use std::collections::HashMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Markdown,
    Html,
}

#[derive(Debug, Clone)]
pub struct TemplateSection {
    pub heading: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct ReportTemplate {
    pub name: String,
    pub sections: Vec<TemplateSection>,
    pub format: ReportFormat,
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

impl ReportTemplate {

    pub fn render(&self, vars: &HashMap<String, String>) -> String {
        let mut out = String::new();
        for section in &self.sections {
            let heading = substitute(&section.heading, vars);
            let body = substitute(&section.body, vars);
            match self.format {
                ReportFormat::Markdown => {
                    let _ = write!(out, "## {heading}\n\n{body}\n\n");
                }
                ReportFormat::Html => {
                    let heading = escape_html(&heading);
                    let body = escape_html(&body);
                    let _ = write!(out, "<h2>{heading}</h2>\n<p>{body}</p>\n");
                }
            }
        }
        out.trim_end().to_string()
    }
}

fn substitute(template: &str, vars: &HashMap<String, String>) -> String {
    let mut result = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if i + 1 < len && bytes[i] == b'{' && bytes[i + 1] == b'{' {

            if let Some(close) = template[i + 2..].find("}}") {
                let key = &template[i + 2..i + 2 + close];
                if let Some(value) = vars.get(key) {
                    result.push_str(value);
                } else {

                    result.push_str(&template[i..i + 2 + close + 2]);
                }
                i += 2 + close + 2;
                continue;
            }
        }
        result.push(template.as_bytes()[i] as char);
        i += 1;
    }

    result
}

pub fn weekly_status_template(lang: &str) -> ReportTemplate {
    let (name, sections) = match lang {
        "de" => (
            "Wochenstatus",
            vec![
                TemplateSection {
                    heading: "Zusammenfassung".into(),
                    body: "Projekt: {{project_name}} | Zeitraum: {{period}}".into(),
                },
                TemplateSection {
                    heading: "Erledigt".into(),
                    body: "{{completed}}".into(),
                },
                TemplateSection {
                    heading: "In Bearbeitung".into(),
                    body: "{{in_progress}}".into(),
                },
                TemplateSection {
                    heading: "Blockiert".into(),
                    body: "{{blocked}}".into(),
                },
                TemplateSection {
                    heading: "Naechste Schritte".into(),
                    body: "{{next_steps}}".into(),
                },
            ],
        ),
        "fr" => (
            "Statut hebdomadaire",
            vec![
                TemplateSection {
                    heading: "Resume".into(),
                    body: "Projet: {{project_name}} | Periode: {{period}}".into(),
                },
                TemplateSection {
                    heading: "Termine".into(),
                    body: "{{completed}}".into(),
                },
                TemplateSection {
                    heading: "En cours".into(),
                    body: "{{in_progress}}".into(),
                },
                TemplateSection {
                    heading: "Bloque".into(),
                    body: "{{blocked}}".into(),
                },
                TemplateSection {
                    heading: "Prochaines etapes".into(),
                    body: "{{next_steps}}".into(),
                },
            ],
        ),
        "it" => (
            "Stato settimanale",
            vec![
                TemplateSection {
                    heading: "Riepilogo".into(),
                    body: "Progetto: {{project_name}} | Periodo: {{period}}".into(),
                },
                TemplateSection {
                    heading: "Completato".into(),
                    body: "{{completed}}".into(),
                },
                TemplateSection {
                    heading: "In corso".into(),
                    body: "{{in_progress}}".into(),
                },
                TemplateSection {
                    heading: "Bloccato".into(),
                    body: "{{blocked}}".into(),
                },
                TemplateSection {
                    heading: "Prossimi passi".into(),
                    body: "{{next_steps}}".into(),
                },
            ],
        ),
        _ => (
            "Weekly Status",
            vec![
                TemplateSection {
                    heading: "Summary".into(),
                    body: "Project: {{project_name}} | Period: {{period}}".into(),
                },
                TemplateSection {
                    heading: "Completed".into(),
                    body: "{{completed}}".into(),
                },
                TemplateSection {
                    heading: "In Progress".into(),
                    body: "{{in_progress}}".into(),
                },
                TemplateSection {
                    heading: "Blocked".into(),
                    body: "{{blocked}}".into(),
                },
                TemplateSection {
                    heading: "Next Steps".into(),
                    body: "{{next_steps}}".into(),
                },
            ],
        ),
    };
    ReportTemplate {
        name: name.into(),
        sections,
        format: ReportFormat::Markdown,
    }
}

pub fn sprint_review_template(lang: &str) -> ReportTemplate {
    let (name, sections) = match lang {
        "de" => (
            "Sprint-Uebersicht",
            vec![
                TemplateSection {
                    heading: "Sprint".into(),
                    body: "{{sprint_dates}}".into(),
                },
                TemplateSection {
                    heading: "Erledigt".into(),
                    body: "{{completed}}".into(),
                },
                TemplateSection {
                    heading: "In Bearbeitung".into(),
                    body: "{{in_progress}}".into(),
                },
                TemplateSection {
                    heading: "Blockiert".into(),
                    body: "{{blocked}}".into(),
                },
                TemplateSection {
                    heading: "Velocity".into(),
                    body: "{{velocity}}".into(),
                },
            ],
        ),
        "fr" => (
            "Revue de sprint",
            vec![
                TemplateSection {
                    heading: "Sprint".into(),
                    body: "{{sprint_dates}}".into(),
                },
                TemplateSection {
                    heading: "Termine".into(),
                    body: "{{completed}}".into(),
                },
                TemplateSection {
                    heading: "En cours".into(),
                    body: "{{in_progress}}".into(),
                },
                TemplateSection {
                    heading: "Bloque".into(),
                    body: "{{blocked}}".into(),
                },
                TemplateSection {
                    heading: "Velocite".into(),
                    body: "{{velocity}}".into(),
                },
            ],
        ),
        "it" => (
            "Revisione sprint",
            vec![
                TemplateSection {
                    heading: "Sprint".into(),
                    body: "{{sprint_dates}}".into(),
                },
                TemplateSection {
                    heading: "Completato".into(),
                    body: "{{completed}}".into(),
                },
                TemplateSection {
                    heading: "In corso".into(),
                    body: "{{in_progress}}".into(),
                },
                TemplateSection {
                    heading: "Bloccato".into(),
                    body: "{{blocked}}".into(),
                },
                TemplateSection {
                    heading: "Velocita".into(),
                    body: "{{velocity}}".into(),
                },
            ],
        ),
        _ => (
            "Sprint Review",
            vec![
                TemplateSection {
                    heading: "Sprint".into(),
                    body: "{{sprint_dates}}".into(),
                },
                TemplateSection {
                    heading: "Completed".into(),
                    body: "{{completed}}".into(),
                },
                TemplateSection {
                    heading: "In Progress".into(),
                    body: "{{in_progress}}".into(),
                },
                TemplateSection {
                    heading: "Blocked".into(),
                    body: "{{blocked}}".into(),
                },
                TemplateSection {
                    heading: "Velocity".into(),
                    body: "{{velocity}}".into(),
                },
            ],
        ),
    };
    ReportTemplate {
        name: name.into(),
        sections,
        format: ReportFormat::Markdown,
    }
}

pub fn risk_register_template(lang: &str) -> ReportTemplate {
    let (name, sections) = match lang {
        "de" => (
            "Risikoregister",
            vec![
                TemplateSection {
                    heading: "Projekt".into(),
                    body: "{{project_name}}".into(),
                },
                TemplateSection {
                    heading: "Risiken".into(),
                    body: "{{risks}}".into(),
                },
                TemplateSection {
                    heading: "Massnahmen".into(),
                    body: "{{mitigations}}".into(),
                },
            ],
        ),
        "fr" => (
            "Registre des risques",
            vec![
                TemplateSection {
                    heading: "Projet".into(),
                    body: "{{project_name}}".into(),
                },
                TemplateSection {
                    heading: "Risques".into(),
                    body: "{{risks}}".into(),
                },
                TemplateSection {
                    heading: "Mesures".into(),
                    body: "{{mitigations}}".into(),
                },
            ],
        ),
        "it" => (
            "Registro dei rischi",
            vec![
                TemplateSection {
                    heading: "Progetto".into(),
                    body: "{{project_name}}".into(),
                },
                TemplateSection {
                    heading: "Rischi".into(),
                    body: "{{risks}}".into(),
                },
                TemplateSection {
                    heading: "Mitigazioni".into(),
                    body: "{{mitigations}}".into(),
                },
            ],
        ),
        _ => (
            "Risk Register",
            vec![
                TemplateSection {
                    heading: "Project".into(),
                    body: "{{project_name}}".into(),
                },
                TemplateSection {
                    heading: "Risks".into(),
                    body: "{{risks}}".into(),
                },
                TemplateSection {
                    heading: "Mitigations".into(),
                    body: "{{mitigations}}".into(),
                },
            ],
        ),
    };
    ReportTemplate {
        name: name.into(),
        sections,
        format: ReportFormat::Markdown,
    }
}

pub fn milestone_report_template(lang: &str) -> ReportTemplate {
    let (name, sections) = match lang {
        "de" => (
            "Meilensteinbericht",
            vec![
                TemplateSection {
                    heading: "Projekt".into(),
                    body: "{{project_name}}".into(),
                },
                TemplateSection {
                    heading: "Meilensteine".into(),
                    body: "{{milestones}}".into(),
                },
                TemplateSection {
                    heading: "Status".into(),
                    body: "{{status}}".into(),
                },
            ],
        ),
        "fr" => (
            "Rapport de jalons",
            vec![
                TemplateSection {
                    heading: "Projet".into(),
                    body: "{{project_name}}".into(),
                },
                TemplateSection {
                    heading: "Jalons".into(),
                    body: "{{milestones}}".into(),
                },
                TemplateSection {
                    heading: "Statut".into(),
                    body: "{{status}}".into(),
                },
            ],
        ),
        "it" => (
            "Report milestone",
            vec![
                TemplateSection {
                    heading: "Progetto".into(),
                    body: "{{project_name}}".into(),
                },
                TemplateSection {
                    heading: "Milestone".into(),
                    body: "{{milestones}}".into(),
                },
                TemplateSection {
                    heading: "Stato".into(),
                    body: "{{status}}".into(),
                },
            ],
        ),
        _ => (
            "Milestone Report",
            vec![
                TemplateSection {
                    heading: "Project".into(),
                    body: "{{project_name}}".into(),
                },
                TemplateSection {
                    heading: "Milestones".into(),
                    body: "{{milestones}}".into(),
                },
                TemplateSection {
                    heading: "Status".into(),
                    body: "{{status}}".into(),
                },
            ],
        ),
    };
    ReportTemplate {
        name: name.into(),
        sections,
        format: ReportFormat::Markdown,
    }
}

#[allow(clippy::implicit_hasher)]
pub fn render_template(
    template_name: &str,
    language: &str,
    vars: &HashMap<String, String>,
) -> anyhow::Result<String> {
    let tpl = match template_name {
        "weekly_status" => weekly_status_template(language),
        "sprint_review" => sprint_review_template(language),
        "risk_register" => risk_register_template(language),
        "milestone_report" => milestone_report_template(language),
        _ => anyhow::bail!("unsupported template: {}", template_name),
    };
    Ok(tpl.render(vars))
}
