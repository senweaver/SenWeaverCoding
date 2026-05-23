// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub index: usize,
    pub content: String,
    pub heading: Option<Rc<str>>,
}

pub fn chunk_markdown(text: &str, max_tokens: usize) -> Vec<Chunk> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let max_chars = max_tokens * 4;
    let sections = split_on_headings(text);
    let mut chunks = Vec::with_capacity(sections.len());

    for (heading, body) in sections {
        let heading: Option<Rc<str>> = heading.map(Rc::from);
        let full = if let Some(ref h) = heading {
            format!("{h}\n{body}")
        } else {
            body.clone()
        };

        if full.len() <= max_chars {
            chunks.push(Chunk {
                index: chunks.len(),
                content: full.trim().to_string(),
                heading: heading.clone(),
            });
        } else {

            let paragraphs = split_on_blank_lines(&body);
            let mut current = heading
                .as_deref()
                .map_or_else(String::new, |h| format!("{h}\n"));

            for para in paragraphs {
                if current.len() + para.len() > max_chars && !current.trim().is_empty() {
                    chunks.push(Chunk {
                        index: chunks.len(),
                        content: current.trim().to_string(),
                        heading: heading.clone(),
                    });
                    current = heading
                        .as_deref()
                        .map_or_else(String::new, |h| format!("{h}\n"));
                }

                if para.len() > max_chars {

                    if !current.trim().is_empty() {
                        chunks.push(Chunk {
                            index: chunks.len(),
                            content: current.trim().to_string(),
                            heading: heading.clone(),
                        });
                        current = heading
                            .as_deref()
                            .map_or_else(String::new, |h| format!("{h}\n"));
                    }
                    for line_chunk in split_on_lines(&para, max_chars) {
                        chunks.push(Chunk {
                            index: chunks.len(),
                            content: line_chunk.trim().to_string(),
                            heading: heading.clone(),
                        });
                    }
                } else {
                    current.push_str(&para);
                    current.push('\n');
                }
            }

            if !current.trim().is_empty() {
                chunks.push(Chunk {
                    index: chunks.len(),
                    content: current.trim().to_string(),
                    heading: heading.clone(),
                });
            }
        }
    }

    chunks.retain(|c| !c.content.is_empty());

    for (i, chunk) in chunks.iter_mut().enumerate() {
        chunk.index = i;
    }

    chunks
}

fn split_on_headings(text: &str) -> Vec<(Option<String>, String)> {
    let mut sections = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_body = String::new();

    for line in text.lines() {
        if line.starts_with("# ") || line.starts_with("## ") || line.starts_with("### ") {
            if !current_body.trim().is_empty() || current_heading.is_some() {
                sections.push((current_heading.take(), std::mem::take(&mut current_body)));
            }
            current_heading = Some(line.to_string());
        } else {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }

    if !current_body.trim().is_empty() || current_heading.is_some() {
        sections.push((current_heading, current_body));
    }

    sections
}

fn split_on_blank_lines(text: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.trim().is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }

    if !current.trim().is_empty() {
        paragraphs.push(current);
    }

    paragraphs
}

fn split_on_lines(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::with_capacity(text.len() / max_chars.max(1) + 1);
    let mut current = String::new();

    for line in text.lines() {
        if current.len() + line.len() + 1 > max_chars && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}
