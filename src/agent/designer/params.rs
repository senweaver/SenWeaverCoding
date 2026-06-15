// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::submode::DesignerSubMode;
use serde_json::{json, Value};

pub const MEDIA_ASPECTS: &[&str] = &["1:1", "16:9", "9:16", "4:3", "3:4"];
pub const VIDEO_LENGTHS_SEC: &[u32] = &[3, 5, 8, 10, 15, 30];
pub const AUDIO_DURATIONS_SEC: &[u32] = &[5, 10, 15, 30, 60, 120];

fn opt(value: &str, en: &str, zh: &str) -> Value {
    json!({ "value": value, "labelEn": en, "labelZh": zh })
}

fn field(key: &str, en: &str, zh: &str, kind: &str) -> Value {
    json!({ "key": key, "labelEn": en, "labelZh": zh, "type": kind })
}

fn design_system_field() -> Value {
    let mut f = field("designSystem", "Design system", "设计体系", "designSystem");
    f["default"] = json!("default");
    f
}

fn toggle_field(key: &str, en: &str, zh: &str) -> Value {
    let mut f = field(key, en, zh, "toggle");
    f["default"] = json!(false);
    f
}

fn resolution_field() -> Value {
    let mut f = field("resolution", "Resolution", "分辨率", "select");
    f["options"] = json!([opt("2k", "2K", "2K"), opt("4k", "4K", "4K")]);
    f["default"] = json!("2k");
    f
}

fn prompt_template_field(surface: &str) -> Value {
    let mut f = field("promptTemplate", "Prompt template", "提示模板", "promptTemplate");
    f["surface"] = json!(surface);
    f["default"] = json!("");
    f
}

fn model_field(surface: crate::tools::media::MediaSurface, default: &str) -> Value {
    let mut options = vec![opt("auto", "Auto (provider default)", "自动(默认模型)")];
    if let Some(models) = crate::tools::media::registry::default_models(surface).as_array() {
        for m in models {
            let id = m.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            let label = m.get("label").and_then(|v| v.as_str()).unwrap_or(id);
            if !id.is_empty() {
                options.push(opt(id, label, label));
            }
        }
    }
    let mut f = field("model", "Model", "模型", "select");
    f["options"] = json!(options);
    f["default"] = json!(default);
    f
}

fn aspect_options() -> Vec<Value> {
    MEDIA_ASPECTS
        .iter()
        .map(|a| opt(a, a, a))
        .collect()
}

fn seconds_options(values: &[u32]) -> Vec<Value> {
    values
        .iter()
        .map(|s| opt(&s.to_string(), &format!("{s}s"), &format!("{s}秒")))
        .collect()
}

