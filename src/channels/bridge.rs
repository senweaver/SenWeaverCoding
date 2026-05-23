// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageFormat {

    PlainText,

    Markdown,

    TelegramMarkdownV2,

    SlackMrkdwn,

    DiscordMarkdown,

    Html,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgedMessage {

    pub content: String,

    pub source_format: MessageFormat,

    pub sender: Option<String>,

    pub source_channel: Option<String>,

    pub attachments: Vec<Attachment>,

    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {

    pub name: String,

    pub mime_type: String,

    pub data: AttachmentData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentData {

    Url(String),

    Base64(String),
}

pub struct ChannelBridge {
    default_format: MessageFormat,
}

impl ChannelBridge {

    pub fn new(default_format: MessageFormat) -> Self {
        Self { default_format }
    }

    pub fn convert(&self, msg: &BridgedMessage, target: MessageFormat) -> String {
        Self::convert_inner(msg, target)
    }

    fn convert_inner(msg: &BridgedMessage, target: MessageFormat) -> String {
        if msg.source_format == target {
            return msg.content.clone();
        }

        match (msg.source_format, target) {
            (MessageFormat::Markdown, MessageFormat::PlainText) => strip_markdown(&msg.content),
            (MessageFormat::Markdown, MessageFormat::TelegramMarkdownV2) => {
                markdown_to_telegram(&msg.content)
            }
            (MessageFormat::Markdown, MessageFormat::SlackMrkdwn) => {
                markdown_to_slack(&msg.content)
            }
            (MessageFormat::Markdown, MessageFormat::DiscordMarkdown) => msg.content.clone(),
            (MessageFormat::Markdown, MessageFormat::Html) => markdown_to_html(&msg.content),

            (
                MessageFormat::PlainText,
                MessageFormat::Markdown | MessageFormat::DiscordMarkdown,
            ) => msg.content.clone(),
            (MessageFormat::PlainText, MessageFormat::TelegramMarkdownV2) => {
                escape_telegram(&msg.content)
            }
            (MessageFormat::PlainText, MessageFormat::SlackMrkdwn) => escape_slack(&msg.content),
            (MessageFormat::PlainText, MessageFormat::Html) => html_escape(&msg.content),

            (MessageFormat::TelegramMarkdownV2, MessageFormat::PlainText) => {
                strip_telegram_markdown(&msg.content)
            }
            (MessageFormat::TelegramMarkdownV2, MessageFormat::Markdown) => {
                telegram_to_markdown(&msg.content)
            }
            (MessageFormat::TelegramMarkdownV2, _) => {
                let md = telegram_to_markdown(&msg.content);
                let intermediate = BridgedMessage {
                    content: md,
                    source_format: MessageFormat::Markdown,
                    ..msg.clone()
                };
                Self::convert_inner(&intermediate, target)
            }

            (MessageFormat::SlackMrkdwn, MessageFormat::PlainText) => {
                strip_slack_mrkdwn(&msg.content)
            }
            (MessageFormat::SlackMrkdwn, MessageFormat::Markdown) => {
                slack_to_markdown(&msg.content)
            }
            (MessageFormat::SlackMrkdwn, _) => {
                let md = slack_to_markdown(&msg.content);
                let intermediate = BridgedMessage {
                    content: md,
                    source_format: MessageFormat::Markdown,
                    ..msg.clone()
                };
                Self::convert_inner(&intermediate, target)
            }

            (MessageFormat::DiscordMarkdown, target_fmt) => {
                let intermediate = BridgedMessage {
                    content: msg.content.clone(),
                    source_format: MessageFormat::Markdown,
                    ..msg.clone()
                };
                Self::convert_inner(&intermediate, target_fmt)
            }

            (MessageFormat::Html, MessageFormat::PlainText) => strip_html(&msg.content),
            (MessageFormat::Html, _) => strip_html(&msg.content),

            _ => msg.content.clone(),
        }
    }

    pub fn to_default(&self, msg: &BridgedMessage) -> String {
        self.convert(msg, self.default_format)
    }

    pub fn bridge(content: String, format: MessageFormat) -> BridgedMessage {
        BridgedMessage {
            content,
            source_format: format,
            sender: None,
            source_channel: None,
            attachments: Vec::new(),
            metadata: HashMap::new(),
        }
    }
}

impl Default for ChannelBridge {
    fn default() -> Self {
        Self::new(MessageFormat::Markdown)
    }
}

fn strip_markdown(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '*' | '_' | '~' | '`' => {}
            '#' => {
                while chars.peek() == Some(&'#') {
                    chars.next();
                }
                if chars.peek() == Some(&' ') {
                    chars.next();
                }
            }
            '[' => {
                let mut text = String::new();
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                    text.push(c);
                }
                if chars.peek() == Some(&'(') {
                    chars.next();
                    for c in chars.by_ref() {
                        if c == ')' {
                            break;
                        }
                    }
                }
                out.push_str(&text);
                continue;
            }
            _ => out.push(ch),
        }
    }
    out
}

fn markdown_to_telegram(s: &str) -> String {
    s.replace('_', "\\_")
        .replace('~', "\\~")
        .replace('>', "\\>")
        .replace('#', "\\#")
        .replace('+', "\\+")
        .replace('-', "\\-")
        .replace('=', "\\=")
        .replace('|', "\\|")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('.', "\\.")
        .replace('!', "\\!")
}

fn escape_telegram(s: &str) -> String {
    markdown_to_telegram(s)
}

fn markdown_to_slack(s: &str) -> String {
    s.replace("**", "*").replace("__", "_").replace("~~", "~")
}

fn escape_slack(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn markdown_to_html(s: &str) -> String {
    let mut out = html_escape(s);
    let mut bold_open = true;
    while out.contains("**") {
        let tag = if bold_open { "<strong>" } else { "</strong>" };
        out = out.replacen("**", tag, 1);
        bold_open = !bold_open;
    }
    let mut em_open = true;
    while out.contains("__") {
        let tag = if em_open { "<em>" } else { "</em>" };
        out = out.replacen("__", tag, 1);
        em_open = !em_open;
    }
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
}

fn strip_telegram_markdown(s: &str) -> String {
    s.replace("\\_", "_")
        .replace("\\~", "~")
        .replace("\\>", ">")
        .replace("\\#", "#")
        .replace("\\+", "+")
        .replace("\\-", "-")
        .replace("\\=", "=")
        .replace("\\|", "|")
        .replace("\\{", "{")
        .replace("\\}", "}")
        .replace("\\.", ".")
        .replace("\\!", "!")
}

fn telegram_to_markdown(s: &str) -> String {
    strip_telegram_markdown(s)
}

fn strip_slack_mrkdwn(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn slack_to_markdown(s: &str) -> String {
    strip_slack_mrkdwn(s)
}
