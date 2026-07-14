// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
pub mod audit;
pub mod condition;
pub mod dispatch;
pub mod engine;
pub mod metrics;
pub mod runner;
pub mod types;

pub use audit::SopAuditLogger;
pub use engine::SopEngine;
pub use metrics::SopMetricsCollector;
pub use types::{
    DeterministicRunState, DeterministicSavings, Sop, SopEvent, SopExecutionMode, SopPriority,
    SopRun, SopRunAction, SopRunStatus, SopStep, SopStepKind, SopStepResult, SopStepStatus,
    SopTrigger, SopTriggerSource, StepSchema,
};

use anyhow::Result;
use std::path::{Path, PathBuf};
use tracing::warn;

use types::{SopManifest, SopMeta};

pub fn parse_execution_mode(s: &str) -> SopExecutionMode {
    match s.trim().to_lowercase().as_str() {
        "auto" => SopExecutionMode::Auto,
        "step_by_step" => SopExecutionMode::StepByStep,
        "priority_based" => SopExecutionMode::PriorityBased,
        "deterministic" => SopExecutionMode::Deterministic,

        _ => SopExecutionMode::Supervised,
    }
}

fn sops_dir(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("sops")
}

pub fn resolve_sops_dir(workspace_dir: &Path, config_dir: Option<&str>) -> PathBuf {
    match config_dir {
        Some(dir) if !dir.is_empty() => {
            let expanded = shellexpand::tilde(dir);
            PathBuf::from(expanded.as_ref())
        }
        _ => sops_dir(workspace_dir),
    }
}

pub fn load_sops(
    workspace_dir: &Path,
    config_dir: Option<&str>,
    default_execution_mode: SopExecutionMode,
) -> Vec<Sop> {
    let dir = resolve_sops_dir(workspace_dir, config_dir);
    load_sops_from_directory(&dir, default_execution_mode)
}

fn load_sops_from_directory(sops_dir: &Path, default_execution_mode: SopExecutionMode) -> Vec<Sop> {
    if !sops_dir.exists() {
        return Vec::new();
    }

    let mut sops = Vec::new();

    let Ok(entries) = std::fs::read_dir(sops_dir) else {
        return sops;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let toml_path = path.join("SOP.toml");
        if !toml_path.exists() {
            continue;
        }

        match load_sop(&path, default_execution_mode) {
            Ok(sop) => sops.push(sop),
            Err(e) => {
                warn!("Failed to load SOP from {}: {e}", path.display());
            }
        }
    }

    sops.sort_by(|a, b| a.name.cmp(&b.name));
    sops
}

fn load_sop(sop_dir: &Path, default_execution_mode: SopExecutionMode) -> Result<Sop> {
    let toml_path = sop_dir.join("SOP.toml");
    let toml_content = std::fs::read_to_string(&toml_path)?;
    let manifest: SopManifest = toml::from_str(&toml_content)?;

    let md_path = sop_dir.join("SOP.md");
    let steps = if md_path.exists() {
        let md_content = std::fs::read_to_string(&md_path)?;
        parse_steps(&md_content)
    } else {
        Vec::new()
    };

    let SopMeta {
        name,
        description,
        version,
        priority,
        execution_mode,
        cooldown_secs,
        max_concurrent,
        deterministic,
    } = manifest.sop;

    let effective_mode = if deterministic {
        SopExecutionMode::Deterministic
    } else {
        execution_mode.unwrap_or(default_execution_mode)
    };

    Ok(Sop {
        name,
        description,
        version,
        priority,
        execution_mode: effective_mode,
        triggers: manifest.triggers,
        steps,
        cooldown_secs,
        max_concurrent,
        location: Some(sop_dir.to_path_buf()),
        deterministic,
    })
}

