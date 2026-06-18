// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::submode::DebugSubMode;
use serde_json::{json, Value};

fn opt(value: &str, en: &str, zh: &str) -> Value {
    json!({ "value": value, "labelEn": en, "labelZh": zh })
}

fn field(key: &str, en: &str, zh: &str, kind: &str) -> Value {
    json!({ "key": key, "labelEn": en, "labelZh": zh, "type": kind })
}

fn select(key: &str, en: &str, zh: &str, options: Vec<Value>, default: &str) -> Value {
    let mut f = field(key, en, zh, "select");
    f["options"] = json!(options);
    f["default"] = json!(default);
    f
}

fn multiselect(key: &str, en: &str, zh: &str, options: Vec<Value>, default: Vec<&str>) -> Value {
    let mut f = field(key, en, zh, "multiselect");
    f["options"] = json!(options);
    f["default"] = json!(default);
    f
}

fn toggle(key: &str, en: &str, zh: &str, default: bool) -> Value {
    let mut f = field(key, en, zh, "toggle");
    f["default"] = json!(default);
    f
}

fn text(key: &str, en: &str, zh: &str, placeholder: &str) -> Value {
    let mut f = field(key, en, zh, "text");
    f["placeholderEn"] = json!(placeholder);
    f["placeholderZh"] = json!(placeholder);
    f
}

fn submode_fields(sub: DebugSubMode) -> Vec<Value> {
    match sub {
        DebugSubMode::Auto => vec![],
        DebugSubMode::CodeReview => vec![
            select(
                "scope",
                "Scope",
                "审查范围",
                vec![
                    opt("working", "Working changes", "工作区改动"),
                    opt("staged", "Staged changes", "暂存区改动"),
                    opt("branch", "Branch vs main", "分支对比主干"),
                    opt("path", "Referenced paths", "引用的路径"),
                ],
                "working",
            ),
            select(
                "personas",
                "Review style",
                "审查风格",
                vec![
                    opt("standard", "Tech lead", "技术负责人"),
                    opt("adversarial", "Multi-persona", "多角色对抗"),
                ],
                "standard",
            ),
            select(
                "depth",
                "Depth",
                "深度",
                vec![
                    opt("quick", "Quick", "快速"),
                    opt("standard", "Standard", "标准"),
                    opt("deep", "Deep", "深入"),
                ],
                "standard",
            ),
        ],
        DebugSubMode::SecurityReview => vec![
            multiselect(
                "frameworks",
                "Frameworks",
                "审计框架",
                vec![
                    opt("owasp", "OWASP Top 10", "OWASP Top 10"),
                    opt("stride", "STRIDE", "STRIDE"),
                    opt("secrets", "Secret scan", "密钥扫描"),
                ],
                vec!["owasp", "secrets"],
            ),
            select(
                "scope",
                "Scope",
                "审计范围",
                vec![
                    opt("changes", "Changes only", "仅改动"),
                    opt("project", "Whole project", "整个项目"),
                ],
                "changes",
            ),
            toggle("includeDeps", "Audit dependencies", "审计依赖", false),
        ],
        DebugSubMode::E2e => vec![
            select(
                "framework",
                "Test framework",
                "测试框架",
                vec![
                    opt("auto", "Auto-detect", "自动识别"),
                    opt("playwright", "Playwright", "Playwright"),
                    opt("cypress", "Cypress", "Cypress"),
                    opt("manual", "Browser dock only", "仅内置浏览器"),
                ],
                "auto",
            ),
            multiselect(
                "devices",
                "Viewports",
                "视口",
                vec![
                    opt("desktop", "Desktop", "桌面"),
                    opt("mobile", "Mobile", "移动端"),
                    opt("tablet", "Tablet", "平板"),
                ],
                vec!["desktop"],
            ),
            select(
                "depth",
                "Coverage",
                "覆盖深度",
                vec![
                    opt("smoke", "Smoke", "冒烟"),
                    opt("core", "Core flows", "核心流程"),
                    opt("full", "Full matrix", "完整矩阵"),
                ],
                "core",
            ),
            toggle("generateTests", "Generate test files", "生成测试文件", true),
            text("baseUrl", "Base URL", "基础地址", "https://app.example.com"),
        ],
        DebugSubMode::Performance => vec![
            select(
                "tool",
                "Tool",
                "工具",
                vec![
                    opt("auto", "Auto-detect", "自动识别"),
                    opt("k6", "k6 (load)", "k6(负载)"),
                    opt("lighthouse", "Lighthouse", "Lighthouse"),
                    opt("browser", "Browser vitals", "浏览器指标"),
                ],
                "auto",
            ),
            select(
                "profile",
                "Profile",
                "压测档位",
                vec![
                    opt("smoke", "Smoke", "冒烟"),
                    opt("load", "Load", "负载"),
                    opt("stress", "Stress", "压力"),
                    opt("soak", "Soak", "稳定性"),
                ],
                "load",
            ),
            {
                let mut f = field("vus", "Virtual users", "虚拟用户", "number");
                f["default"] = json!(50);
                f["min"] = json!(1);
                f["max"] = json!(5000);
                f
            },
            text("duration", "Duration", "持续时间", "30s / 5m"),
            text("p95Threshold", "p95 threshold (ms)", "p95 阈值(ms)", "500"),
            text("targetUrl", "Target URL/API", "目标地址/API", "https://api.example.com/..."),
        ],
    }
}

pub fn submode_param_schema(sub: DebugSubMode) -> Value {
    json!({
        "id": sub.id(),
        "labelEn": sub.label_en(),
        "labelZh": sub.label_zh(),
        "icon": sub.icon(),
        "mayWrite": sub.may_write(),
        "fields": submode_fields(sub),
    })
}

pub fn all_submode_schemas() -> Value {
    let modes: Vec<Value> = DebugSubMode::all()
        .iter()
        .map(|m| submode_param_schema(*m))
        .collect();
    json!({ "submodes": modes })
}

pub fn render_params_prompt(sub: DebugSubMode, params: &Value) -> String {
    let schema = submode_param_schema(sub);
    let mut lines: Vec<String> = Vec::new();
    if let Some(fields) = schema.get("fields").and_then(|f| f.as_array()) {
        for f in fields {
            let key = f.get("key").and_then(|v| v.as_str()).unwrap_or_default();
            let label = f.get("labelEn").and_then(|v| v.as_str()).unwrap_or(key);
            let rendered = match params.get(key) {
                Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
                Some(Value::Bool(b)) => Some(if *b { "yes".into() } else { "no".into() }),
                Some(Value::Number(n)) => Some(n.to_string()),
                Some(Value::Array(arr)) if !arr.is_empty() => {
                    let joined: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect();
                    if joined.is_empty() {
                        None
                    } else {
                        Some(joined.join(", "))
                    }
                }
                _ => None,
            };
            if let Some(val) = rendered {
                lines.push(format!("- {label}: {val}"));
            }
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    format!("### Sub-mode parameters\n{}", lines.join("\n"))
}
