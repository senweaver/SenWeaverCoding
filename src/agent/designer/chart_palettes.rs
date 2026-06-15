// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub struct ChartPalette {
    pub id: &'static str,
    pub name_en: &'static str,
    pub name_zh: &'static str,
    pub kind: &'static str,
    pub colors: &'static [&'static str],
}

pub const CHART_PALETTES: &[ChartPalette] = &[
    ChartPalette {
        id: "ocean-product-20",
        name_en: "Ocean product 20",
        name_zh: "海洋产品 20 色",
        kind: "categorical",
        colors: &[
            "#1FA8C9", "#454E7C", "#5AC189", "#FF7F44", "#666666", "#E04355", "#FCC700",
            "#A868B7", "#3CCCCB", "#A38F79", "#8FD3E4", "#A1A6BD", "#ACE1C4", "#FEC0A1",
            "#B2B2B2", "#EFA1AA", "#FDE380", "#D3B3DA", "#9EE5E5", "#D1C6BC",
        ],
    },
    ChartPalette {
        id: "modern-sunset-12",
        name_en: "Modern sunset 12",
        name_zh: "现代日落 12 色",
        kind: "categorical",
        colors: &[
            "#0080F6", "#254081", "#6C4592", "#A94693", "#DC4180", "#F35193", "#FF7582",
            "#FF4C5D", "#FF824E", "#FFAD2A", "#FFDB04", "#F3F700",
        ],
    },
    ChartPalette {
        id: "indigo-spectrum-10",
        name_en: "Indigo spectrum 10",
        name_zh: "靛蓝光谱 10 色",
        kind: "categorical",
        colors: &[
            "#7763CF", "#444CE7", "#1570EF", "#0086C9", "#3E4784", "#E31B54", "#EC4A0A",
            "#EF8D0C", "#EBC405", "#5381AD",
        ],
    },
    ChartPalette {
        id: "harmonic-9",
        name_en: "Harmonic 9",
        name_zh: "和谐 9 色",
        kind: "categorical",
        colors: &[
            "#5470C6", "#91CC75", "#FAC858", "#EE6666", "#73C0DE", "#3BA272", "#FC8452",
            "#9A60B4", "#EA7CCC",
        ],
    },
    ChartPalette {
        id: "classic-10",
        name_en: "Classic 10",
        name_zh: "经典 10 色",
        kind: "categorical",
        colors: &[
            "#1f77b4", "#ff7f0e", "#2ca02c", "#d62728", "#9467bd", "#8c564b", "#e377c2",
            "#7f7f7f", "#bcbd22", "#17becf",
        ],
    },
    ChartPalette {
        id: "classic-20",
        name_en: "Classic 20 (paired)",
        name_zh: "经典 20 色（深浅成对）",
        kind: "categorical",
        colors: &[
            "#1f77b4", "#aec7e8", "#ff7f0e", "#ffbb78", "#2ca02c", "#98df8a", "#d62728",
            "#ff9896", "#9467bd", "#c5b0d5", "#8c564b", "#c49c94", "#e377c2", "#f7b6d2",
            "#7f7f7f", "#c7c7c7", "#bcbd22", "#dbdb8d", "#17becf", "#9edae5",
        ],
    },
    ChartPalette {
        id: "vivid-10",
        name_en: "Vivid 10",
        name_zh: "鲜明 10 色",
        kind: "categorical",
        colors: &[
            "#3366cc", "#dc3912", "#ff9900", "#109618", "#990099", "#0099c6", "#dd4477",
            "#66aa00", "#b82e2e", "#316395",
        ],
    },
    ChartPalette {
        id: "vivid-20",
        name_en: "Vivid 20",
        name_zh: "鲜明 20 色",
        kind: "categorical",
        colors: &[
            "#3366cc", "#dc3912", "#ff9900", "#109618", "#990099", "#0099c6", "#dd4477",
            "#66aa00", "#b82e2e", "#316395", "#994499", "#22aa99", "#aaaa11", "#6633cc",
            "#e67300", "#8b0707", "#651067", "#329262", "#5574a6", "#3b3eac",
        ],
    },
    ChartPalette {
        id: "coral-resort-12",
        name_en: "Coral resort 12",
        name_zh: "珊瑚度假 12 色",
        kind: "categorical",
        colors: &[
            "#29696B", "#5BCACE", "#F4B02A", "#F1826A", "#792EB2", "#C96EC6", "#921E50",
            "#B27700", "#9C3498", "#E4679D", "#C32F0E", "#9D63CA",
        ],
    },
    ChartPalette {
        id: "magenta-pop-10",
        name_en: "Magenta pop 10",
        name_zh: "品红流行 10 色",
        kind: "categorical",
        colors: &[
            "#EA0B8C", "#6C838E", "#29ABE2", "#33D9C1", "#9DACB9", "#7560AA", "#2D5584",
            "#831C4A", "#333D47", "#AC2077",
        ],
    },
    ChartPalette {
        id: "teal-depth-seq",
        name_en: "Teal depth (sequential)",
        name_zh: "青蓝深度（连续）",
        kind: "sequential",
        colors: &[
            "#F4FAD4", "#D7F1AC", "#A9E3AF", "#82CDBB", "#63C1BF", "#1FA8C9", "#2367AC",
            "#2A2D84", "#251354", "#050415",
        ],
    },
    ChartPalette {
        id: "sunset-heat-seq",
        name_en: "Sunset heat (sequential)",
        name_zh: "日落热度（连续）",
        kind: "sequential",
        colors: &[
            "#FBF1B4", "#FDD093", "#FEAD71", "#FF7F44", "#E04355", "#C53D6F", "#952B7B",
            "#4F167B", "#251354", "#050415",
        ],
    },
    ChartPalette {
        id: "viridis-seq",
        name_en: "Viridis (sequential)",
        name_zh: "翠光谱（连续）",
        kind: "sequential",
        colors: &[
            "#482475", "#414487", "#355f8d", "#2a788e", "#21918c", "#22a884", "#44bf70",
            "#7ad151", "#bddf26", "#fde725",
        ],
    },
    ChartPalette {
        id: "blue-depth-seq",
        name_en: "Blue depth (sequential)",
        name_zh: "蓝色深度（连续）",
        kind: "sequential",
        colors: &[
            "#b5d4e9", "#93c3df", "#6daed5", "#4b97c9", "#2f7ebc", "#1864aa", "#0a4a90",
            "#08306b",
        ],
    },
    ChartPalette {
        id: "coral-teal-div",
        name_en: "Coral-teal (diverging)",
        name_zh: "珊瑚-青（发散）",
        kind: "diverging",
        colors: &[
            "#E04355", "#E87180", "#EFA1AA", "#F7D0D4", "#F6F6F7", "#C8E9F1", "#8FD3E4",
            "#58BDD7", "#1FA8C9",
        ],
    },
    ChartPalette {
        id: "climate-div",
        name_en: "Climate (diverging)",
        name_zh: "冷暖气候（发散）",
        kind: "diverging",
        colors: &[
            "#a50026", "#d73027", "#f46d43", "#fdae61", "#fee090", "#e0f3f8", "#abd9e9",
            "#74add1", "#4575b4", "#313695",
        ],
    },
];

