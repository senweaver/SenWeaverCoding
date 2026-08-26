// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    SenAgent,
    Generic,
}

impl Architecture {
    pub fn parse(id: &str) -> Option<Self> {
        match id {
            "sen-agent" => Some(Architecture::SenAgent),
            "generic" | "agent-skill" => Some(Architecture::Generic),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Architecture::SenAgent => "sen-agent",
            Architecture::Generic => "generic",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Architecture::SenAgent => "Sen agent",
            Architecture::Generic => "Portable agent skill",
        }
    }

    pub fn supports_automation(self) -> bool {
        matches!(self, Architecture::SenAgent)
    }

    pub fn skill_catalogue(self) -> &'static str {
        match self {
            Architecture::SenAgent => SEN_AGENT_CATALOGUE,
            Architecture::Generic => GENERIC_CATALOGUE,
        }
    }

    pub fn automation_catalogue(self) -> &'static str {
        SEN_AGENT_AUTOMATION_CATALOGUE
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchitectureTarget {
    pub architecture: String,
    pub label: String,
    pub kind: String,
    pub placements: Vec<String>,
}

pub fn build_targets() -> Vec<ArchitectureTarget> {
    vec![
        ArchitectureTarget {
            architecture: "sen-agent".to_string(),
            label: "Sen skill".to_string(),
            kind: "skill".to_string(),
            placements: vec!["install".to_string(), "export".to_string()],
        },
        ArchitectureTarget {
            architecture: "sen-agent".to_string(),
            label: "Sen automation".to_string(),
            kind: "automation".to_string(),
            placements: vec!["install".to_string(), "export".to_string()],
        },
        ArchitectureTarget {
            architecture: "generic".to_string(),
            label: "Portable skill".to_string(),
            kind: "skill".to_string(),
            placements: vec!["export".to_string()],
        },
    ]
}

const SEN_AGENT_CATALOGUE: &str = r#"# Target: Sen coding agent — native capability catalogue

A Sen skill is a SKILL.md file: YAML frontmatter (name, description, optional allowed-tools)
followed by a markdown instructions body. Sen auto-loads it from the workspace skills folder.

Frontmatter:
- name — kebab-case, ^[a-z0-9-]+$.
- description — one line of trigger keywords (when the agent should reach for this skill).
- allowed-tools (optional) — YAML list of tool patterns the skill may use, e.g.
  Read, Write, Edit, Grep, Glob, Bash(gh *), Bash(git *).

## Prefer native capabilities over UI replay
Map each recorded UI action to a native tool. Sen has:
1. Files — Read / Write / Edit / Grep / Glob for reading and editing files in the workspace.
2. Shell — Bash tool for running commands and CLIs. Prefer first-class CLIs over the browser:
   GitHub → the `gh` CLI (never drive github.com through the browser), plus `git` and cloud CLIs.
   Gate the shell with allowed-tools like Bash(gh *). Commands run on the device OS
   (PowerShell on Windows, bash/zsh on macOS/Linux).
3. Web — web_fetch to fetch a URL's contents, web_search for lookups.
4. Embedded browser — the browser dock automates a real page (navigate, click, type, read)
   ONLY for UI-only web apps with no API or CLI. Snapshot before you act.
5. Computer replay — for genuinely GUI-only desktop steps, the recorded computer-use
   procedure itself is the fallback; describe the click/type sequence by the element it targets.

## Writing the SKILL.md body
- Write a GENERALIZED procedure: if the recording acted on N specific items, the body loops
  over ALL items of that kind, not the specific examples recorded.
- Reference each fixed value by its {{id}} token, never inline the literal.
- Keep it concise and imperative. Include a short "When to use" and the ordered steps.
- Rely ONLY on the built-in tools above, never on a skill the user might have added."#;

const GENERIC_CATALOGUE: &str = r#"# Target: Any AI agent with skill support — portable catalogue

A portable skill is a SKILL.md file: optional YAML frontmatter followed by a markdown
instructions body. It targets no specific host, so it must NOT assume proprietary built-in tools.

Frontmatter:
- name — kebab-case, ^[a-z0-9-]+$.
- description — one line of trigger keywords.
- allowed-tools (optional) — tool patterns, e.g. Bash(gh *), Read, Write, Grep, Glob.

## Assume only portable capabilities
- Do NOT assume host-specific integrations and no browser automation. Rely only on
  capabilities every agent can provide: reading and writing files, running shell commands
  and standard CLIs, and calling documented HTTP APIs.
- Map each recorded UI action to a portable tool, an API call, or a CLI — never write
  "click" / "type" UI steps.

## Writing the SKILL.md body
- Write a GENERALIZED procedure that loops over ALL items of a kind, not the recorded examples.
- Reference each fixed value by its {{id}} token, never inline the literal.
- Keep it concise and imperative with a short "When to use" and ordered steps."#;

const SEN_AGENT_AUTOMATION_CATALOGUE: &str = r#"# Target: Sen automation — scheduled multi-step procedure

An automation runs unattended on a schedule. It has a trigger and ordered steps, each a
natural-language prompt the Sen agent executes in sequence.

## Propose the trigger (you must infer it)
The recording has no "when to run" signal, so default to a schedule:
- single — once a day at a time of day.
- interval — every N minutes where N divides 1440 evenly, from an anchor time.
- multi — a few fixed times of day.
Always set both naturalLanguage AND the structured fields. Use a condition trigger only when
the recording clearly implies an event to watch for.

## Steps are prompts
Each step is a short label plus an imperative natural-language prompt to the agent. Because an
automation runs unattended and can't stop to ask a human, every step must be self-resolving:
locate its own inputs, handle the empty/missing case, and make destructive actions explicit.
Aim for 2-6 steps. Reference each fixed value by its {{id}} token.

## Prefer native capabilities
Same capability ladder as skills: files, shell/CLIs (gh/git), web_fetch/web_search, the
embedded browser only for UI-only web apps."#;
