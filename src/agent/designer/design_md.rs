// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

const MAX_BATON_INJECT: usize = 8_000;

#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

fn split_frontmatter(md: &str) -> (Option<&str>, &str) {
    let trimmed = md.trim_start_matches('\u{feff}');
    let Some(rest) = trimmed.strip_prefix("---") else {
        return (None, md);
    };
    let rest = rest.trim_start_matches(['\r', '\n']);
    let Some(end) = rest.find("\n---") else {
        return (None, md);
    };
    let fm = &rest[..end];
    let after = &rest[end + 4..];
    let body = match after.split_once('\n') {
        Some((_, b)) => b,
        None => "",
    };
    (Some(fm), body)
}

pub fn validate(content: &str) -> ValidationReport {
    let mut report = ValidationReport::default();
    let (fm, body) = split_frontmatter(content);
    let Some(fm) = fm else {
        report.errors.push(
            "Missing YAML frontmatter: DESIGN.md must start with `---` and define the token \
             contract (name, colors, typography, spacing)."
                .to_string(),
        );
        return report;
    };
    let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(fm);
    let value = match parsed {
        Ok(v) => v,
        Err(e) => {
            report
                .errors
                .push(format!("Frontmatter is not valid YAML: {e}"));
            return report;
        }
    };
    let Some(map) = value.as_mapping() else {
        report
            .errors
            .push("Frontmatter must be a YAML mapping of token groups.".to_string());
        return report;
    };
    let get = |key: &str| map.get(serde_yaml::Value::String(key.to_string()));

    for required in ["name", "colors", "typography", "spacing"] {
        if get(required).is_none() {
            report
                .errors
                .push(format!("Missing required frontmatter key `{required}`."));
        }
    }
    if let Some(colors) = get("colors").and_then(|v| v.as_mapping()) {
        for c in ["background", "text", "accent"] {
            if !colors.contains_key(serde_yaml::Value::String(c.to_string())) {
                report
                    .errors
                    .push(format!("`colors` must define at least `{c}`."));
            }
        }
    }
    if let Some(typo) = get("typography").and_then(|v| v.as_mapping()) {
        if !typo.contains_key(serde_yaml::Value::String("body".to_string())) {
            report
                .errors
                .push("`typography` must define at least a `body` style.".to_string());
        }
    }
    for recommended in ["rounded", "components"] {
        if get(recommended).is_none() {
            report.warnings.push(format!(
                "Frontmatter key `{recommended}` is missing — recommended for a complete baton."
            ));
        }
    }
    for section in ["## Colors", "## Typography", "## Layout", "## Components"] {
        if !body.contains(section) {
            report.warnings.push(format!(
                "Body section `{section}` is missing — document how the tokens are applied."
            ));
        }
    }
    report
}

pub fn format_validation(rel: &str, report: &ValidationReport) -> String {
    let mut out = format!(
        "DESIGN.md baton check — {rel}: {} error(s), {} warning(s).\n",
        report.errors.len(),
        report.warnings.len()
    );
    for e in &report.errors {
        out.push_str(&format!("- [error] {e}\n"));
    }
    for w in &report.warnings {
        out.push_str(&format!("- [warn] {w}\n"));
    }
    if report.is_valid() {
        out.push_str(
            "Baton is structurally valid. Keep it updated whenever the visual direction changes.",
        );
    } else {
        out.push_str(
            "Fix every error before shipping: artifacts must bind to a valid DESIGN.md baton.",
        );
    }
    out
}

pub fn starter() -> Option<&'static str> {
    super::scaffold::read("design-system-starter")
}

pub fn baton_rel_path(session_id: &str) -> String {
    format!("{}/DESIGN.md", super::pipeline::designer_session_dir(session_id))
}

pub fn injection() -> Option<String> {
    let session = crate::session::current_session_context()?;
    let rel = baton_rel_path(&session.session_id);
    let abs = std::path::Path::new(&session.workspace_dir).join(&rel);

    let contract = format!(
        "\n### Design baton — DESIGN.md (session source of truth)\n\
         The file `{rel}` is the evolving design source of truth for every artifact in this \
         session. Rules:\n\
         - Before producing the first substantive HTML artifact, create it: call \
         `designer_scaffold` with `id=design-system-starter` and `dest={rel}`, then immediately \
         rewrite its tokens (colors, typography, rounded, spacing, components) to match the brief \
         and the active design system.\n\
         - Bind every artifact to the baton: derive CSS custom properties from its tokens instead \
         of inventing per-file values, so all screens in this session stay visually coherent.\n\
         - When the user changes the visual direction, update `{rel}` FIRST, then propagate to \
         the affected artifacts.\n\
         - Validate it with `designer_lint` (`path={rel}`) during the critique stage; fix every \
         reported error.\n"
    );

    match std::fs::read_to_string(&abs) {
        Ok(content) => {
            let report = validate(&content);
            let status = if report.is_valid() {
                "valid".to_string()
            } else {
                format!("INVALID — {} error(s), repair it this turn", report.errors.len())
            };
            let mut body = content;
            if body.len() > MAX_BATON_INJECT {
                let mut cut = MAX_BATON_INJECT;
                while cut > 0 && !body.is_char_boundary(cut) {
                    cut -= 1;
                }
                body.truncate(cut);
                body.push_str("\n[truncated — read the full file from disk]");
            }
            Some(format!(
                "{contract}\nCurrent baton (`{rel}`, status: {status}):\n\n{body}\n"
            ))
        }
        Err(_) => Some(contract),
    }
}