pub fn palette_for(id: &str) -> Option<&'static ChartPalette> {
    let id = id.trim();
    CHART_PALETTES.iter().find(|p| p.id == id)
}

pub fn palette_name_en(id: &str) -> Option<&'static str> {
    palette_for(id).map(|p| p.name_en)
}

pub fn palette_spec(id: &str) -> Option<String> {
    let p = palette_for(id)?;
    let colors = p
        .colors
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let mut out = format!(
        "Palette spec — {} (palette id `{}`, {}):\n\
         Colors (ordered, use verbatim): [{}]\n",
        p.name_en, p.id, p.kind, colors
    );
    match p.kind {
        "categorical" => {
            out.push_str(
                "Usage rules:\n\
                 - ECharts: set the option's top-level `color` array to this exact list; series then \
                 cycle through it in order.\n\
                 - Single-series charts: use the FIRST color as the series color; do not rainbow a \
                 single measure.\n\
                 - Charts with <=5 series: prefer maximally separated picks from the list (e.g. \
                 indexes 0, 4, 5, 8, 3) instead of the first five neighbors when adjacent hues read \
                 too close.\n\
                 - Mermaid: set `primaryColor` to the first color and reuse the next colors for \
                 additional node classes when needed.\n",
            );
            if p.id == "indigo-spectrum-10" {
                out.push_str(
                    "- High-contrast subset for <=5 categories: [\"#3E4784\", \"#E31B54\", \
                     \"#EBC405\", \"#0086C9\", \"#7763CF\"].\n",
                );
            }
            if p.id == "classic-20" {
                out.push_str(
                    "- Colors are dark/light pairs: use even indexes for primary series and the \
                     following odd index for its lighter companion (forecast vs actual, area fill \
                     vs line).\n",
                );
            }
        }
        "sequential" => {
            out.push_str(
                "Usage rules:\n\
                 - For magnitude encodings only: heatmaps, choropleth-style fills, density scales, \
                 single-hue bar gradients.\n\
                 - ECharts: use as `visualMap.inRange.color` (ordered low → high), or pick 3-5 \
                 evenly spaced stops for binned legends.\n\
                 - Never use a sequential ramp to distinguish unrelated categories.\n",
            );
        }
        _ => {
            out.push_str(
                "Usage rules:\n\
                 - For signed/centered data only: deltas, gains vs losses, sentiment, correlation.\n\
                 - ECharts: use as `visualMap.inRange.color` with the midpoint anchored at zero (or \
                 the neutral baseline).\n\
                 - The middle color is the neutral point; both ends must map to equal magnitudes.\n",
            );
        }
    }
    Some(out)
}

pub fn palette_menu() -> String {
    CHART_PALETTES
        .iter()
        .map(|p| format!("`{}` ({}, {})", p.id, p.name_en, p.kind))
        .collect::<Vec<_>>()
        .join(", ")
}
