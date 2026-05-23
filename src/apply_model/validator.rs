// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ValidationKind {

    Empty,

    BracketUnbalanced { bracket: String, net: i64, line: u32 },

    TreeSitterError { node_kind: String, line: u32 },

    ValidatorCustom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub code: String,
    pub message: String,

    #[serde(default = "default_kind")]
    pub kind: ValidationKind,
}

fn default_kind() -> ValidationKind {
    ValidationKind::ValidatorCustom("legacy".into())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidationReport {
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn first_kind(&self) -> Option<&ValidationKind> {
        self.issues.first().map(|i| &i.kind)
    }
}

pub fn validate_bytes(s: &str) -> ValidationReport {
    validate_bytes_with_lang(s, None)
}

pub fn validate_bytes_with_lang(s: &str, lang: Option<&str>) -> ValidationReport {
    if s.is_empty() {
        return ValidationReport {
            issues: vec![ValidationIssue {
                code: "empty".into(),
                message: "result is empty".into(),
                kind: ValidationKind::Empty,
            }],
        };
    }

    #[cfg(feature = "code-intel")]
    {
        if let Some(name) = lang {
            if let Some(report) = tree_sitter_validate(s, name) {
                return report;
            }
        }
    }
    #[cfg(not(feature = "code-intel"))]
    {

        let _ = lang;
    }

    bracket_balance_validate(s)
}

#[cfg(feature = "code-intel")]
fn tree_sitter_validate(s: &str, lang: &str) -> Option<ValidationReport> {
    let language = crate::code_intel::grammars::grammar_for(lang)?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(s, None)?;
    let root = tree.root_node();
    if !root.has_error() {
        return Some(ValidationReport::default());
    }

    let mut walker = root.walk();
    let mut stack: Vec<tree_sitter::Node<'_>> = vec![root];
    while let Some(node) = stack.pop() {
        if node.is_error() || node.is_missing() {
            let pos = node.start_position();
            let node_kind = if node.is_missing() {
                format!("missing token: {}", node.kind())
            } else {
                format!("error in {}", node.kind())
            };
            let line = pos.row as u32 + 1;
            return Some(ValidationReport {
                issues: vec![ValidationIssue {
                    code: "tree_sitter".into(),
                    message: format!("{node_kind} @ line {line}"),
                    kind: ValidationKind::TreeSitterError {
                        node_kind,
                        line,
                    },
                }],
            });
        }
        for child in node.children(&mut walker) {
            stack.push(child);
        }
    }
    Some(ValidationReport::default())
}

fn bracket_balance_validate(s: &str) -> ValidationReport {
    let mut issues = Vec::new();
    let mut parens = 0i64;
    let mut braces = 0i64;
    let mut brackets = 0i64;
    let mut paren_line = 0u32;
    let mut brace_line = 0u32;
    let mut bracket_line = 0u32;
    let mut line = 1u32;
    let mut in_string = false;
    let mut escaped = false;
    for c in s.chars() {
        if c == '\n' {
            line += 1;
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '(' if !in_string => {
                parens += 1;
                paren_line = line;
            }
            ')' if !in_string => parens -= 1,
            '{' if !in_string => {
                braces += 1;
                brace_line = line;
            }
            '}' if !in_string => braces -= 1,
            '[' if !in_string => {
                brackets += 1;
                bracket_line = line;
            }
            ']' if !in_string => brackets -= 1,
            _ => {}
        }
    }
    if parens != 0 {
        issues.push(ValidationIssue {
            code: "parens".into(),
            message: format!("unbalanced parentheses (net={parens})"),
            kind: ValidationKind::BracketUnbalanced {
                bracket: "parens".into(),
                net: parens,
                line: paren_line,
            },
        });
    }
    if braces != 0 {
        issues.push(ValidationIssue {
            code: "braces".into(),
            message: format!("unbalanced braces (net={braces})"),
            kind: ValidationKind::BracketUnbalanced {
                bracket: "braces".into(),
                net: braces,
                line: brace_line,
            },
        });
    }
    if brackets != 0 {
        issues.push(ValidationIssue {
            code: "brackets".into(),
            message: format!("unbalanced brackets (net={brackets})"),
            kind: ValidationKind::BracketUnbalanced {
                bracket: "brackets".into(),
                net: brackets,
                line: bracket_line,
            },
        });
    }
    ValidationReport { issues }
}