pub fn submode_param_schema(sub: DesignerSubMode) -> Value {
    let fields: Vec<Value> = match sub {
        DesignerSubMode::Prototype => vec![
            design_system_field(),
            {
                let mut f = field("platform", "Platform", "应用平台", "select");
                f["options"] = json!([
                    opt("web", "Web", "网页"),
                    opt("mobile", "Mobile", "移动端"),
                    opt("desktop", "Desktop", "桌面"),
                    opt("tablet", "Tablet", "平板"),
                ]);
                f["default"] = json!("web");
                f
            },
            {
                let mut f = field("fidelity", "Fidelity", "保真度", "select");
                f["options"] = json!([
                    opt("high-fidelity", "High fidelity", "高保真"),
                    opt("wireframe", "Wireframe", "线框图"),
                ]);
                f["default"] = json!("high-fidelity");
                f
            },
        ],
        DesignerSubMode::LiveArtifact => vec![
            design_system_field(),
            {
                let mut f = field("biStyle", "Visual style", "视觉风格", "select");
                let mut options = vec![opt("auto", "Auto (match topic)", "智能匹配")];
                options.extend(
                    super::bi_styles::BI_STYLES
                        .iter()
                        .map(|s| opt(s.id, s.name_en, s.name_zh)),
                );
                f["options"] = json!(options);
                f["default"] = json!("auto");
                f
            },
            {
                let mut tpl = field("htmlTemplate", "Template", "模板", "htmlTemplate");
                tpl["default"] = json!("");
                tpl
            },
            {
                let mut f = field("platform", "Platform", "应用平台", "select");
                f["options"] = json!([
                    opt("web", "Web", "网页"),
                    opt("mobile", "Mobile", "移动端"),
                    opt("desktop", "Desktop", "桌面"),
                    opt("tablet", "Tablet", "平板"),
                ]);
                f["default"] = json!("web");
                f
            },
            {
                let mut f = field("refresh", "Refresh", "刷新方式", "select");
                f["options"] = json!([
                    opt("manual", "Manual", "手动"),
                    opt("on-load", "On load", "加载时"),
                    opt("interval", "Interval", "定时"),
                ]);
                f["default"] = json!("manual");
                f
            },
            {
                let mut f = field("refreshInterval", "Interval", "刷新间隔", "select");
                f["options"] = json!([
                    opt("5", "5s", "5秒"),
                    opt("15", "15s", "15秒"),
                    opt("30", "30s", "30秒"),
                    opt("60", "60s", "60秒"),
                    opt("300", "5min", "5分钟"),
                ]);
                f["default"] = json!("30");
                f
            },
        ],
        DesignerSubMode::Deck => vec![
            design_system_field(),
            {
                let mut f = field("deckStyle", "Visual style", "视觉风格", "select");
                let mut options = vec![opt("auto", "Auto (match topic)", "智能匹配")];
                options.extend(
                    super::deck::styles::DECK_STYLES
                        .iter()
                        .map(|s| opt(s.id, s.name_en, s.name_zh)),
                );
                f["options"] = json!(options);
                f["default"] = json!("auto");
                f
            },
            {
                let mut f = field("deckType", "Narrative", "叙事类型", "select");
                f["options"] = json!([
                    opt("pitch", "Pitch", "融资路演"),
                    opt("product", "Product launch", "产品发布"),
                    opt("study", "Case study", "案例研究"),
                    opt("strategy", "Strategy", "战略汇报"),
                    opt("sales", "Sales", "销售提案"),
                    opt("report", "Data report", "数据分析报告"),
                    opt("training", "Training", "培训课程"),
                    opt("academic", "Academic", "学术答辩"),
                    opt("review", "Retrospective", "复盘总结"),
                    opt("allhands", "All-hands", "全员大会"),
                    opt("keynote", "Keynote", "主题演讲"),
                    opt("portfolio", "Portfolio", "个人作品集"),
                ]);
                f["default"] = json!("pitch");
                f
            },
            {
                let mut f = field("slideCount", "Slides", "页数", "select");
                f["options"] = json!([
                    opt("5-10 pages", "5-10 pages", "5-10 页"),
                    opt("10-15 pages", "10-15 pages", "10-15 页"),
                    opt("15-20 pages", "15-20 pages", "15-20 页"),
                    opt("20-30 pages", "20-30 pages", "20-30 页"),
                ]);
                f["default"] = json!("10-15 pages");
                f
            },
            {
                let mut f = field("deckAspect", "Aspect", "画幅", "select");
                f["options"] = json!([
                    opt("16:9", "16:9 widescreen", "16:9 宽屏"),
                    opt("4:3", "4:3 classic", "4:3 经典"),
                ]);
                f["default"] = json!("16:9");
                f
            },
            {
                let mut f = field("contentDensity", "Density", "信息密度", "select");
                f["options"] = json!([
                    opt("minimal", "Minimal (keynote)", "极简(演讲型)"),
                    opt("balanced", "Balanced", "均衡"),
                    opt("detailed", "Detailed (report)", "详实(报告型)"),
                ]);
                f["default"] = json!("balanced");
                f
            },
            {
                let mut f = field("transition", "Transitions", "转场动效", "select");
                f["options"] = json!([
                    opt("none", "None", "无"),
                    opt("subtle", "Subtle", "轻盈"),
                    opt("cinematic", "Cinematic", "影院级"),
                ]);
                f["default"] = json!("subtle");
                f
            },
            {
                let mut f = field("aiImagery", "AI imagery", "AI 配图", "select");
                f["options"] = json!([
                    opt("auto", "Auto (when available)", "自动(可用即用)"),
                    opt("rich", "Rich", "丰富"),
                    opt("none", "None (pure CSS/SVG)", "无(纯CSS/SVG)"),
                ]);
                f["default"] = json!("auto");
                f
            },
            toggle_field("speakerNotes", "Speaker notes", "演讲备注"),
        ],
        DesignerSubMode::Diagram => vec![
            design_system_field(),
            {
                let mut f = field("engine", "Engine", "图表引擎", "select");
                f["options"] = json!([
                    opt("auto", "Auto (match intent)", "智能匹配"),
                    opt("mermaid", "Mermaid (flow/sequence/UML...)", "Mermaid (流程/时序/UML...)"),
                    opt("echarts", "ECharts (data charts)", "ECharts (数据图表)"),
                    opt("mindmap", "Mind map", "思维导图"),
                ]);
                f["default"] = json!("auto");
                f
            },
            {
                let mut f = field("diagramType", "Diagram type", "图表类型", "select");
                f["options"] = json!([
                    opt("auto", "Auto", "自动"),
                    opt("flowchart", "Flowchart", "流程图"),
                    opt("sequence", "Sequence", "时序图"),
                    opt("class", "Class (UML)", "类图 (UML)"),
                    opt("state", "State machine", "状态图"),
                    opt("er", "Entity relation", "ER 图"),
                    opt("gantt", "Gantt", "甘特图"),
                    opt("timeline", "Timeline", "时间线"),
                    opt("journey", "User journey", "用户旅程"),
                    opt("quadrant", "Quadrant", "四象限"),
                    opt("architecture", "Architecture", "架构图"),
                    opt("mindmap", "Mind map", "思维导图"),
                    opt("bar", "Bar chart", "柱状图"),
                    opt("line", "Line chart", "折线图"),
                    opt("pie", "Pie chart", "饼图"),
                    opt("scatter", "Scatter", "散点图"),
                    opt("radar", "Radar", "雷达图"),
                    opt("heatmap", "Heatmap", "热力图"),
                    opt("sunburst", "Sunburst", "旭日图"),
                    opt("funnel", "Funnel", "漏斗图"),
                    opt("gauge", "Gauge", "仪表盘"),
                    opt("sankey", "Sankey", "桑基图"),
                    opt("tree", "Tree", "树图"),
                    opt("graph", "Relation graph", "关系图"),
                ]);
                f["default"] = json!("auto");
                f
            },
            {
                let mut f = field("direction", "Direction", "布局方向", "select");
                f["options"] = json!([
                    opt("auto", "Auto", "自动"),
                    opt("TB", "Top-down", "纵向"),
                    opt("LR", "Left-right", "横向"),
                ]);
                f["default"] = json!("auto");
                f
            },
            {
                let mut f = field("theme", "Theme", "主题", "select");
                f["options"] = json!([
                    opt("default", "Default", "默认"),
                    opt("dark", "Dark", "暗色"),
                    opt("forest", "Forest", "森林"),
                    opt("neutral", "Neutral", "中性"),
                ]);
                f["default"] = json!("default");
                f
            },
            {
                let mut f = field("chartPalette", "Palette", "图表色板", "select");
                let mut options = vec![opt("auto", "Auto (match topic)", "智能匹配")];
                options.extend(
                    super::chart_palettes::CHART_PALETTES
                        .iter()
                        .map(|p| opt(p.id, p.name_en, p.name_zh)),
                );
                f["options"] = json!(options);
                f["default"] = json!("auto");
                f
            },
            {
                let mut f = field("detail", "Detail level", "细节密度", "select");
                f["options"] = json!([
                    opt("simple", "Simple (overview)", "简洁(概览)"),
                    opt("balanced", "Balanced", "均衡"),
                    opt("detailed", "Detailed (exhaustive)", "详尽(完整)"),
                ]);
                f["default"] = json!("balanced");
                f
            },
        ],
        DesignerSubMode::Image => vec![
            design_system_field(),
            prompt_template_field("image"),
            model_field(crate::tools::media::MediaSurface::Image, "auto"),
            {
                let mut f = field("aspect", "Ratio", "比例", "select");
                f["options"] = json!(aspect_options());
                f["default"] = json!("16:9");
                f
            },
            resolution_field(),
            {
                let mut f = field("count", "Count", "数量", "select");
                f["options"] = json!([
                    opt("1", "1", "1"),
                    opt("2", "2", "2"),
                    opt("3", "3", "3"),
                    opt("4", "4", "4"),
                ]);
                f["default"] = json!("1");
                f
            },
        ],
        DesignerSubMode::Video => vec![
            design_system_field(),
            prompt_template_field("video"),
            model_field(crate::tools::media::MediaSurface::Video, "auto"),
            {
                let mut f = field("aspect", "Ratio", "比例", "select");
                f["options"] = json!(aspect_options());
                f["default"] = json!("16:9");
                f
            },
            {
                let mut f = field("length", "Duration", "时长", "select");
                f["options"] = json!(seconds_options(VIDEO_LENGTHS_SEC));
                f["default"] = json!("5");
                f
            },
        ],
        DesignerSubMode::HyperFrames => vec![
            design_system_field(),
            {
                let mut f = field("aspect", "Ratio", "比例", "select");
                f["options"] = json!(aspect_options());
                f["default"] = json!("16:9");
                f
            },
            {
                let mut f = field("length", "Duration", "时长", "select");
                f["options"] = json!(seconds_options(VIDEO_LENGTHS_SEC));
                f["default"] = json!("10");
                f
            },
        ],
        DesignerSubMode::Audio => vec![
            model_field(crate::tools::media::MediaSurface::Audio, "auto"),
            {
                let mut f = field("audioKind", "Audio type", "音频类型", "select");
                f["options"] = json!([
                    opt("speech", "Speech", "语音"),
                    opt("sfx", "Sound effect", "音效"),
                    opt("music", "Music", "音乐"),
                ]);
                f["default"] = json!("speech");
                f
            },
            {
                let mut f = field("duration", "Duration", "时长", "select");
                f["options"] = json!(seconds_options(AUDIO_DURATIONS_SEC));
                f["default"] = json!("10");
                f
            },
        ],
        DesignerSubMode::FromFigma => vec![
            {
                let mut f = field("figmaUrl", "Figma URL", "Figma 链接", "text");
                f["required"] = json!(true);
                f["placeholderEn"] = json!("https://www.figma.com/file/...");
                f["placeholderZh"] = json!("https://www.figma.com/file/...");
                f
            },
            {
                let mut f = field("frameName", "Frame to extract", "提取的 Frame", "text");
                f["placeholderEn"] = json!("Dashboard / Card / Active sessions");
                f["placeholderZh"] = json!("Dashboard / Card / Active sessions");
                f
            },
        ],
        DesignerSubMode::FromTemplate => vec![
            {
                let mut tpl = field("htmlTemplate", "Template", "模板", "htmlTemplate");
                tpl["default"] = json!("");
                tpl
            },
            design_system_field(),
            {
                let mut f = field("platform", "Platform", "应用平台", "select");
                f["options"] = json!([
                    opt("web", "Web", "网页"),
                    opt("mobile", "Mobile", "移动端"),
                    opt("desktop", "Desktop", "桌面"),
                    opt("tablet", "Tablet", "平板"),
                ]);
                f["default"] = json!("web");
                f
            },
        ],
    };

    json!({
        "id": sub.id(),
        "labelEn": sub.label_en(),
        "labelZh": sub.label_zh(),
        "icon": sub.icon(),
        "surface": sub.media_surface(),
        "fields": fields,
    })
}

