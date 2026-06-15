// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[derive(Debug, Clone, Copy)]
pub struct ThemeColors {
    pub background: &'static str,
    pub surface: &'static str,
    pub text: &'static str,
    pub muted: &'static str,
    pub accent: &'static str,
    pub accent2: &'static str,
    pub hairline: &'static str,
    pub on_accent: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct DeckTheme {
    pub id: &'static str,
    pub colors: ThemeColors,
    pub heading_latin: &'static str,
    pub heading_ea: &'static str,
    pub body_latin: &'static str,
    pub body_ea: &'static str,
    pub heading_css: &'static str,
    pub body_css: &'static str,
    pub bullet: &'static str,
    pub background_gradient: Option<(&'static str, &'static str, f64)>,
}

pub const DEFAULT_THEME_ID: &str = "business-simple";

pub const THEMES: &[DeckTheme] = &[
    DeckTheme {
        id: "business-simple",
        colors: ThemeColors {
            background: "#0B1F3B",
            surface: "#13294B",
            text: "#FFFFFF",
            muted: "#C7D2E2",
            accent: "#38BDF8",
            accent2: "#E5E7EB",
            hairline: "#33486B",
            on_accent: "#0B1F3B",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "▪",
        background_gradient: None,
    },
    DeckTheme {
        id: "tech-modern",
        colors: ThemeColors {
            background: "#0B0F19",
            surface: "#141B2E",
            text: "#E2E8F0",
            muted: "#94A3B8",
            accent: "#00A3FF",
            accent2: "#7C3AED",
            hairline: "#1E293B",
            on_accent: "#06121F",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "▸",
        background_gradient: None,
    },
    DeckTheme {
        id: "academic-formal",
        colors: ThemeColors {
            background: "#F8F7F2",
            surface: "#FFFFFF",
            text: "#1F2937",
            muted: "#6B7280",
            accent: "#1E3A8A",
            accent2: "#7F1D1D",
            hairline: "#D6D3C8",
            on_accent: "#F8F7F2",
        },
        heading_latin: "Georgia",
        heading_ea: "SimSun",
        body_latin: "Georgia",
        body_ea: "SimSun",
        heading_css: "Georgia,'Times New Roman','SimSun',serif",
        body_css: "Georgia,'Times New Roman','SimSun',serif",
        bullet: "–",
        background_gradient: None,
    },
    DeckTheme {
        id: "creative-fun",
        colors: ThemeColors {
            background: "#FFD54A",
            surface: "#FFF3C9",
            text: "#1F2937",
            muted: "#4B5563",
            accent: "#FF6A00",
            accent2: "#22C55E",
            hairline: "#1F2937",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "●",
        background_gradient: None,
    },
    DeckTheme {
        id: "minimalist-clean",
        colors: ThemeColors {
            background: "#F5F5F7",
            surface: "#FFFFFF",
            text: "#111827",
            muted: "#6B7280",
            accent: "#7A8FA6",
            accent2: "#9CA3AF",
            hairline: "#E5E7EB",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "·",
        background_gradient: None,
    },
    DeckTheme {
        id: "luxury-premium",
        colors: ThemeColors {
            background: "#0B0B0F",
            surface: "#16161D",
            text: "#EDE6DA",
            muted: "#9B948A",
            accent: "#F7E7CE",
            accent2: "#C9B037",
            hairline: "#2A2A30",
            on_accent: "#0B0B0F",
        },
        heading_latin: "Georgia",
        heading_ea: "SimSun",
        body_latin: "Georgia",
        body_ea: "SimSun",
        heading_css: "Georgia,'Times New Roman','SimSun',serif",
        body_css: "Georgia,'Times New Roman','SimSun',serif",
        bullet: "◆",
        background_gradient: None,
    },
    DeckTheme {
        id: "nature-fresh",
        colors: ThemeColors {
            background: "#EAD9C6",
            surface: "#F6EFE6",
            text: "#4A4036",
            muted: "#7A6A58",
            accent: "#14532D",
            accent2: "#7A4E2D",
            hairline: "#D6C7B4",
            on_accent: "#F6EFE6",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "●",
        background_gradient: None,
    },
    DeckTheme {
        id: "gradient-vibrant",
        colors: ThemeColors {
            background: "#2563EB",
            surface: "#FFFFFF",
            text: "#FFFFFF",
            muted: "#E0E7FF",
            accent: "#FDE68A",
            accent2: "#FFFFFF",
            hairline: "#FFFFFF",
            on_accent: "#1E1B4B",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "▸",
        background_gradient: Some(("#2563EB", "#DB2777", 135.0)),
    },
    DeckTheme {
        id: "swiss-editorial",
        colors: ThemeColors {
            background: "#FAFAF8",
            surface: "#FFFFFF",
            text: "#111111",
            muted: "#9CA3AF",
            accent: "#E1251B",
            accent2: "#111111",
            hairline: "#111111",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Arial",
        heading_ea: "Microsoft YaHei",
        body_latin: "Arial",
        body_ea: "Microsoft YaHei",
        heading_css: "Helvetica,Arial,'Microsoft YaHei',sans-serif",
        body_css: "Helvetica,Arial,'Microsoft YaHei',sans-serif",
        bullet: "—",
        background_gradient: None,
    },
    DeckTheme {
        id: "dark-keynote",
        colors: ThemeColors {
            background: "#0A0A0C",
            surface: "#16161A",
            text: "#FFFFFF",
            muted: "#8E8E93",
            accent: "#0A84FF",
            accent2: "#64D2FF",
            hairline: "#2C2C2E",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "›",
        background_gradient: None,
    },
    DeckTheme {
        id: "ink-wash",
        colors: ThemeColors {
            background: "#F7F4EC",
            surface: "#FFFFFF",
            text: "#1C1A17",
            muted: "#6E675C",
            accent: "#B03A2E",
            accent2: "#3D5A6C",
            hairline: "#D8D2C4",
            on_accent: "#F7F4EC",
        },
        heading_latin: "Georgia",
        heading_ea: "KaiTi",
        body_latin: "Georgia",
        body_ea: "SimSun",
        heading_css: "Georgia,'KaiTi','STKaiti',serif",
        body_css: "Georgia,'SimSun','Songti SC',serif",
        bullet: "·",
        background_gradient: None,
    },
    DeckTheme {
        id: "china-red",
        colors: ThemeColors {
            background: "#9A1F1F",
            surface: "#AC2E2E",
            text: "#FFF6E9",
            muted: "#F2D7B6",
            accent: "#F7C873",
            accent2: "#FFE9C9",
            hairline: "#C0653F",
            on_accent: "#7A1212",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "◆",
        background_gradient: None,
    },
    DeckTheme {
        id: "magazine-editorial",
        colors: ThemeColors {
            background: "#F4F1EA",
            surface: "#FFFFFF",
            text: "#181512",
            muted: "#7A736A",
            accent: "#D35400",
            accent2: "#1F3A5F",
            hairline: "#181512",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Georgia",
        heading_ea: "SimHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "Georgia,'Times New Roman','SimHei',serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "—",
        background_gradient: None,
    },
    DeckTheme {
        id: "data-insight",
        colors: ThemeColors {
            background: "#FFFFFF",
            surface: "#F4F7FA",
            text: "#102A43",
            muted: "#627D98",
            accent: "#0CA678",
            accent2: "#1C7ED6",
            hairline: "#D9E2EC",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "▪",
        background_gradient: None,
    },
    DeckTheme {
        id: "sunset-warm",
        colors: ThemeColors {
            background: "#D9480F",
            surface: "#FFFFFF",
            text: "#FFF7ED",
            muted: "#FFD8A8",
            accent: "#FFD43B",
            accent2: "#FFC9C9",
            hairline: "#FFFFFF",
            on_accent: "#7A2E0E",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "●",
        background_gradient: Some(("#D9480F", "#9D174D", 135.0)),
    },
    DeckTheme {
        id: "mono-noir",
        colors: ThemeColors {
            background: "#FFFFFF",
            surface: "#F5F5F5",
            text: "#0A0A0A",
            muted: "#737373",
            accent: "#0A0A0A",
            accent2: "#737373",
            hairline: "#0A0A0A",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Arial",
        heading_ea: "Microsoft YaHei",
        body_latin: "Arial",
        body_ea: "Microsoft YaHei",
        heading_css: "Helvetica,Arial,'Microsoft YaHei',sans-serif",
        body_css: "Helvetica,Arial,'Microsoft YaHei',sans-serif",
        bullet: "—",
        background_gradient: None,
    },
    DeckTheme {
        id: "bento-grid",
        colors: ThemeColors {
            background: "#F2F2F7",
            surface: "#FFFFFF",
            text: "#1D1D1F",
            muted: "#86868B",
            accent: "#0A84FF",
            accent2: "#BF5AF2",
            hairline: "#E5E5EA",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'SF Pro Display','Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'SF Pro Text','Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "•",
        background_gradient: None,
    },
    DeckTheme {
        id: "neo-brutalist",
        colors: ThemeColors {
            background: "#FFFBEB",
            surface: "#FFFFFF",
            text: "#000000",
            muted: "#525252",
            accent: "#FF5C00",
            accent2: "#2563EB",
            hairline: "#000000",
            on_accent: "#000000",
        },
        heading_latin: "Arial Black",
        heading_ea: "Microsoft YaHei",
        body_latin: "Arial",
        body_ea: "Microsoft YaHei",
        heading_css: "'Arial Black','Archivo Black','Microsoft YaHei',sans-serif",
        body_css: "Arial,'Microsoft YaHei',sans-serif",
        bullet: "■",
        background_gradient: None,
    },
    DeckTheme {
        id: "crimson-report",
        colors: ThemeColors {
            background: "#FFFFFF",
            surface: "#F9F1F1",
            text: "#333333",
            muted: "#8A8A8A",
            accent: "#9B0000",
            accent2: "#54585A",
            hairline: "#E6D5D5",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "▪",
        background_gradient: None,
    },
    DeckTheme {
        id: "teal-breeze",
        colors: ThemeColors {
            background: "#FFFFFF",
            surface: "#EEF7FA",
            text: "#2F3A3F",
            muted: "#7C8A91",
            accent: "#2E8FAD",
            accent2: "#47ACC5",
            hairline: "#D7E8EE",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "●",
        background_gradient: None,
    },
    DeckTheme {
        id: "violet-haze",
        colors: ThemeColors {
            background: "#FBFAFC",
            surface: "#F1EEF5",
            text: "#322C3D",
            muted: "#847C92",
            accent: "#60546F",
            accent2: "#A99BC0",
            hairline: "#DDD7E5",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "◆",
        background_gradient: None,
    },
    DeckTheme {
        id: "morandi-duotone",
        colors: ThemeColors {
            background: "#F7F6F2",
            surface: "#FFFFFF",
            text: "#3F443C",
            muted: "#8B9088",
            accent: "#80937D",
            accent2: "#D7A89A",
            hairline: "#E2E1DA",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Segoe UI",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "·",
        background_gradient: None,
    },
    DeckTheme {
        id: "jade-serif",
        colors: ThemeColors {
            background: "#FFFFFF",
            surface: "#F0F8F3",
            text: "#2A322C",
            muted: "#6F7D74",
            accent: "#008C49",
            accent2: "#00AF57",
            hairline: "#DCE9E0",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Georgia",
        heading_ea: "SimSun",
        body_latin: "Georgia",
        body_ea: "SimSun",
        heading_css: "'Source Han Serif SC','Noto Serif SC',Georgia,'SimSun',serif",
        body_css: "'Source Han Serif SC','Noto Serif SC',Georgia,'SimSun',serif",
        bullet: "—",
        background_gradient: None,
    },
    DeckTheme {
        id: "cocoa-gold",
        colors: ThemeColors {
            background: "#F8F4EE",
            surface: "#FFFFFF",
            text: "#38261D",
            muted: "#8A7466",
            accent: "#59382A",
            accent2: "#D9A441",
            hairline: "#E6DCCD",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Georgia",
        heading_ea: "Microsoft YaHei",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "Georgia,'Times New Roman','Microsoft YaHei',serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "◆",
        background_gradient: None,
    },
    DeckTheme {
        id: "scroll-antique",
        colors: ThemeColors {
            background: "#EFE3CC",
            surface: "#F7EFDE",
            text: "#21384D",
            muted: "#7A6F5C",
            accent: "#003153",
            accent2: "#8C5A2B",
            hairline: "#D9C9A8",
            on_accent: "#F4EBD8",
        },
        heading_latin: "Georgia",
        heading_ea: "KaiTi",
        body_latin: "Georgia",
        body_ea: "KaiTi",
        heading_css: "'LXGW WenKai','KaiTi','STKaiti',serif",
        body_css: "'LXGW WenKai','KaiTi','STKaiti',serif",
        bullet: "·",
        background_gradient: None,
    },
    DeckTheme {
        id: "powder-azure",
        colors: ThemeColors {
            background: "#EAF3FC",
            surface: "#FFFFFF",
            text: "#284870",
            muted: "#6E89A8",
            accent: "#2D6CB0",
            accent2: "#94C7F1",
            hairline: "#CFE2F4",
            on_accent: "#FFFFFF",
        },
        heading_latin: "Georgia",
        heading_ea: "FangSong",
        body_latin: "Segoe UI",
        body_ea: "Microsoft YaHei",
        heading_css: "'Zhuque Fangsong','FangSong','STFangsong',serif",
        body_css: "'Segoe UI','Microsoft YaHei',sans-serif",
        bullet: "○",
        background_gradient: None,
    },
];

pub fn theme_for(id: &str) -> &'static DeckTheme {
    let needle = id.trim();
    THEMES
        .iter()
        .find(|t| t.id.eq_ignore_ascii_case(needle))
        .unwrap_or_else(|| {
            THEMES
                .iter()
                .find(|t| t.id == DEFAULT_THEME_ID)
                .expect("default deck theme present")
        })
}

pub fn is_known_theme(id: &str) -> bool {
    THEMES.iter().any(|t| t.id.eq_ignore_ascii_case(id.trim()))
}

fn normalize_hex(raw: &str) -> Option<String> {
    let hex: String = raw
        .trim()
        .trim_start_matches('#')
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    match hex.len() {
        6 => Some(format!("#{}", hex.to_ascii_uppercase())),
        3 => Some(format!(
            "#{}",
            hex.chars()
                .flat_map(|c| [c, c])
                .collect::<String>()
                .to_ascii_uppercase()
        )),
        _ => None,
    }
}

pub fn resolve_color(
    value: &str,
    theme: &DeckTheme,
    overrides: Option<&std::collections::BTreeMap<String, String>>,
) -> Option<String> {
    let token = value.trim();
    if token.is_empty() {
        return None;
    }
    let token_key = token.to_ascii_lowercase();
    let base = match token_key.as_str() {
        "background" | "bg" => Some(theme.colors.background),
        "surface" => Some(theme.colors.surface),
        "text" => Some(theme.colors.text),
        "muted" => Some(theme.colors.muted),
        "accent" => Some(theme.colors.accent),
        "accent2" => Some(theme.colors.accent2),
        "hairline" => Some(theme.colors.hairline),
        "onaccent" | "on-accent" | "on_accent" => Some(theme.colors.on_accent),
        _ => None,
    };
    if base.is_some() {
        if let Some(map) = overrides {
            for (k, v) in map {
                if k.trim().to_ascii_lowercase() == token_key {
                    if let Some(hex) = normalize_hex(v) {
                        return Some(hex);
                    }
                }
            }
        }
        return base.map(str::to_string);
    }
    normalize_hex(token)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontKind {
    Heading,
    Body,
}

#[derive(Debug, Clone, Copy)]
pub struct RolePreset {
    pub size: f64,
    pub bold: bool,
    pub font: FontKind,
    pub color_token: &'static str,
    pub line_spacing: f64,
    pub budget: usize,
}

pub fn role_preset(role: &str) -> RolePreset {
    match role.trim().to_ascii_lowercase().as_str() {
        "display" => RolePreset {
            size: 132.0,
            bold: true,
            font: FontKind::Heading,
            color_token: "text",
            line_spacing: 1.05,
            budget: 36,
        },
        "title" => RolePreset {
            size: 84.0,
            bold: true,
            font: FontKind::Heading,
            color_token: "text",
            line_spacing: 1.1,
            budget: 48,
        },
        "subtitle" => RolePreset {
            size: 44.0,
            bold: false,
            font: FontKind::Body,
            color_token: "muted",
            line_spacing: 1.3,
            budget: 90,
        },
        "heading" => RolePreset {
            size: 52.0,
            bold: true,
            font: FontKind::Heading,
            color_token: "text",
            line_spacing: 1.2,
            budget: 60,
        },
        "caption" => RolePreset {
            size: 26.0,
            bold: false,
            font: FontKind::Body,
            color_token: "muted",
            line_spacing: 1.35,
            budget: 110,
        },
        "number" => RolePreset {
            size: 140.0,
            bold: true,
            font: FontKind::Heading,
            color_token: "accent",
            line_spacing: 1.0,
            budget: 14,
        },
        "label" => RolePreset {
            size: 28.0,
            bold: true,
            font: FontKind::Body,
            color_token: "accent",
            line_spacing: 1.2,
            budget: 36,
        },
        "quote" => RolePreset {
            size: 58.0,
            bold: false,
            font: FontKind::Heading,
            color_token: "text",
            line_spacing: 1.35,
            budget: 150,
        },
        _ => RolePreset {
            size: 36.0,
            bold: false,
            font: FontKind::Body,
            color_token: "text",
            line_spacing: 1.4,
            budget: 130,
        },
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutSlot {
    pub name: &'static str,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub role: &'static str,
    pub align: &'static str,
}

const fn slot(
    name: &'static str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    role: &'static str,
    align: &'static str,
) -> LayoutSlot {
    LayoutSlot {
        name,
        x,
        y,
        w,
        h,
        role,
        align,
    }
}

pub const LAYOUT_IDS: &[&str] = &[
    "cover",
    "agenda",
    "section",
    "content",
    "two-col",
    "data",
    "quote",
    "image-full",
    "ending",
    "cards-3",
    "cards-4",
    "timeline",
    "kpi",
];

static COVER_SLOTS: [LayoutSlot; 4] = [
    slot("kicker", 120.0, 320.0, 1320.0, 56.0, "label", "left"),
    slot("title", 120.0, 408.0, 1620.0, 300.0, "display", "left"),
    slot("subtitle", 120.0, 740.0, 1380.0, 120.0, "subtitle", "left"),
    slot("meta", 120.0, 936.0, 1380.0, 56.0, "caption", "left"),
];

static AGENDA_SLOTS: [LayoutSlot; 3] = [
    slot("kicker", 120.0, 104.0, 1320.0, 48.0, "label", "left"),
    slot("title", 120.0, 168.0, 1620.0, 120.0, "title", "left"),
    slot("body", 120.0, 348.0, 1380.0, 620.0, "body", "left"),
];

static SECTION_SLOTS: [LayoutSlot; 3] = [
    slot("number", 120.0, 256.0, 720.0, 240.0, "number", "left"),
    slot("title", 120.0, 540.0, 1620.0, 200.0, "title", "left"),
    slot("subtitle", 120.0, 770.0, 1380.0, 90.0, "subtitle", "left"),
];

static TWO_COL_SLOTS: [LayoutSlot; 3] = [
    slot("title", 120.0, 96.0, 1620.0, 110.0, "title", "left"),
    slot("left", 120.0, 270.0, 815.0, 690.0, "body", "left"),
    slot("right", 985.0, 270.0, 815.0, 690.0, "body", "left"),
];

static DATA_SLOTS: [LayoutSlot; 3] = [
    slot("title", 120.0, 96.0, 1620.0, 110.0, "title", "left"),
    slot("body", 120.0, 270.0, 780.0, 690.0, "body", "left"),
    slot("visual", 960.0, 270.0, 840.0, 690.0, "body", "left"),
];

static QUOTE_SLOTS: [LayoutSlot; 2] = [
    slot("quote", 200.0, 330.0, 1520.0, 330.0, "quote", "left"),
    slot("attribution", 200.0, 720.0, 1320.0, 64.0, "caption", "left"),
];

static IMAGE_FULL_SLOTS: [LayoutSlot; 2] = [
    slot("image", 0.0, 0.0, 1920.0, 1080.0, "body", "left"),
    slot("caption", 120.0, 912.0, 1380.0, 90.0, "caption", "left"),
];

static ENDING_SLOTS: [LayoutSlot; 3] = [
    slot("title", 120.0, 400.0, 1620.0, 220.0, "display", "left"),
    slot("subtitle", 120.0, 668.0, 1380.0, 100.0, "subtitle", "left"),
    slot("meta", 120.0, 880.0, 1380.0, 56.0, "caption", "left"),
];

static CONTENT_SLOTS: [LayoutSlot; 3] = [
    slot("title", 120.0, 96.0, 1620.0, 110.0, "title", "left"),
    slot("body", 120.0, 270.0, 950.0, 690.0, "body", "left"),
    slot("visual", 1130.0, 270.0, 670.0, 690.0, "body", "left"),
];

static CARDS_3_SLOTS: [LayoutSlot; 13] = [
    slot("title", 120.0, 96.0, 1620.0, 110.0, "title", "left"),
    slot("card-1", 120.0, 280.0, 520.0, 640.0, "body", "left"),
    slot("card-2", 700.0, 280.0, 520.0, 640.0, "body", "left"),
    slot("card-3", 1280.0, 280.0, 520.0, 640.0, "body", "left"),
    slot("card-1-label", 168.0, 328.0, 424.0, 56.0, "label", "left"),
    slot("card-2-label", 748.0, 328.0, 424.0, 56.0, "label", "left"),
    slot("card-3-label", 1328.0, 328.0, 424.0, 56.0, "label", "left"),
    slot("card-1-title", 168.0, 400.0, 424.0, 96.0, "heading", "left"),
    slot("card-2-title", 748.0, 400.0, 424.0, 96.0, "heading", "left"),
    slot("card-3-title", 1328.0, 400.0, 424.0, 96.0, "heading", "left"),
    slot("card-1-body", 168.0, 520.0, 424.0, 352.0, "body", "left"),
    slot("card-2-body", 748.0, 520.0, 424.0, 352.0, "body", "left"),
    slot("card-3-body", 1328.0, 520.0, 424.0, 352.0, "body", "left"),
];

static CARDS_4_SLOTS: [LayoutSlot; 13] = [
    slot("title", 120.0, 96.0, 1620.0, 110.0, "title", "left"),
    slot("card-1", 120.0, 260.0, 810.0, 330.0, "body", "left"),
    slot("card-2", 990.0, 260.0, 810.0, 330.0, "body", "left"),
    slot("card-3", 120.0, 630.0, 810.0, 330.0, "body", "left"),
    slot("card-4", 990.0, 630.0, 810.0, 330.0, "body", "left"),
    slot("card-1-title", 168.0, 300.0, 714.0, 72.0, "heading", "left"),
    slot("card-2-title", 1038.0, 300.0, 714.0, 72.0, "heading", "left"),
    slot("card-3-title", 168.0, 670.0, 714.0, 72.0, "heading", "left"),
    slot("card-4-title", 1038.0, 670.0, 714.0, 72.0, "heading", "left"),
    slot("card-1-body", 168.0, 384.0, 714.0, 170.0, "body", "left"),
    slot("card-2-body", 1038.0, 384.0, 714.0, 170.0, "body", "left"),
    slot("card-3-body", 168.0, 754.0, 714.0, 170.0, "body", "left"),
    slot("card-4-body", 1038.0, 754.0, 714.0, 170.0, "body", "left"),
];

static TIMELINE_SLOTS: [LayoutSlot; 14] = [
    slot("title", 120.0, 96.0, 1620.0, 110.0, "title", "left"),
    slot("axis", 120.0, 556.0, 1680.0, 8.0, "body", "left"),
    slot("step-1-label", 120.0, 450.0, 380.0, 56.0, "label", "left"),
    slot("step-2-label", 553.0, 450.0, 380.0, 56.0, "label", "left"),
    slot("step-3-label", 987.0, 450.0, 380.0, 56.0, "label", "left"),
    slot("step-4-label", 1420.0, 450.0, 380.0, 56.0, "label", "left"),
    slot("step-1-title", 120.0, 612.0, 380.0, 80.0, "heading", "left"),
    slot("step-2-title", 553.0, 612.0, 380.0, 80.0, "heading", "left"),
    slot("step-3-title", 987.0, 612.0, 380.0, 80.0, "heading", "left"),
    slot("step-4-title", 1420.0, 612.0, 380.0, 80.0, "heading", "left"),
    slot("step-1-body", 120.0, 704.0, 380.0, 230.0, "body", "left"),
    slot("step-2-body", 553.0, 704.0, 380.0, 230.0, "body", "left"),
    slot("step-3-body", 987.0, 704.0, 380.0, 230.0, "body", "left"),
    slot("step-4-body", 1420.0, 704.0, 380.0, 230.0, "body", "left"),
];

static KPI_SLOTS: [LayoutSlot; 13] = [
    slot("title", 120.0, 96.0, 1620.0, 110.0, "title", "left"),
    slot("kpi-1", 120.0, 300.0, 520.0, 480.0, "body", "left"),
    slot("kpi-2", 700.0, 300.0, 520.0, 480.0, "body", "left"),
    slot("kpi-3", 1280.0, 300.0, 520.0, 480.0, "body", "left"),
    slot("kpi-1-label", 168.0, 348.0, 424.0, 56.0, "label", "left"),
    slot("kpi-2-label", 748.0, 348.0, 424.0, 56.0, "label", "left"),
    slot("kpi-3-label", 1328.0, 348.0, 424.0, 56.0, "label", "left"),
    slot("kpi-1-value", 168.0, 430.0, 424.0, 170.0, "number", "left"),
    slot("kpi-2-value", 748.0, 430.0, 424.0, 170.0, "number", "left"),
    slot("kpi-3-value", 1328.0, 430.0, 424.0, 170.0, "number", "left"),
    slot("kpi-1-caption", 168.0, 620.0, 424.0, 120.0, "caption", "left"),
    slot("kpi-2-caption", 748.0, 620.0, 424.0, 120.0, "caption", "left"),
    slot("kpi-3-caption", 1328.0, 620.0, 424.0, 120.0, "caption", "left"),
];

pub fn layout_slots(layout: &str) -> &'static [LayoutSlot] {
    match layout.trim().to_ascii_lowercase().as_str() {
        "cover" => &COVER_SLOTS,
        "agenda" => &AGENDA_SLOTS,
        "section" => &SECTION_SLOTS,
        "two-col" => &TWO_COL_SLOTS,
        "data" => &DATA_SLOTS,
        "quote" => &QUOTE_SLOTS,
        "image-full" => &IMAGE_FULL_SLOTS,
        "ending" => &ENDING_SLOTS,
        "cards-3" => &CARDS_3_SLOTS,
        "cards-4" => &CARDS_4_SLOTS,
        "timeline" => &TIMELINE_SLOTS,
        "kpi" => &KPI_SLOTS,
        _ => &CONTENT_SLOTS,
    }
}

pub fn is_known_layout(layout: &str) -> bool {
    LAYOUT_IDS
        .iter()
        .any(|l| l.eq_ignore_ascii_case(layout.trim()))
}
