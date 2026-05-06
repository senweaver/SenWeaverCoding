// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Syntactic verifier — tree-sitter parse-error detection.
//!
//! the verifier now actually consumes tree-sitter when the
//! `code-intel` feature is on.  We walk the parse tree depth-first and
//! collect every node where `is_error()` *or* `is_missing()` returns
//! true; the missing-token branch is what catches "code that compiles
//! to a tree but is structurally broken" — e.g. `fn main() { let x =
//! 1 }` (Rust requires a `;` and tree-sitter inserts a MISSING node
//! for it).
//!
//! When `code-intel` is disabled (or the language has no registered
//! grammar) the verifier degrades gracefully to the bracket-balance
//! heuristic that has shipped since D2.1.  The summary is
//! tagged `degraded` so callers can distinguish a real pass from a
//! best-effort one.

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

        let trimmed = if line.len() > 120 { &line[..120] } else { line };
        buf.push_str(&format!("L{}: {}", idx + 1, trimmed.trim_end()));
    }
    buf
}

pub(crate) fn heuristic_check(source: &str, _lang: Language) -> VerificationReport {
    let mut stack: Vec<(char, u32, u32)> = Vec::new();
    let mut in_string = false;
    let mut string_char = '"';
    let mut line: u32 = 1;
    let mut col: u32 = 1;
    let mut prev_was_backslash = false;

    for ch in source.chars() {
        match ch {
            '\n' => {
                line += 1;
                col = 1;
                in_string = false;
                prev_was_backslash = false;
                continue;
            }
            '\\' if in_string => {
                prev_was_backslash = !prev_was_backslash;
            }
            '"' | '\'' => {
                if in_string && ch == string_char && !prev_was_backslash {
                    in_string = false;
                } else if !in_string {
                    in_string = true;
                    string_char = ch;
                }
                prev_was_backslash = false;
            }
            '(' | '[' | '{' if !in_string => {
                stack.push((ch, line, col));
            }
            ')' | ']' | '}' if !in_string => {
                let expected = match ch {
                    ')' => '(',
                    ']' => '[',
                    '}' => '{',
                    _ => unreachable!(),
                };
                match stack.pop() {
                    Some((o, _, _)) if o == expected => {}
                    _ => {
                        return VerificationReport::failed(
                            "syntactic",
                            vec![VerificationIssue {
                                line,
                                column: col,
                                message: format!("unmatched closing bracket '{ch}'"),
                                severity: IssueSeverity::Error,
                            }],
                            String::new(),
                        );
                    }
                }
            }
            _ => {
                prev_was_backslash = false;
            }
        }
        col += 1;
    }

    if let Some((ch, l, c)) = stack.into_iter().next() {
        return VerificationReport::failed(
            "syntactic",
            vec![VerificationIssue {
                line: l,
                column: c,
                message: format!("unclosed bracket '{ch}'"),
                severity: IssueSeverity::Error,
            }],
            String::new(),
        );
    }

    VerificationReport::ok("syntactic")
}