pub fn all_submode_schemas() -> Value {
    let modes: Vec<Value> = DesignerSubMode::all()
        .iter()
        .map(|m| submode_param_schema(*m))
        .collect();
    json!({ "submodes": modes })
}

pub fn render_params_prompt(sub: DesignerSubMode, params: &Value) -> String {
    let schema = submode_param_schema(sub);
    let mut lines: Vec<String> = Vec::new();
    if let Some(fields) = schema.get("fields").and_then(|f| f.as_array()) {
        for field in fields {
            let key = field.get("key").and_then(|v| v.as_str()).unwrap_or_default();
            let label = field.get("labelEn").and_then(|v| v.as_str()).unwrap_or(key);
            let value = params.get(key);
            let rendered = match value {
                Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
                Some(Value::Bool(b)) => Some(if *b { "yes".to_string() } else { "no".to_string() }),
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
                let display = if key == "designSystem" {
                    super::design_system::name_for(&val)
                        .map(|name| format!("{name} (id: `{val}` — use this exact id for `design_system_read`)"))
                        .unwrap_or(val)
                } else if key == "deckStyle" {
                    super::deck::styles::deck_style_name_en(&val)
                        .map(|name| format!("{name} (theme id: `{val}`)"))
                        .unwrap_or(val)
                } else if key == "biStyle" {
                    super::bi_styles::bi_style_name_en(&val)
                        .map(|name| format!("{name} (style id: `{val}`)"))
                        .unwrap_or(val)
                } else if key == "chartPalette" {
                    super::chart_palettes::palette_name_en(&val)
                        .map(|name| format!("{name} (palette id: `{val}`)"))
                        .unwrap_or(val)
                } else if key == "promptTemplate" {
                    sub.media_surface()
                        .and_then(|surface| super::prompt_template::title_for(surface, &val))
                        .map(|title| format!("{title} (id: `{val}`)"))
                        .unwrap_or(val)
                } else if key == "htmlTemplate" {
                    super::html_template::title_for(&val)
                        .map(|title| format!("{title} (id: `{val}`)"))
                        .unwrap_or(val)
                } else {
                    val
                };
                lines.push(format!("- {label}: {display}"));
            }
        }
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("Selected parameters:\n{}", lines.join("\n"))
    }
}