pub fn parse_steps(md: &str) -> Vec<SopStep> {
    let mut steps = Vec::new();
    let mut in_steps_section = false;
    let mut current_number: Option<u32> = None;
    let mut current_title = String::new();
    let mut current_body = String::new();
    let mut current_tools: Vec<String> = Vec::new();
    let mut current_requires_confirmation = false;
    let mut current_kind = SopStepKind::Execute;

    for line in md.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("## ") {
            if trimmed.eq_ignore_ascii_case("## steps") || trimmed.eq_ignore_ascii_case("## Steps")
            {
                in_steps_section = true;
                continue;
            }

            if in_steps_section {

                flush_step(
                    &mut steps,
                    &mut current_number,
                    &mut current_title,
                    &mut current_body,
                    &mut current_tools,
                    &mut current_requires_confirmation,
                    &mut current_kind,
                );
                in_steps_section = false;
            }
            continue;
        }

        if !in_steps_section {
            continue;
        }

        if let Some(rest) = parse_numbered_item(trimmed) {

            flush_step(
                &mut steps,
                &mut current_number,
                &mut current_title,
                &mut current_body,
                &mut current_tools,
                &mut current_requires_confirmation,
                &mut current_kind,
            );

            let step_num = u32::try_from(steps.len())
                .unwrap_or(u32::MAX)
                .saturating_add(1);
            current_number = Some(step_num);

            if let Some((title, body)) = extract_bold_title(rest) {
                current_title = title;
                current_body = body;
            } else {
                current_title = rest.to_string();
                current_body = String::new();
            }
            current_tools = Vec::new();
            current_requires_confirmation = false;
            continue;
        }

        if current_number.is_some() && trimmed.starts_with("- ") {
            let bullet = trimmed.trim_start_matches("- ").trim();
            if let Some(tools_str) = bullet.strip_prefix("tools:") {
                current_tools = tools_str
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
            } else if bullet.starts_with("requires_confirmation:") {
                if let Some(val) = bullet.strip_prefix("requires_confirmation:") {
                    current_requires_confirmation = val.trim().eq_ignore_ascii_case("true");
                }
            } else if bullet.starts_with("kind:") {
                if let Some(val) = bullet.strip_prefix("kind:") {
                    let val = val.trim();
                    if val.eq_ignore_ascii_case("checkpoint") {
                        current_kind = SopStepKind::Checkpoint;
                    } else {
                        current_kind = SopStepKind::Execute;
                    }
                }
            } else {

                if !current_body.is_empty() {
                    current_body.push('\n');
                }
                current_body.push_str(trimmed);
            }
            continue;
        }

        if current_number.is_some() && !trimmed.is_empty() {
            if !current_body.is_empty() {
                current_body.push('\n');
            }
            current_body.push_str(trimmed);
        }
    }

    flush_step(
        &mut steps,
        &mut current_number,
        &mut current_title,
        &mut current_body,
        &mut current_tools,
        &mut current_requires_confirmation,
        &mut current_kind,
    );

    steps
}

fn flush_step(
    steps: &mut Vec<SopStep>,
    number: &mut Option<u32>,
    title: &mut String,
    body: &mut String,
    tools: &mut Vec<String>,
    requires_confirmation: &mut bool,
    kind: &mut SopStepKind,
) {
    if let Some(n) = number.take() {
        steps.push(SopStep {
            number: n,
            title: std::mem::take(title),
            body: body.trim().to_string(),
            suggested_tools: std::mem::take(tools),
            requires_confirmation: *requires_confirmation,
            kind: *kind,
            schema: None,
        });
        *body = String::new();
        *requires_confirmation = false;
        *kind = SopStepKind::Execute;
    }
}

fn parse_numbered_item(line: &str) -> Option<&str> {
    let dot_pos = line.find(". ")?;
    let prefix = &line[..dot_pos];
    if prefix.chars().all(|c| c.is_ascii_digit()) && !prefix.is_empty() {
        Some(line[dot_pos + 2..].trim())
    } else {
        None
    }
}

fn extract_bold_title(text: &str) -> Option<(String, String)> {
    let start = text.find("**")?;
    let after_start = start + 2;
    let end = text[after_start..].find("**")?;
    let title = text[after_start..after_start + end].to_string();

    let rest_start = after_start + end + 2;
    let rest = text[rest_start..].trim();
    let rest = rest
        .strip_prefix(" - ")
        .or_else(|| rest.strip_prefix("–"))
        .or_else(|| rest.strip_prefix("-"))
        .unwrap_or(rest)
        .trim();

    Some((title, rest.to_string()))
}

pub fn validate_sop(sop: &Sop) -> Vec<String> {
    let mut warnings = Vec::new();

    if sop.name.is_empty() {
        warnings.push("SOP name is empty".into());
    }
    if sop.description.is_empty() {
        warnings.push("SOP description is empty".into());
    }
    if sop.triggers.is_empty() {
        warnings.push("SOP has no triggers defined".into());
    }
    if sop.steps.is_empty() {
        warnings.push("SOP has no steps (missing or empty SOP.md)".into());
    }

    for (i, step) in sop.steps.iter().enumerate() {
        let expected = u32::try_from(i).unwrap_or(u32::MAX).saturating_add(1);
        if step.number != expected {
            warnings.push(format!(
                "Step numbering gap: expected {expected}, got {}",
                step.number
            ));
        }
        if step.title.is_empty() {
            warnings.push(format!("Step {} has an empty title", step.number));
        }
    }

    warnings
}

