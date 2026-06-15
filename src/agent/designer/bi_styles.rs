// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub struct BiStyle {
    pub id: &'static str,
    pub name_en: &'static str,
    pub name_zh: &'static str,
    pub spec: &'static str,
}

pub const BI_STYLES: &[BiStyle] = &[
    BiStyle {
        id: "tech-command-center",
        name_en: "Tech command center",
        name_zh: "科技指挥中心",
        spec: "Style spec — Tech command center (large-screen control room), style id `tech-command-center`:\n\
- Mood: mission control at night — deep, luminous, authoritative; data feels alive.\n\
- Palette: near-black navy stage (#050e1f → #0a1a33 radial), cyan `--od-accent` default #19d3f5 \
for glows/key numbers, ice-white primary text, desaturated blue-gray secondary text; an amber \
warning hue reserved for alerts only.\n\
- Stage: fixed 1920x1080 composition scaled to fit the viewport (transform scale wrapper); a slim \
top header strip with the screen title centered, live clock on the right, status pill on the left.\n\
- Panel chrome: translucent panels (rgba white 0.02-0.04) with 1px cyan hairline borders at 25-40% \
alpha and corner-bracket accents (small L-shaped strokes on all four corners); soft outer glow on \
the focal panel only.\n\
- Layout: 12-column grid — tall side columns of stacked panels flanking one dominant center zone \
(map, flow diagram, or the hero chart); KPI strip directly under the header.\n\
- Charts: inline SVG/canvas only — glowing line charts with gradient area fills, thin-ring donuts, \
bar charts with rounded caps; gridlines at 6-8% white alpha; numbers in a tabular, slightly \
condensed numeric style with cyan glow on heroes.\n\
- Motion: slow breathing glow on status dots, count-up on KPI numbers at load, marquee only for \
alert tickers; everything respects `--od-motion` and reduced-motion.",
    },
    BiStyle {
        id: "gov-blue-screen",
        name_en: "Gov blue screen",
        name_zh: "政企蓝大屏",
        spec: "Style spec — Gov blue screen (Chinese government/enterprise big screen), style id `gov-blue-screen`:\n\
- Mood: formal, symmetric, ceremonious — the classic 政务大屏 idiom executed with restraint.\n\
- Palette: deep-blue gradient stage (#04123a → #0b2a6b), bright azure `--od-accent` default \
#2f8cff, gold #f5c451 strictly for the title band and the single most important number, white text.\n\
- Stage: fixed 1920x1080 scaled to fit; a decorated center title band (thin gold rules flanking \
the title, subtle wing ornaments built from CSS gradients — no image assets).\n\
- Layout: strict mirror symmetry — equal left/right panel columns, dominant center map/chart zone, \
a bottom ticker strip; panels use double-line borders (outer 1px azure 30%, inner 1px azure 12%).\n\
- Charts: inline SVG bar/line/donut with azure-to-cyan gradients; rank lists with index badges \
(top 3 in gold); KPI numbers extra bold with unit characters at 50% size.\n\
- Motion: slow vertical auto-scroll inside rank-list panels, gentle pulse on the center zone; \
all bound to `--od-motion`.",
    },
    BiStyle {
        id: "dark-glass-analytics",
        name_en: "Dark glass analytics",
        name_zh: "暗色玻璃分析",
        spec: "Style spec — Dark glass analytics (glassmorphism product analytics), style id `dark-glass-analytics`:\n\
- Mood: premium SaaS at night — soft, deep, dimensional; calm confidence over spectacle.\n\
- Palette: charcoal-indigo gradient stage (#0d0f1a → #161a2e) with two faint aurora blobs \
(violet + cyan radial gradients at 12-18% alpha, blurred); `--od-accent` default #8b7cf6; white \
text at 92/64/40% alpha tiers.\n\
- Panel chrome: glass cards — rgba(255,255,255,0.05) fills, backdrop-blur(14px) when supported \
with a solid fallback, 1px white hairline at 10%, 24px radii, soft 30px shadows.\n\
- Layout: fluid 12-column grid (this style may flow responsively instead of a fixed stage); \
generous 20-24px gaps; KPI cards top, dual chart row, breakdown tables below.\n\
- Charts: smooth gradient-stroked SVG lines with glassy area fills, segmented progress rings, \
soft-edged heat strips; legend chips as tiny glass pills.\n\
- Motion: 250-400ms eased entrance per panel (staggered), hover lift 2px; honors `--od-motion`.",
    },
    BiStyle {
        id: "light-saas-analytics",
        name_en: "Light SaaS analytics",
        name_zh: "明亮 SaaS 分析",
        spec: "Style spec — Light SaaS analytics (daylight product dashboard), style id `light-saas-analytics`:\n\
- Mood: clean, optimistic, product-grade — flagship-SaaS clarity.\n\
- Palette: #f7f8fb canvas, pure-white cards with 1px #e7e9f0 borders and 1-2px shadows, ink text \
#171a22, `--od-accent` default #5560ff; a categorical chart palette of 5-6 muted-saturation hues.\n\
- Layout: fluid 12-column grid with a slim top bar (title, date-range chip, refresh affordance); \
KPI row of 4 cards with delta badges (green up / red down, with arrows); main chart card spanning \
8 columns beside a 4-column breakdown; tables with sticky headers below.\n\
- Charts: inline SVG — clean 2px line charts with dot terminals, rounded 6px bars, donut with \
center KPI; axis labels 11-12px gray; gridlines #eef0f5.\n\
- Numbers: tabular-nums everywhere; large KPIs 28-36px semibold with 12px unit captions.\n\
- Motion: subtle 200ms fades and count-ups only; hover states on every interactive row.",
    },
    BiStyle {
        id: "terminal-ops",
        name_en: "Terminal ops",
        name_zh: "终端运维",
        spec: "Style spec — Terminal ops (NOC / SRE monitoring wall), style id `terminal-ops`:\n\
- Mood: hacker-utilitarian — a wall of living telemetry; zero decoration, maximum signal.\n\
- Palette: true black #0a0c0a stage, phosphor green `--od-accent` default #33ff66 for healthy, \
amber #ffb347 for warnings, red #ff4d4d for critical; all text in a monospace stack \
(ui-monospace, 'JetBrains Mono', Menlo).\n\
- Panel chrome: 1px green borders at 25% alpha, square corners, header rows rendered like shell \
prompts (`$ service-name --status`); faint scanline overlay (repeating-linear-gradient at 3% alpha).\n\
- Layout: dense grid — service status matrix (small cells with colored state dots), latency \
sparkline strip, an auto-scrolling log feed column with severity-colored lines, uptime gauges.\n\
- Charts: ASCII-flavored SVG — stepped lines, block-character-style bar fills, braille-density \
sparklines; numeric readouts dominate over axes.\n\
- Motion: blinking cursor in the log feed, state-dot pulse on incidents only; bound to `--od-motion`.",
    },
    BiStyle {
        id: "neon-cyber",
        name_en: "Neon cyber",
        name_zh: "霓虹赛博",
        spec: "Style spec — Neon cyber (cyberpunk data wall), style id `neon-cyber`:\n\
- Mood: rain-slick midnight city — electric, saturated, theatrical; data as neon signage.\n\
- Palette: #0b0514 stage with a faint magenta-to-cyan diagonal wash; dual accents — magenta \
#ff3d9a and cyan #18e6ff (bind `--od-accent` to magenta); white text with colored glows on heroes.\n\
- Panel chrome: 1px neon borders with outer glow (box-shadow 0 0 12px accent at 35%), clipped \
corners (polygon clip-path notches on one or two corners), thin gradient top-rails per panel.\n\
- Layout: fixed 1920x1080 scaled stage; asymmetric grid with one oversized hero metric zone; \
diagonal section dividers built from gradients.\n\
- Charts: neon-stroked SVG lines with bloom (duplicate blurred stroke under the crisp one), \
gradient bars magenta→cyan, radar charts with glowing vertices.\n\
- Motion: slow hue-shift on the background wash, flicker-in on panel mount (one-shot), pulsing \
hero number; all gated by `--od-motion` and reduced-motion.",
    },
    BiStyle {
        id: "fintech-trading",
        name_en: "Fintech trading",
        name_zh: "金融行情",
        spec: "Style spec — Fintech trading (markets terminal wall), style id `fintech-trading`:\n\
- Mood: trading floor intensity — dense, precise, unsentimental; numbers are the interface.\n\
- Palette: #0c0e12 stage, graphite panels #14171d with 1px #232833 borders, off-white text; \
semantic up/down colors (up green #22c373 / down red #f04444 — flip to red-up only if the brief \
is a CN equities context and says so); `--od-accent` default #3d8bff for selection/focus.\n\
- Layout: fixed 1920x1080 scaled stage; a top index strip (horizontally scrolling tickers with \
delta arrows), candlestick or area hero chart center-left, order-book/depth columns right, \
movers rank table and heat-tile sector map below.\n\
- Type: everything numeric in tabular-nums; tick sizes 11-13px; row height 26-30px for density; \
deltas always signed with arrows and percentage.\n\
- Charts: inline SVG candlesticks (thin wicks, 60% body width), depth area chart mirrored \
bid/ask, sector heat tiles colored by magnitude with value labels.\n\
- Motion: row flash on simulated tick updates (green/red 300ms fade), ticker marquee; honors \
`--od-motion`.",
    },
    BiStyle {
        id: "enterprise-neutral",
        name_en: "Enterprise neutral",
        name_zh: "企业稳重",
        spec: "Style spec — Enterprise neutral (boardroom KPI wall), style id `enterprise-neutral`:\n\
- Mood: composed executive reporting — trustworthy, unhurried, print-quality discipline.\n\
- Palette: #f4f5f7 canvas or #1b1e24 dark variant (pick from the brief; default light), white \
panels with 1px #dfe2e8 borders, slate-ink text, ONE corporate `--od-accent` default #1f5eff used \
at <=8% coverage; gray-scale chart palette with the accent reserved for the primary series.\n\
- Layout: calm 12-column grid — title bar with reporting-period label, 3-4 KPI cards with \
sparklines, one dominant trend chart, supporting bar/donut pair, a clean bordered summary table.\n\
- Charts: thin 1.5px lines, unfilled or 6%-alpha area, square-ended bars, generous axis \
whitespace; data labels only on hero points.\n\
- Numbers: large KPIs in semibold with muted captions and YoY/MoM delta chips.\n\
- Motion: none beyond 150ms fades — stability is the statement.",
    },
    BiStyle {
        id: "industrial-scada",
        name_en: "Industrial SCADA",
        name_zh: "工业监控",
        spec: "Style spec — Industrial SCADA (plant telemetry wall), style id `industrial-scada`:\n\
- Mood: heavy-industry control — engineered, schematic, safety-first.\n\
- Palette: gunmetal stage #161a1d, steel panels #1f2429 with 1px #39424a borders, off-white text, \
safety orange `--od-accent` default #ff8a00 for warnings/setpoints, teal #2fd1c5 for healthy \
flows; hazard-stripe (45° repeating gradient) only on critical banners.\n\
- Layout: fixed 1920x1080 scaled stage; a central schematic zone (pipeline/process flow drawn \
with SVG lines, valve/pump nodes as simple geometric symbols with state colors), surrounded by \
gauge clusters, threshold bar meters, and an alarm list panel with severity tags.\n\
- Charts: radial gauges with red-line zones, horizontal bullet bars vs setpoint markers, stepped \
trend lines; units displayed on every readout (°C, kPa, rpm, m³/h).\n\
- Type: condensed numeric readouts in monospace; panel titles uppercase 11px letterspaced.\n\
- Motion: flow-direction dash animation along active pipes, alarm row blink for unacknowledged \
criticals only; bound to `--od-motion`.",
    },
    BiStyle {
        id: "analytics-grid",
        name_en: "Analytics grid",
        name_zh: "明亮分析网格",
        spec: "Style spec — Analytics grid (product analytics workspace), style id `analytics-grid`:\n\
- Mood: working BI tool in daylight — dense but orderly, explore-first, zero spectacle.\n\
- Palette: #f7f8fa canvas, white cards with 1px #e1e4ea borders (radius 8, shadow 0 1px 2px \
rgba(0,0,0,0.04)), ink text #1c2433, `--od-accent` default #2893B3 (teal) for selection, links \
and the primary chart series; success #5ac189 / error #e04355 for deltas; Inter-class sans stack \
with tabular numerals.\n\
- Layout: fluid 12-column grid on an 8px base unit with 16px gutters; slim top bar (board title, \
date-range chip, refresh affordance); a LEFT FILTER RAIL fixed at 260px — search field, 2-3 \
multi-select filter groups with checkboxes and counts, a time-range section, an Apply/Clear \
footer row; content cards default to 4-column width and ~368px height, charts may span 6-8 \
columns.\n\
- Card chrome: header row with a strong-weight title clamped to 2 lines plus a kebab affordance; \
32px content inset; footer meta line (last refreshed, row count) in 12px muted.\n\
- KPI cards: follow the KPI proportions in the sub-mode skill — metric label up top, main number \
at ~30% of card height, signed delta pill, ~30%-height sparkline whose area gradient fades to \
the card background.\n\
- Charts: inline SVG — 2px lines with 0.2-alpha area fills, rounded 4px bars, donut at 30/70 \
radii, axis labels 11px muted, gridlines #eef1f5; one accent-colored primary series, neighbors \
from a muted categorical ramp.\n\
- States: design the empty state (muted illustration block + one-line guidance + primary action) \
and a skeleton-shimmer loading card variant for at least one panel.\n\
- Motion: 150-200ms fades and hover lifts only, honoring `--od-motion`.",
    },
    BiStyle {
        id: "ai-insight-cards",
        name_en: "AI insight cards",
        name_zh: "AI 洞察卡流",
        spec: "Style spec — AI insight cards (conversational analytics feed), style id `ai-insight-cards`:\n\
- Mood: an AI analyst presenting findings — editorial answer first, evidence pinned below.\n\
- Palette: #fafbfc canvas, white cards radius 4 with shadow `rgba(45,62,80,0.12) 0 1px 5px`, \
ink text #262626, muted #65676c, hairline #d9d9d9, `--od-accent` default #444CE7 (indigo) for \
links, active states and the primary series; categorical chart ramp anchored on indigo with \
crimson/amber/teal companions.\n\
- Layout: a centered content column (~1100px) — top ANSWER CARD: question as the card title, a \
2-4 sentence natural-language summary with the key numbers inline-bolded, then a compact data \
preview table (sticky header row, 12px caption \"showing first N rows\"); below it a row of \
RECOMMENDED QUESTION chips (pill outline, accent text, wrap to 2 lines max); then the PINNED \
CHART GRID on a 6-column base with 8px gutters where each card spans 2-3 columns.\n\
- Pinned chart cards: 14px/700 title, right-aligned 12px muted \"Last refreshed\" stamp, \
16px/12px content padding, hover raises the border to the accent color; charts are inline SVG \
(line, grouped bar, donut) with legends as small chips.\n\
- Tooltip idiom: dark panel #262626 with white values and muted keys — render one chart with a \
static example tooltip to set the pattern.\n\
- Tables: 12px vertical cell padding, type icons beside column names, right-aligned numerals \
with tabular-nums, zebra-free with hairline row dividers.\n\
- States: an error variant card (alert strip + retry affordance) and a skeleton answer card \
belong to the system; show at least one.\n\
- Motion: chips and cards fade-slide in once (200-300ms staggered), bound to `--od-motion`.",
    },
    BiStyle {
        id: "minimal-kpi",
        name_en: "Minimal KPI",
        name_zh: "极简 KPI",
        spec: "Style spec — Minimal KPI (oversized numbers wall), style id `minimal-kpi`:\n\
- Mood: gallery-calm confidence — a handful of numbers, perfectly set; whitespace is the design.\n\
- Palette: warm paper #faf9f6 (or near-black #111110 dark variant if the brief asks), ink text, \
ONE `--od-accent` default #c96442 for the single most important delta or underline.\n\
- Layout: fluid grid of 4-8 oversized KPI blocks — value at 72-120px light-weight with \
tabular-nums, label in 12px uppercase letterspaced muted text above, delta chip and a whisper-thin \
sparkline below; one optional full-width hero trend at 1.5px stroke; hairline dividers instead of \
card borders.\n\
- Charts: sparklines and one hero line only — no axes, no legends, no gridlines; direct labeling.\n\
- Density: at least 55% empty space; max 10 numbers on screen; every number must earn its place.\n\
- Motion: a single elegant count-up on load (600ms, eased), nothing else; honors `--od-motion`.",
    },
];

pub fn bi_style_spec(id: &str) -> Option<&'static str> {
    let id = id.trim();
    BI_STYLES.iter().find(|s| s.id == id).map(|s| s.spec)
}

pub fn bi_style_name_en(id: &str) -> Option<&'static str> {
    let id = id.trim();
    BI_STYLES.iter().find(|s| s.id == id).map(|s| s.name_en)
}

pub fn style_menu() -> String {
    BI_STYLES
        .iter()
        .map(|s| format!("`{}` ({})", s.id, s.name_en))
        .collect::<Vec<_>>()
        .join(", ")
}
