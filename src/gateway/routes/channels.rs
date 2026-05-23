// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

pub fn channels_router() -> Router {
    Router::new().route("/channels", get(list_channels))
}

async fn list_channels() -> Json<Value> {
    let names = enabled_channels();
    Json(json!({ "channels": names, "count": names.len() }))
}

fn enabled_channels() -> Vec<&'static str> {
    #[allow(unused_mut)]
    let mut out: Vec<&'static str> = Vec::new();
    #[cfg(feature = "channel-telegram")]
    out.push("telegram");
    #[cfg(feature = "channel-slack")]
    out.push("slack");
    #[cfg(feature = "channel-discord")]
    out.push("discord");
    #[cfg(feature = "channel-dingtalk")]
    out.push("dingtalk");
    #[cfg(feature = "channel-wechat")]
    out.push("wechat");
    #[cfg(feature = "channel-email")]
    out.push("email");
    #[cfg(feature = "channel-line")]
    out.push("line");
    #[cfg(feature = "channel-twilio")]
    out.push("twilio");
    #[cfg(feature = "channel-matrix")]
    out.push("matrix");
    #[cfg(feature = "channel-lark")]
    out.push("lark");
    #[cfg(feature = "channel-nostr")]
    out.push("nostr");
    #[cfg(feature = "whatsapp-web")]
    out.push("whatsapp");
    out
}