pub fn handle_command(command: crate::SopCommands, config: &crate::config::Config) -> Result<()> {
    let sops_dir_override = config.sop.sops_dir.as_deref();

    match command {
        crate::SopCommands::List => {
            let sops = load_sops(
                &config.workspace_dir,
                sops_dir_override,
                parse_execution_mode(&config.sop.default_execution_mode),
            );
            if sops.is_empty() {
                println!("No SOPs found.");
                println!();
                println!("  Create one: mkdir -p ~/.senweavercoding/workspace/sops/my-sop");
                println!("              # Add SOP.toml and SOP.md");
                println!();
                println!(
                    "  SOPs directory: {}",
                    resolve_sops_dir(&config.workspace_dir, sops_dir_override).display()
                );
            } else {
                println!("SOPs ({}):", sops.len());
                println!();
                for sop in &sops {
                    let triggers: Vec<String> =
                        sop.triggers.iter().map(ToString::to_string).collect();
                    println!(
                        "  {} {} [{}]  -  {}",
                        console::style(&sop.name).white().bold(),
                        console::style(format!("v{}", sop.version)).dim(),
                        console::style(&sop.priority).cyan(),
                        sop.description
                    );
                    println!(
                        "    Mode: {}  Steps: {}  Triggers: {}",
                        sop.execution_mode,
                        sop.steps.len(),
                        triggers.join(", ")
                    );
                    if sop.cooldown_secs > 0 {
                        println!("    Cooldown: {}s", sop.cooldown_secs);
                    }
                }
            }
            println!();
            Ok(())
        }

        crate::SopCommands::Validate { name } => {
            let sops = load_sops(
                &config.workspace_dir,
                sops_dir_override,
                parse_execution_mode(&config.sop.default_execution_mode),
            );
            let matching: Vec<&Sop> = if let Some(ref name) = name {
                sops.iter().filter(|s| s.name == *name).collect()
            } else {
                sops.iter().collect()
            };

            if matching.is_empty() {
                if let Some(name) = name {
                    anyhow::bail!("SOP not found: {name}");
                }
                println!("No SOPs to validate.");
                return Ok(());
            }

            let mut any_warnings = false;
            for sop in &matching {
                let warnings = validate_sop(sop);
                if warnings.is_empty() {
                    println!(
                        "  {} {}  -  valid",
                        console::style("✓").green().bold(),
                        sop.name
                    );
                } else {
                    any_warnings = true;
                    println!(
                        "  {} {}  -  {} warning(s):",
                        console::style("!").yellow().bold(),
                        sop.name,
                        warnings.len()
                    );
                    for w in &warnings {
                        println!("      {w}");
                    }
                }
            }
            println!();

            if any_warnings {
                anyhow::bail!("Validation completed with warnings");
            }
            Ok(())
        }

        crate::SopCommands::Show { name } => {
            let sops = load_sops(
                &config.workspace_dir,
                sops_dir_override,
                parse_execution_mode(&config.sop.default_execution_mode),
            );
            let sop = sops
                .iter()
                .find(|s| s.name == name)
                .ok_or_else(|| anyhow::anyhow!("SOP not found: {name}"))?;

            println!(
                "{} v{}",
                console::style(&sop.name).white().bold(),
                sop.version
            );
            println!("{}", sop.description);
            println!();
            println!("Priority:       {}", sop.priority);
            println!("Execution mode: {}", sop.execution_mode);
            println!("Cooldown:       {}s", sop.cooldown_secs);
            println!("Max concurrent: {}", sop.max_concurrent);
            println!();

            if !sop.triggers.is_empty() {
                println!("Triggers:");
                for trigger in &sop.triggers {
                    println!("  - {trigger}");
                }
                println!();
            }

            if !sop.steps.is_empty() {
                println!("Steps:");
                for step in &sop.steps {
                    let mut tags = Vec::new();
                    if step.requires_confirmation {
                        tags.push("requires confirmation");
                    }
                    if step.kind == SopStepKind::Checkpoint {
                        tags.push("checkpoint");
                    }
                    let tag_str = if tags.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", tags.join(", "))
                    };
                    println!(
                        "  {}. {}{}",
                        step.number,
                        console::style(&step.title).bold(),
                        tag_str
                    );
                    if !step.body.is_empty() {
                        for line in step.body.lines() {
                            println!("     {line}");
                        }
                    }
                    if !step.suggested_tools.is_empty() {
                        println!("     Tools: {}", step.suggested_tools.join(", "));
                    }
                }
            }
            println!();
            Ok(())
        }
    }
}