pub fn selected_prompt_template_block(sub: DesignerSubMode, params: &Value) -> Option<String> {
    let surface = sub.media_surface()?;
    let id = params
        .get("promptTemplate")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let detail = super::prompt_template::read(surface, id)?;
    let mut out = format!(
        "\n\n## Reference prompt template — {} ({})\n\n\
         The user picked this curated {} prompt template. Treat the block below as the authoritative \
         prompt seed: adapt it to the user's brief (subject, brand, copy) while preserving its \
         structure, motion/scene beats, and technical contract. Do not discard its requirements.",
        detail.meta.title, detail.meta.id, surface,
    );
    if let Some(model) = detail.meta.model.as_deref() {
        out.push_str(&format!("\nSuggested model: {model}"));
    }
    if let Some(aspect) = detail.meta.aspect.as_deref() {
        out.push_str(&format!("\nSuggested aspect: {aspect}"));
    }
    out.push_str(&format!("\n\n{}", detail.prompt));
    Some(out)
}

pub fn selected_html_template_block(sub: DesignerSubMode, params: &Value) -> Option<String> {
    if !matches!(
        sub,
        DesignerSubMode::FromTemplate | DesignerSubMode::LiveArtifact
    ) {
        return None;
    }
    let id = params
        .get("htmlTemplate")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    super::html_template::injection(id)
}
