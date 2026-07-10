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

    let chars: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    let n = chars.len();
    while i < n {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        // Line comments: `//` and `#` (covers C/Rust/JS/TS and shell/Python).
        if c == '#' || (c == '/' && i + 1 < n && chars[i + 1] == '/') {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        // Block comments: `/* ... */`.
        if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < n && !(chars[i] == '*' && chars[i + 1] == '/') {
                if chars[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
            continue;
        }
        // Double-quoted string (with escapes); brackets inside are ignored.
        if c == '"' {
            i += 1;
            while i < n {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    i += 1;
                    break;
                }
                if chars[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            continue;
        }
        // Char literal `'x'` / `'\n'`. A bare `'` (Rust lifetime, apostrophe) that
        // is not a valid char literal is ignored rather than toggling string state.
        if c == '\'' {
            let is_char_lit = if i + 1 < n && chars[i + 1] == '\\' {
                i + 3 < n && chars[i + 3] == '\''
            } else {
                i + 2 < n && chars[i + 2] == '\''
            };
            if is_char_lit {
                i += if i + 1 < n && chars[i + 1] == '\\' { 4 } else { 3 };
                continue;
            }
            i += 1;
            continue;
        }
        match c {
            '(' => {
                parens += 1;
                paren_line = line;
            }
            ')' => parens -= 1,
            '{' => {
                braces += 1;
                brace_line = line;
            }
            '}' => braces -= 1,
            '[' => {
                brackets += 1;
                bracket_line = line;
            }
            ']' => brackets -= 1,
            _ => {}
        }
        i += 1;
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
