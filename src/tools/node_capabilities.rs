// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde_json::json;

pub struct NodeCapabilityDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: serde_json::Value,
    pub risk_level: RiskLevel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

pub fn camera_capabilities() -> Vec<NodeCapabilityDef> {
    vec![
        NodeCapabilityDef {
            name: "camera.snap",
            description: "Capture a photo from the device camera",
            parameters: json!({
                "type": "object",
                "properties": {
                    "camera": { "type": "string", "enum": ["front", "back"], "default": "back" },
                    "quality": { "type": "string", "enum": ["low", "medium", "high"], "default": "medium" },
                    "approved": { "type": "boolean", "description": "Set to true to approve camera access" }
                },
                "required": ["approved"]
            }),
            risk_level: RiskLevel::High,
        },
        NodeCapabilityDef {
            name: "camera.clip",
            description: "Record a short video clip from the device camera",
            parameters: json!({
                "type": "object",
                "properties": {
                    "camera": { "type": "string", "enum": ["front", "back"], "default": "back" },
                    "duration_secs": { "type": "integer", "minimum": 1, "maximum": 30, "default": 5 },
                    "quality": { "type": "string", "enum": ["low", "medium", "high"], "default": "medium" },
                    "approved": { "type": "boolean", "description": "Set to true to approve camera access" }
                },
                "required": ["approved"]
            }),
            risk_level: RiskLevel::High,
        },
    ]
}

pub fn screen_capabilities() -> Vec<NodeCapabilityDef> {
    vec![
        NodeCapabilityDef {
            name: "screen.capture",
            description: "Capture a screenshot of the device screen",
            parameters: json!({
                "type": "object",
                "properties": {
                    "display": { "type": "integer", "default": 0, "description": "Display index for multi-monitor setups" },
                    "approved": { "type": "boolean", "description": "Set to true to approve screen capture" }
                },
                "required": ["approved"]
            }),
            risk_level: RiskLevel::High,
        },
        NodeCapabilityDef {
            name: "screen.record",
            description: "Record the device screen for a specified duration",
            parameters: json!({
                "type": "object",
                "properties": {
                    "duration_secs": { "type": "integer", "minimum": 1, "maximum": 60, "default": 10 },
                    "display": { "type": "integer", "default": 0 },
                    "approved": { "type": "boolean", "description": "Set to true to approve screen recording" }
                },
                "required": ["approved"]
            }),
            risk_level: RiskLevel::High,
        },
    ]
}

pub fn location_capabilities() -> Vec<NodeCapabilityDef> {
    vec![NodeCapabilityDef {
        name: "location.get",
        description: "Get the current GPS location of the device",
        parameters: json!({
            "type": "object",
            "properties": {
                "accuracy": { "type": "string", "enum": ["coarse", "fine"], "default": "coarse" },
                "approved": { "type": "boolean", "description": "Set to true to approve location access" }
            },
            "required": ["approved"]
        }),
        risk_level: RiskLevel::High,
    }]
}

pub fn notification_capabilities() -> Vec<NodeCapabilityDef> {
    vec![NodeCapabilityDef {
        name: "system.notify",
        description: "Send a system notification to the device",
        parameters: json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Notification title" },
                "body": { "type": "string", "description": "Notification body text" },
                "priority": { "type": "string", "enum": ["low", "normal", "high"], "default": "normal" }
            },
            "required": ["title", "body"]
        }),
        risk_level: RiskLevel::Low,
    }]
}

pub fn all_standard_capabilities() -> Vec<NodeCapabilityDef> {
    let mut caps = Vec::new();
    caps.extend(camera_capabilities());
    caps.extend(screen_capabilities());
    caps.extend(location_capabilities());
    caps.extend(notification_capabilities());
    caps
}

pub fn requires_approval(capability_name: &str) -> bool {
    let sensitive_prefixes = ["camera.", "screen.", "location."];
    sensitive_prefixes
        .iter()
        .any(|p| capability_name.starts_with(p))
}

pub fn detect_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(target_os = "android")]
    {
        "android"
    }
    #[cfg(target_os = "ios")]
    {
        "ios"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "android",
        target_os = "ios",
        target_os = "windows"
    )))]
    {
        "unknown"
    }
}
