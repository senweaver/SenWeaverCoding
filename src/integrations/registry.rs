// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::{IntegrationCategory, IntegrationEntry, IntegrationStatus};
use crate::providers::{
    is_glm_alias, is_minimax_alias, is_moonshot_alias, is_qianfan_alias, is_qwen_alias,
    is_zai_alias,
};

#[allow(clippy::too_many_lines)]
pub fn all_integrations() -> Vec<IntegrationEntry> {
    vec![

        IntegrationEntry {
            name: "Telegram",
            description: "Bot API  -  long-polling",
            category: IntegrationCategory::Chat,
            status_fn: |c| {
                if c.channels_config.telegram.is_some() {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Discord",
            description: "Servers, channels & DMs",
            category: IntegrationCategory::Chat,
            status_fn: |c| {
                if c.channels_config.discord.is_some() {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Slack",
            description: "Workspace apps via Web API",
            category: IntegrationCategory::Chat,
            status_fn: |c| {
                if c.channels_config.slack.is_some() {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Webhooks",
            description: "HTTP endpoint for triggers",
            category: IntegrationCategory::Chat,
            status_fn: |c| {
                if c.channels_config.webhook.is_some() {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "WhatsApp",
            description: "Meta Cloud API via webhook",
            category: IntegrationCategory::Chat,
            status_fn: |c| {
                if c.channels_config.whatsapp.is_some() {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Signal",
            description: "Privacy-focused via signal-cli",
            category: IntegrationCategory::Chat,
            status_fn: |c| {
                if c.channels_config.signal.is_some() {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "iMessage",
            description: "macOS AppleScript bridge",
            category: IntegrationCategory::Chat,
            status_fn: |c| {
                if c.channels_config.imessage.is_some() {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Microsoft Teams",
            description: "Enterprise chat support",
            category: IntegrationCategory::Chat,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Matrix",
            description: "Matrix protocol (Element)",
            category: IntegrationCategory::Chat,
            status_fn: |c| {
                if c.channels_config.matrix.is_some() {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Nostr",
            description: "Decentralized DMs (NIP-04)",
            category: IntegrationCategory::Chat,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "WebChat",
            description: "Browser-based chat UI",
            category: IntegrationCategory::Chat,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Nextcloud Talk",
            description: "Self-hosted Nextcloud chat",
            category: IntegrationCategory::Chat,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Zalo",
            description: "Zalo Bot API",
            category: IntegrationCategory::Chat,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "DingTalk",
            description: "DingTalk Stream Mode",
            category: IntegrationCategory::Chat,
            status_fn: |c| {
                if c.channels_config.dingtalk.is_some() {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "QQ Official",
            description: "Tencent QQ Bot SDK",
            category: IntegrationCategory::Chat,
            status_fn: |c| {
                if c.channels_config.qq.is_some() {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },

        IntegrationEntry {
            name: "OpenRouter",
            description: "200+ models, 1 API key",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("openrouter") && c.api_key.is_some() {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Anthropic",
            description: "Claude 3.5/4 Sonnet & Opus",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("anthropic") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "OpenAI",
            description: "GPT-4o, GPT-5, o1",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("openai") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Google",
            description: "Gemini 2.5 Pro/Flash",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_model
                    .as_deref()
                    .is_some_and(|m| m.starts_with("google/"))
                {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "DeepSeek",
            description: "DeepSeek V3 & R1",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_model
                    .as_deref()
                    .is_some_and(|m| m.starts_with("deepseek/"))
                {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "xAI",
            description: "Grok 3 & 4",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_model
                    .as_deref()
                    .is_some_and(|m| m.starts_with("x-ai/"))
                {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Mistral",
            description: "Mistral Large & Codestral",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_model
                    .as_deref()
                    .is_some_and(|m| m.starts_with("mistral"))
                {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Ollama",
            description: "Local models (Llama, etc.)",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("ollama") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Perplexity",
            description: "Search-augmented AI",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("perplexity") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Hugging Face",
            description: "Open-source models",
            category: IntegrationCategory::AiModel,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "LM Studio",
            description: "Local model server",
            category: IntegrationCategory::AiModel,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Venice",
            description: "Privacy-first inference (Llama, Opus)",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("venice") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Vercel AI",
            description: "Vercel AI Gateway",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("vercel") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Cloudflare AI",
            description: "Cloudflare AI Gateway",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("cloudflare") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Moonshot",
            description: "Kimi & Kimi Coding",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref().is_some_and(is_moonshot_alias) {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Synthetic",
            description: "Synthetic AI models",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("synthetic") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "OpenCode Zen",
            description: "Code-focused AI models",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("opencode") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "OpenCode Go",
            description: "Subsidized Code-focused AI models",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("opencode-go") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Z.AI",
            description: "Z.AI inference",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref().is_some_and(is_zai_alias) {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "GLM",
            description: "ChatGLM / Zhipu models",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref().is_some_and(is_glm_alias) {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "MiniMax",
            description: "MiniMax AI models",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref().is_some_and(is_minimax_alias) {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Qwen",
            description: "Alibaba DashScope Qwen models",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref().is_some_and(is_qwen_alias) {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Amazon Bedrock",
            description: "AWS managed model access",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("bedrock") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Qianfan",
            description: "Baidu AI models",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref().is_some_and(is_qianfan_alias) {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Groq",
            description: "Ultra-fast LPU inference",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("groq") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Together AI",
            description: "Open-source model hosting",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("together") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Fireworks AI",
            description: "Fast open-source inference",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("fireworks") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Novita AI",
            description: "Affordable open-source inference",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("novita") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Cohere",
            description: "Command R+ & embeddings",
            category: IntegrationCategory::AiModel,
            status_fn: |c| {
                if c.default_provider.as_deref() == Some("cohere") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },

        IntegrationEntry {
            name: "Google Workspace",
            description: "Drive, Gmail, Calendar, Sheets, Docs via gws CLI",
            category: IntegrationCategory::Productivity,
            status_fn: |c| {
                if c.google_workspace.enabled {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "GitHub",
            description: "Code, issues, PRs",
            category: IntegrationCategory::Productivity,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Notion",
            description: "Workspace & databases",
            category: IntegrationCategory::Productivity,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Apple Notes",
            description: "Native macOS/iOS notes",
            category: IntegrationCategory::Productivity,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Apple Reminders",
            description: "Task management",
            category: IntegrationCategory::Productivity,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Obsidian",
            description: "Knowledge graph notes",
            category: IntegrationCategory::Productivity,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Things 3",
            description: "GTD task manager",
            category: IntegrationCategory::Productivity,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Bear Notes",
            description: "Markdown notes",
            category: IntegrationCategory::Productivity,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Trello",
            description: "Kanban boards",
            category: IntegrationCategory::Productivity,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Linear",
            description: "Issue tracking",
            category: IntegrationCategory::Productivity,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },

        IntegrationEntry {
            name: "Spotify",
            description: "Music playback control",
            category: IntegrationCategory::MusicAudio,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Sonos",
            description: "Multi-room audio",
            category: IntegrationCategory::MusicAudio,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Shazam",
            description: "Song recognition",
            category: IntegrationCategory::MusicAudio,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },

        IntegrationEntry {
            name: "Home Assistant",
            description: "Home automation hub",
            category: IntegrationCategory::SmartHome,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Philips Hue",
            description: "Smart lighting",
            category: IntegrationCategory::SmartHome,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "8Sleep",
            description: "Smart mattress",
            category: IntegrationCategory::SmartHome,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },

        IntegrationEntry {
            name: "Browser",
            description: "Chrome/Chromium control",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: |c| {
                if c.browser.enabled {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Shell",
            description: "Terminal command execution",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: |_| IntegrationStatus::Active,
        },
        IntegrationEntry {
            name: "File System",
            description: "Read/write files",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: |_| IntegrationStatus::Active,
        },
        IntegrationEntry {
            name: "Cron",
            description: "Scheduled tasks",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: |c| {
                if c.cron.enabled {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Voice",
            description: "Voice wake + talk mode",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Gmail",
            description: "Email triggers & send",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "1Password",
            description: "Secure credentials",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Weather",
            description: "Forecasts & conditions",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: |_| IntegrationStatus::Active,
        },
        IntegrationEntry {
            name: "Canvas",
            description: "Visual workspace + A2UI",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },

        IntegrationEntry {
            name: "Image Gen",
            description: "AI image generation",
            category: IntegrationCategory::MediaCreative,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "GIF Search",
            description: "Find the perfect GIF",
            category: IntegrationCategory::MediaCreative,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Screen Capture",
            description: "Screenshot & screen control",
            category: IntegrationCategory::MediaCreative,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Camera",
            description: "Photo/video capture",
            category: IntegrationCategory::MediaCreative,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },

        IntegrationEntry {
            name: "Twitter/X",
            description: "Tweet, reply, search",
            category: IntegrationCategory::Social,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Email",
            description: "IMAP/SMTP email channel",
            category: IntegrationCategory::Social,
            status_fn: |c| {
                if c.channels_config.email.is_some() {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },

        IntegrationEntry {
            name: "macOS",
            description: "Native support + AppleScript",
            category: IntegrationCategory::Platform,
            status_fn: |_| {
                if cfg!(target_os = "macos") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Linux",
            description: "Native support",
            category: IntegrationCategory::Platform,
            status_fn: |_| {
                if cfg!(target_os = "linux") {
                    IntegrationStatus::Active
                } else {
                    IntegrationStatus::Available
                }
            },
        },
        IntegrationEntry {
            name: "Windows",
            description: "WSL2 recommended",
            category: IntegrationCategory::Platform,
            status_fn: |_| IntegrationStatus::Available,
        },
        IntegrationEntry {
            name: "iOS",
            description: "Chat via Telegram/Discord",
            category: IntegrationCategory::Platform,
            status_fn: |_| IntegrationStatus::Available,
        },
        IntegrationEntry {
            name: "Android",
            description: "Chat via Telegram/Discord",
            category: IntegrationCategory::Platform,
            status_fn: |_| IntegrationStatus::Available,
        },
    ]
}
