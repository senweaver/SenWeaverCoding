// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Channel Format Bridge - unified cross-channel message routing and format normalization.
//!
//! Provides a `ChannelBridge` that normalizes messages between different channel
//! formats (Telegram Markdown, Discord Markdown, Slack mrkdwn, plain text, HTML)
//! and routes messages across channels.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Supported message format types across channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageFormat {
    /// Plain UTF-8 text, no formatting.
    PlainText,
    /// Standard Markdown (CommonMark).
    Markdown,
    /// Telegram's MarkdownV2 dialect.
    TelegramMarkdownV2,
    /// Slack's mrkdwn format.
    SlackMrkdwn,
    /// Discord's Markdown dialect.
    DiscordMarkdown,
    /// HTML (for email, web).
    Html,
}

/// A normalized message that can be converted to any channel format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgedMessage {
    /// Original text content.
    pub content: String,
    /// Original format of the content.
    pub source_format: MessageFormat,
    /// Optional sender identifier.
    pub sender: Option<String>,
    /// Optional channel source identifier.
    pub source_channel: Option<String>,
    /// Optional attachments (URLs or base64 data).
    pub attachments: Vec<Attachment>,
    /// Optional metadata.
    pub metadata: HashMap<String, String>,
}

/// An attachment in a bridged message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// Filename or identifier.
    pub name: String,
    /// MIME type.
    pub mime_type: String,
    /// URL or inline data.
    pub data: AttachmentData,
}

/// Attachment data source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentData {
    /// URL to fetch the attachment from.
    Url(String),
    /// Base64-encoded inline data.
    Base64(String),
}

/// Channel bridge for format conversion and message routing.
pub struct ChannelBridge {
    default_format: MessageFormat,
}

impl ChannelBridge {
    /// Create a new channel bridge with a default output format.
    pub fn new(default_format: MessageFormat) -> Self {
        Self { default_format }
    }

    /// Convert a bridged message to a target format.
    pub fn convert(&self, msg: &BridgedMessage, target: MessageFormat) -> String {
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

            (MessageFormat::PlainText, MessageFormat::Markdown)
            | (MessageFormat::PlainText, MessageFormat::DiscordMarkdown) => msg.content.clone(),
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
                self.convert(&intermediate, target)
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
                self.convert(&intermediate, target)
            }

            (MessageFormat::DiscordMarkdown, target_fmt) => {
                let intermediate = BridgedMessage {
                    content: msg.content.clone(),
                    source_format: MessageFormat::Markdown,
                    ..msg.clone()
                };
                self.convert(&intermediate, target_fmt)
            }

            (MessageFormat::Html, MessageFormat::PlainText) => strip_html(&msg.content),
            (MessageFormat::Html, _) => strip_html(&msg.content),

            _ => msg.content.clone(),
        }
    }

    /// Convert a message to the bridge's default format.
    pub fn to_default(&self, msg: &BridgedMessage) -> String {
        self.convert(msg, self.default_format)
    }

    /// Create a bridged message from raw content.
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

// ── Format conversion helpers ──

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_to_telegram() {
        let bridge = ChannelBridge::default();
        let msg = ChannelBridge::bridge("Hello World!".into(), MessageFormat::PlainText);
        let result = bridge.convert(&msg, MessageFormat::TelegramMarkdownV2);
        assert!(result.contains("Hello World\\!"));
    }

    #[test]
    fn markdown_to_plain() {
        let bridge = ChannelBridge::default();
        let msg = ChannelBridge::bridge("**bold** and _italic_".into(), MessageFormat::Markdown);
        let result = bridge.convert(&msg, MessageFormat::PlainText);
        assert_eq!(result, "bold and italic");
    }

    #[test]
    fn same_format_passthrough() {
        let bridge = ChannelBridge::default();
        let msg = ChannelBridge::bridge("Hello".into(), MessageFormat::Markdown);
        let result = bridge.convert(&msg, MessageFormat::Markdown);
        assert_eq!(result, "Hello");
    }

    #[test]
    fn markdown_to_slack() {
        let bridge = ChannelBridge::default();
        let msg = ChannelBridge::bridge("**bold** text".into(), MessageFormat::Markdown);
        let result = bridge.convert(&msg, MessageFormat::SlackMrkdwn);
        assert_eq!(result, "*bold* text");
    }

    #[test]
    fn html_strip() {
        let bridge = ChannelBridge::default();
        let msg = ChannelBridge::bridge("<p>Hello &amp; world</p>".into(), MessageFormat::Html);
        let result = bridge.convert(&msg, MessageFormat::PlainText);
        assert_eq!(result, "Hello & world");
    }

    #[test]
    fn bridge_factory() {
        let msg = ChannelBridge::bridge("test".into(), MessageFormat::PlainText);
        assert_eq!(msg.content, "test");
        assert_eq!(msg.source_format, MessageFormat::PlainText);
        assert!(msg.attachments.is_empty());
    }

    #[test]
    fn link_extraction() {
        let bridge = ChannelBridge::default();
        let msg = ChannelBridge::bridge(
            "[click here](https://example.com)".into(),
            MessageFormat::Markdown,
        );
        let result = bridge.convert(&msg, MessageFormat::PlainText);
        assert_eq!(result, "click here");
    }
}
