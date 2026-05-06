//! sen-channels — Messaging channel integrations.
//!
//! Provides adapters for Telegram, Discord, Slack, Matrix, Lark, and more.
//! Enable specific channel features to include only the integrations you need.
//!
//! # Feature flags
//! - `telegram` — Telegram Bot API adapter
//! - `discord` — Discord Gateway adapter
//! - `slack` — Slack Events API adapter
//! - `matrix` — Matrix (Element) adapter with E2EE
//! - `lark` / `feishu` — Lark/Feishu adapter
//! - `nostr` — Nostr decentralised social protocol adapter
//! - `email` — SMTP/IMAP email adapter
//! - `whatsapp-web` — WhatsApp Web native adapter
//! - `dingtalk` — DingTalk adapter
//! - `wechat` — WeChat adapter
//! - `line` — LINE adapter
//! - `all` — enables every channel above

pub use senweavercoding::channels;
