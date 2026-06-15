// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use std::sync::LazyLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

fn syntect_to_ratatui_color(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

fn plain_code_lines<'a>(code: &str) -> Vec<Line<'a>> {
    LinesWithEndings::from(code)
        .map(|line| Line::from(Span::raw(line.trim_end_matches('\n').to_string())))
        .collect()
}

pub fn highlight_code<'a>(code: &str, language: &str) -> Vec<Line<'a>> {
    let syntax = SYNTAX_SET
        .find_syntax_by_token(language)
        .or_else(|| SYNTAX_SET.find_syntax_by_extension(language))
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());

    let theme = match THEME_SET
        .themes
        .get("base16-ocean.dark")
        .or_else(|| THEME_SET.themes.values().next())
    {
        Some(theme) => theme,
        None => return plain_code_lines(code),
    };
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut lines = Vec::new();
    for line in LinesWithEndings::from(code) {
        let Ok(ranges) = highlighter.highlight_line(line, &SYNTAX_SET) else {
            lines.push(Line::from(Span::raw(line.to_string())));
            continue;
        };

        let spans: Vec<Span<'a>> = ranges
            .into_iter()
            .map(|(style, text)| {
                let fg = syntect_to_ratatui_color(style.foreground);
                Span::styled(text.to_string(), Style::default().fg(fg))
            })
            .collect();
        lines.push(Line::from(spans));
    }
    lines
}

pub fn render_message_with_highlighting<'a>(content: &str) -> Vec<Line<'a>> {
    let mut result = Vec::new();
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buffer = String::new();

    for line in content.lines() {
        if !in_code_block && line.trim_start().starts_with("```") {
            in_code_block = true;
            code_lang = line.trim_start().trim_start_matches('`').trim().to_string();
            code_buffer.clear();
        } else if in_code_block && line.trim_start().starts_with("```") {
            let highlighted = highlight_code(&code_buffer, &code_lang);
            result.push(Line::from(Span::styled(
                format!(
                    "─── {} ───",
                    if code_lang.is_empty() {
                        "code"
                    } else {
                        &code_lang
                    }
                ),
                Style::default().fg(super::theme::ACCENT_DIM),
            )));
            result.extend(highlighted);
            result.push(Line::from(Span::styled(
                "───────────",
                Style::default().fg(super::theme::ACCENT_DIM),
            )));
            in_code_block = false;
            code_lang.clear();
            code_buffer.clear();
        } else if in_code_block {
            code_buffer.push_str(line);
            code_buffer.push('\n');
        } else {
            result.push(Line::from(Span::styled(
                line.to_string(),
                super::theme::normal(),
            )));
        }
    }

    if in_code_block && !code_buffer.is_empty() {
        result.push(Line::from(Span::styled(
            format!(
                "─── {} ───",
                if code_lang.is_empty() {
                    "code"
                } else {
                    &code_lang
                }
            ),
            Style::default().fg(super::theme::ACCENT_DIM),
        )));
        let highlighted = highlight_code(&code_buffer, &code_lang);
        result.extend(highlighted);
    }

    if result.is_empty() {
        result.push(Line::from(Span::raw(content.to_string())));
    }

    result
}
