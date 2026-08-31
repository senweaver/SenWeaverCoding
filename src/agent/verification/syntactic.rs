// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;

use super::traits::{
    Artifact, IssueSeverity, Language, VerificationIssue, VerificationReport, Verifier,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct SyntacticVerifier;

impl SyntacticVerifier {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Verifier for SyntacticVerifier {
    fn name(&self) -> &'static str {
        "syntactic"
    }

    async fn verify(&self, artifact: &Artifact) -> anyhow::Result<VerificationReport> {

        if matches!(artifact.kind, super::traits::ArtifactKind::Workspace) {
            return Ok(VerificationReport::ok(self.name()));
        }

        if artifact.contents.is_empty() {
            return Ok(VerificationReport::ok(self.name()));
        }

        #[cfg(feature = "code-intel")]
        {
            if let Some(report) = tree_sitter_check(&artifact.contents, artifact.language) {
                return Ok(report);
            }
        }

        let mut report = heuristic_check(&artifact.contents, artifact.language);
        if report.summary.is_empty() {
            report.summary = "degraded:bracket-balance".into();
        }
        Ok(report)
    }
}

#[cfg(feature = "code-intel")]
fn tree_sitter_check(source: &str, lang: Language) -> Option<VerificationReport> {
    let id = lang.grammar_id()?;
    let language = crate::code_intel::grammars::grammar_for(id)?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(source, None)?;

    let mut errors: Vec<VerificationIssue> = Vec::new();
    collect_error_nodes(tree.root_node(), source, &mut errors);

    if errors.is_empty() {
        return Some(VerificationReport::ok("syntactic"));
    }

    let first = &errors[0];
    let summary = format!("tree_sitter:{}@{}:{}", first.message, first.line, first.column);
    Some(VerificationReport::failed("syntactic", errors, summary))
}

#[cfg(feature = "code-intel")]
fn collect_error_nodes(node: tree_sitter::Node<'_>, source: &str, out: &mut Vec<VerificationIssue>) {
    const MAX_ISSUES: usize = 64;
    if out.len() >= MAX_ISSUES {
        return;
    }

    if node.is_error() || node.is_missing() {
        let start = node.start_position();
        let kind = if node.is_missing() {

            format!("missing token: {}", node.kind())
        } else {
            format!("syntax error in {}", node.kind())
        };
        let snippet = context_snippet(source, start.row);
        let message = if snippet.is_empty() {
            kind
        } else {
            format!("{kind} | context: {snippet}")
        };
        out.push(VerificationIssue {
            line: start.row as u32 + 1,
            column: start.column as u32 + 1,
            message,
            severity: IssueSeverity::Error,
        });

        if !node.is_missing() && node.is_error() {
            return;
        }
    }

    let mut walker = node.walk();
    for child in node.children(&mut walker) {
        collect_error_nodes(child, source, out);
        if out.len() >= MAX_ISSUES {
            return;
        }
    }
}

#[cfg(feature = "code-intel")]
fn context_snippet(source: &str, row: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let lo = row.saturating_sub(1);
    let hi = (row + 2).min(lines.len());
    let mut buf = String::new();
    for (idx, line) in lines.iter().enumerate().take(hi).skip(lo) {
        if !buf.is_empty() {
            buf.push_str(" \\n ");
        }

        let trimmed = if line.len() > 120 {
            crate::util::truncate_str_bytes(line, 120)
        } else {
            line
        };
        buf.push_str(&format!("L{}: {}", idx + 1, trimmed.trim_end()));
    }
    buf
}

pub(crate) fn heuristic_check(source: &str, lang: Language) -> VerificationReport {
    let single_quote_strings = matches!(
        lang,
        Language::Python | Language::JavaScript | Language::TypeScript
    );
    let line_comment: Option<&str> = match lang {
        Language::Python | Language::Toml => Some("#"),
        Language::Rust
        | Language::JavaScript
        | Language::TypeScript
        | Language::Go
        | Language::Java
        | Language::C
        | Language::Cpp => Some("//"),
        _ => None,
    };

    let mut stack: Vec<(char, u32, u32)> = Vec::new();
    let mut first_unmatched: Option<(char, u32, u32)> = None;

    'lines: for (line_idx, raw_line) in source.lines().enumerate() {
        let line = line_idx as u32 + 1;
        let chars: Vec<char> = raw_line.chars().collect();
        let mut i = 0usize;
        let mut in_string = false;
        let mut string_char = '"';
        let mut escaped = false;

        while i < chars.len() {
            let ch = chars[i];
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == string_char {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            if let Some(marker) = line_comment {
                let marker_chars: Vec<char> = marker.chars().collect();
                if chars[i..].starts_with(marker_chars.as_slice()) {
                    continue 'lines;
                }
            }
            match ch {
                '"' => {
                    in_string = true;
                    string_char = '"';
                }
                '\'' => {
                    if single_quote_strings {
                        in_string = true;
                        string_char = '\'';
                    } else {
                        let close_rel = chars[i + 1..]
                            .iter()
                            .take(3)
                            .position(|c| *c == '\'');
                        if let Some(rel) = close_rel {
                            i += rel + 2;
                            continue;
                        }
                    }
                }
                '(' | '[' | '{' => {
                    stack.push((ch, line, i as u32 + 1));
                }
                ')' | ']' | '}' => {
                    let expected = match ch {
                        ')' => '(',
                        ']' => '[',
                        _ => '{',
                    };
                    match stack.pop() {
                        Some((o, _, _)) if o == expected => {}
                        _ => {
                            if first_unmatched.is_none() {
                                first_unmatched = Some((ch, line, i as u32 + 1));
                            }
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    let issue = match (first_unmatched, stack.into_iter().next()) {
        (Some((ch, l, c)), _) => Some((format!("unmatched closing bracket '{ch}'"), l, c)),
        (None, Some((ch, l, c))) => Some((format!("unclosed bracket '{ch}'"), l, c)),
        (None, None) => None,
    };

    match issue {
        Some((message, l, c)) => VerificationReport {
            verifier: "syntactic",
            passed: true,
            issues: vec![VerificationIssue {
                line: l,
                column: c,
                message: format!(
                    "{message} (degraded bracket-balance heuristic; may be a false positive \
                     in strings/comments - verify with the project's own compiler)"
                ),
                severity: IssueSeverity::Warning,
            }],
            summary: "degraded:bracket-balance-warning".into(),
        },
        None => VerificationReport::ok("syntactic"),
    }
}
