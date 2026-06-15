// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub struct DeckStyle {
    pub id: &'static str,
    pub name_en: &'static str,
    pub name_zh: &'static str,
    pub spec: &'static str,
}

pub const DECK_STYLES: &[DeckStyle] = &[
    DeckStyle {
        id: "business-simple",
        name_en: "Business simple",
        name_zh: "简约商务",
        spec: "Style spec — Business simple (top-tier consulting deck), theme id `business-simple`:\n\
- Mood: professional, stable, restrained; information clarity is the only priority.\n\
- Theme palette: navy background, white text, sky-blue accent. Reserve `accent` for key numbers, \
conclusion keywords and highlighted bullet markers — at most 2-3 accent moments per slide; use \
`muted` for secondary copy and `hairline` for thin divider lines (1px line shapes).\n\
- Composition: strict alignment to the layout slots; one decisive headline per slide; prefer \
two-col and data layouts for evidence; keep decoration to thin hairline rules and at most one \
surface panel per slide.\n\
- Data: present numbers as large `number`-role text blocks or compact tables, never decorative.\n\
- Imagery: only if the brief supplies real assets; otherwise no images — typography carries the deck.",
    },
    DeckStyle {
        id: "tech-modern",
        name_en: "Tech modern",
        name_zh: "现代科技",
        spec: "Style spec — Tech modern (deep-space SaaS futurism), theme id `tech-modern`:\n\
- Mood: mysterious, deep, high-energy; a dark product-launch atmosphere.\n\
- Theme palette: midnight background, cool gray text, electric-blue `accent` and cyber-purple \
`accent2`. Alternate accent and accent2 across sections; use translucent `surface` panels \
(shape fill alpha 0.5-0.8) behind grouped content.\n\
- Composition: asymmetric but slot-anchored; oversized section numbers; thin accent rules as \
energy lines (line shapes in accent, width 2-3).\n\
- Data: tables with accent headers; key stats as glowing-feeling `number` blocks in accent.\n\
- Imagery: AI backgrounds work well on cover/section slides — prompt for dark abstract circuitry, \
volumetric light, deep blue/purple gradients matching the palette.",
    },
    DeckStyle {
        id: "academic-formal",
        name_en: "Academic formal",
        name_zh: "严谨学术",
        spec: "Style spec — Academic formal (print publication rigor), theme id `academic-formal`:\n\
- Mood: rational, objective, intellectually weighty; quiet like a hardcover journal page.\n\
- Theme palette: off-white background, charcoal text, ONE deep-blue accent used at <=5% coverage \
(key findings, one highlighted row). Serif typography is built into the theme.\n\
- Composition: classic top-bottom alignment, generous margins, numbered headings allowed \
(\"1. 研究背景\"); figure/table captions as `caption` blocks under visuals; source lines as \
caption blocks at the bottom.\n\
- Data: clean bordered tables; cite sources; never invent metrics.\n\
- Imagery: none unless the brief supplies real figures; structure and typography carry the deck.",
    },
    DeckStyle {
        id: "creative-fun",
        name_en: "Creative fun",
        name_zh: "活泼创意",
        spec: "Style spec — Creative fun (Memphis startup energy), theme id `creative-fun`:\n\
- Mood: relaxed, joyful, imaginative; bright sunny flat colors.\n\
- Theme palette: warm-yellow background, near-black text, vibrant orange `accent` and grass-green \
`accent2`. Clash accents deliberately between blocks but keep body text near-black for legibility.\n\
- Composition: playful but slot-anchored — round-rect surface cards with bold radii (24-32), \
circles and rounded shapes as decorative badges; oversized friendly headlines.\n\
- Data: chunky tables with accent headers; numbers as big playful `number` blocks.\n\
- Imagery: AI illustrations in flat sticker style match well on cover/section slides.",
    },
    DeckStyle {
        id: "minimalist-clean",
        name_en: "Minimalist clean",
        name_zh: "极简清爽",
        spec: "Style spec — Minimalist clean (Scandinavian gallery calm), theme id `minimalist-clean`:\n\
- Mood: ethereal, tranquil, 'less is more'; airy and soft.\n\
- Theme palette: haze-gray background, near-black headlines, gray-blue `accent` used sparingly.\n\
- Composition: negative space IS the composition — keep >=60% of each slide empty; one focal \
point per slide; hairline rules as the only dividers; never more than 5 blocks per slide.\n\
- Data: ultra-restrained — direct labels, thin hairline table borders, one number can be an \
entire slide.\n\
- Imagery: avoid raster images; rely on whitespace and type.",
    },
    DeckStyle {
        id: "luxury-premium",
        name_en: "Luxury premium",
        name_zh: "高端奢华",
        spec: "Style spec — Luxury premium (haute couture ceremony), theme id `luxury-premium`:\n\
- Mood: mysterious, noble, singular; spotlight feel — key elements lit against darkness.\n\
- Theme palette: obsidian background, champagne-gold `accent` for headings/ornaments/key figures, \
warm off-white body text. Never introduce other hues.\n\
- Composition: centered or symmetric placement (use align center on hero blocks); thin gold rules \
(line shapes in accent, width 1-2) above and below focal statements; abundant darkness around \
focal content. Serif typography is built into the theme.\n\
- Data: gold-on-black numbers; jewel-like precision over density.\n\
- Imagery: AI backgrounds only for the cover — dark velvet textures with faint gold light.",
    },
    DeckStyle {
        id: "nature-fresh",
        name_en: "Nature fresh",
        name_zh: "自然清新",
        spec: "Style spec — Nature fresh (organic well-being), theme id `nature-fresh`:\n\
- Mood: healing, breathable, organic — warm soft morning light.\n\
- Theme palette: soft-beige background, forest-green `accent`, earth-brown `accent2`, deep \
brown-gray text; cream `surface` cards with rounded corners (radius 16-24).\n\
- Composition: slightly loose, organic arrangement but still slot-anchored; rounded surface \
cards group related points; generous breathing room.\n\
- Data: earth-tone tables; growth numbers in forest green.\n\
- Imagery: AI botanical/landscape imagery suits cover and section slides — soft light, organic \
textures, palette-matched greens and beiges.",
    },
    DeckStyle {
        id: "gradient-vibrant",
        name_en: "Gradient vibrant",
        name_zh: "渐变活力",
        spec: "Style spec — Gradient vibrant (aurora glassmorphism), theme id `gradient-vibrant`:\n\
- Mood: dreamy, translucent, breathing — aurora flow; elegant color fusion.\n\
- Theme palette: the theme background is a blue→magenta gradient; all text pure white for \
clarity; `accent` (soft gold) only for the single highlighted word or number per slide.\n\
- Composition: glass panels — white surface shapes at alpha 0.10-0.16 with white hairline \
strokes (alpha 0.3) behind grouped content; generous spacing; content always sits on a glass \
panel to guarantee contrast.\n\
- Data: white text tables on glass panels; one gradient-accented number per data slide.\n\
- Imagery: avoid raster images; the gradient background plus glass panels carry the visual.",
    },
    DeckStyle {
        id: "swiss-editorial",
        name_en: "Swiss editorial",
        name_zh: "瑞士国际主义",
        spec: "Style spec — Swiss editorial (International Typographic Style), theme id `swiss-editorial`:\n\
- Mood: objective clarity, typographic confidence, mathematical order.\n\
- Theme palette: paper-white background, ink-black text, ONE signal-red `accent` — used only \
for rules, key numbers and the page marker; cool gray `muted` for secondary marks.\n\
- Composition: rigorous grid — oversized flush-left headlines with tight copy; strong horizontal \
black rules (line shapes, width 3-4) anchoring sections; oversized slide numbers as graphic \
elements on section slides; deliberate white space; flush-left ragged-right everywhere, never \
center body text.\n\
- Data: black ink tables with the red accent marking exactly one insight; direct labels.\n\
- Imagery: geometric abstraction only — solid circles/bars as shape blocks in black/red; no photos.",
    },
    DeckStyle {
        id: "dark-keynote",
        name_en: "Dark keynote",
        name_zh: "暗色主题演讲",
        spec: "Style spec — Dark keynote (product-launch stage), theme id `dark-keynote`:\n\
- Mood: cinematic stage presence; one idea glowing per slide against near-black depth.\n\
- Theme palette: near-black background, pure-white primary text, ONE electric-azure `accent` for \
key words/numbers, gray `muted` for secondary text.\n\
- Composition: centered or golden-ratio focal placement (align center on hero statements); \
massive breathing room; keynote density — minimal text per slide; thin accent underlines (line \
shapes) as the only ornament.\n\
- Data: oversized single-metric `number` callouts preferred over dense tables.\n\
- Imagery: AI backgrounds for cover/section — dark gradient meshes with soft azure glows.",
    },
    DeckStyle {
        id: "ink-wash",
        name_en: "Ink wash",
        name_zh: "墨韵东方",
        spec: "Style spec — Ink wash (oriental literati elegance), theme id `ink-wash`:\n\
- Mood: tranquil, scholarly, breathing like rice paper; the restraint of Chinese ink painting \
translated to slides.\n\
- Theme palette: rice-paper background, ink-black text, vermilion-seal `accent` (use like a seal \
stamp — one small decisive mark per slide: a key word, a number, a thin rule), indigo `accent2` \
for secondary marks. Serif/Kai typography is built into the theme.\n\
- Composition: vast negative space (>=55% empty); vertical rhythm — short assertion titles; thin \
hairline rules as scroll dividers; one small accent square/seal shape (rect 24-40px, fill accent) \
beside titles as the signature mark.\n\
- Data: minimal tables with hairline rules only; numbers in ink black with one vermilion highlight.\n\
- Imagery: AI imagery suits cover/section — ink-wash mountains, mist, paper texture in palette \
tones; otherwise pure typography carries the deck.",
    },
    DeckStyle {
        id: "china-red",
        name_en: "China red",
        name_zh: "中国红",
        spec: "Style spec — China red (festive ceremony), theme id `china-red`:\n\
- Mood: celebratory, confident, warm — annual meetings, launches, milestones, festival campaigns.\n\
- Theme palette: deep-red background, warm ivory text, gold `accent` for headings ornaments and \
key numbers, soft gold `muted` for secondary copy. Never introduce cool hues.\n\
- Composition: centered or symmetric hero statements (align center on cover/ending); thin gold \
rules (line shapes in accent, width 1-2) framing focal content; gold `number` blocks for \
milestones; surface panels sparingly (slightly lighter red) behind grouped points.\n\
- Data: gold-on-red numbers; small ivory tables with gold header row.\n\
- Imagery: AI imagery for cover only — red silk, lanterns, golden particles matching the palette.",
    },
    DeckStyle {
        id: "magazine-editorial",
        name_en: "Magazine editorial",
        name_zh: "杂志编辑",
        spec: "Style spec — Magazine editorial (long-form print feature), theme id `magazine-editorial`:\n\
- Mood: cultured, narrative, art-directed — a feature spread in a quality magazine.\n\
- Theme palette: cream background, near-black ink text, burnt-orange `accent` for drop-cap style \
moments and pull-quote marks, deep-blue `accent2` for secondary highlights. The theme pairs serif \
headings with sans body — let that contrast carry the design.\n\
- Composition: editorial hierarchy — oversized serif headlines, kicker labels above titles \
(`label` role), pull-quotes as `quote` layout slides between content chapters; thick ink rules \
(line shapes width 3-5) under titles; asymmetric two-col spreads (text left, visual right).\n\
- Data: numbers presented as magazine infographics — big `number` blocks with caption beneath, \
hairline-ruled tables.\n\
- Imagery: AI imagery works on cover/section/image-full — photographic, warm-toned, editorial \
framing; captions mandatory under images.",
    },
    DeckStyle {
        id: "data-insight",
        name_en: "Data insight",
        name_zh: "数据洞察",
        spec: "Style spec — Data insight (analytics report clarity), theme id `data-insight`:\n\
- Mood: crisp, trustworthy, evidence-first — the house style of a data team's readout.\n\
- Theme palette: white background, deep-slate text, teal `accent` for positive/key series and \
KPI numbers, blue `accent2` for comparison series, pale `surface` cards on light hairlines.\n\
- Composition: takeaway-first — every data slide's title states the conclusion, evidence below; \
KPI card rows (roundRect surface panels radius 12 + `number` + caption, three across); use the \
`data` layout heavily; bar comparisons built from rect shapes with widths proportional to REAL \
values, teal for the highlighted bar.\n\
- Data: tables are first-class — teal header row, zebra-free, right-align numeric columns by \
keeping numbers short; one insight highlighted per table via an accent-colored cell text.\n\
- Imagery: no decorative photos; data structures ARE the visuals.",
    },
    DeckStyle {
        id: "sunset-warm",
        name_en: "Sunset warm",
        name_zh: "暮色暖阳",
        spec: "Style spec — Sunset warm (storytelling glow), theme id `sunset-warm`:\n\
- Mood: emotional, optimistic, human — brand stories, community, vision and culture decks.\n\
- Theme palette: the theme background is a burnt-orange → deep-rose gradient; warm ivory text; \
sun-gold `accent` for the single glowing word or number per slide; rose `accent2` sparingly.\n\
- Composition: glass panels (white surface shapes at alpha 0.10-0.16, radius 20) behind grouped \
content to guarantee contrast on the gradient; generous spacing; hero statements centered on \
cover/section; story beats as one-idea-per-slide statements.\n\
- Data: keep numbers big and few — gold `number` blocks on glass panels; avoid dense tables.\n\
- Imagery: AI imagery suits cover/section — golden-hour skies, silhouettes, soft warm light in \
palette tones.",
    },
    DeckStyle {
        id: "mono-noir",
        name_en: "Mono noir",
        name_zh: "黑白极简",
        spec: "Style spec — Mono noir (high-contrast typographic minimalism), theme id `mono-noir`:\n\
- Mood: stark, fearless, statement-driven — pure black on white, nothing to hide behind.\n\
- Theme palette: white background, pure-black text — black IS the accent. Emphasis comes from \
scale and weight contrast, never from color; gray `muted` only for captions and page furniture.\n\
- Composition: extreme type scale — display titles dominating each slide; thick black rules \
(line shapes width 4-6) as the only graphic element; occasional inverted slides (background color \
`text`, text in `background`) for section dividers to create rhythm; never more than 5 blocks \
per slide.\n\
- Data: black tables with bold header on black fill + white header text; one number can own an \
entire slide at `display` scale.\n\
- Imagery: none, or strictly black-and-white photographic assets supplied by the brief.",
    },
    DeckStyle {
        id: "bento-grid",
        name_en: "Bento grid",
        name_zh: "便当栅格",
        spec: "Style spec — Bento grid (modern product-keynote card mosaic), theme id `bento-grid`:\n\
- Mood: crisp, friendly, product-launch polish — every idea lives in its own rounded card.\n\
- Theme palette: soft light-gray background, pure-white `surface` cards, near-black text, ONE \
blue `accent` plus violet `accent2` reserved for a single hero stat or card per slide; `hairline` \
only as card borders, never free-floating rules.\n\
- Composition: tile EVERY content slide as a bento mosaic — 2-5 rounded `surface` rectangles \
(corner radius large, shape fill `surface`) of mixed sizes anchored to the layout slots, one fact \
or metric per card; the most important card may be 2x size or filled with `accent`; keep gutters \
even and breathe — cards never touch.\n\
- Data: each KPI gets its own card with a `number`-role block plus a one-line caption; tables \
live inside a single wide card; comparisons become side-by-side cards.\n\
- Imagery: optional small image cards on cover/section slides — soft 3D abstract shapes or \
gradient blobs that match the palette; content slides stay image-free.",
    },
    DeckStyle {
        id: "neo-brutalist",
        name_en: "Neo brutalist",
        name_zh: "新粗野主义",
        spec: "Style spec — Neo brutalist (loud raw web-poster energy), theme id `neo-brutalist`:\n\
- Mood: bold, raw, confident — flat colors, hard edges, zero subtlety, maximum statement.\n\
- Theme palette: cream background, white panels, pure-black text and borders; saturated orange \
`accent` and electric-blue `accent2` as flat fills behind headline words or key cards — never \
gradients, never shadows softer than a hard offset.\n\
- Composition: thick black outlines (line shapes width 4-8) framing content panels; deliberately \
chunky misaligned-looking blocks that still snap to layout slots; oversized grotesque headlines \
in `display` scale; stickers/badges as small accent-filled rectangles with black borders carrying \
short ALL-CAPS labels; at most 4 panels per slide.\n\
- Data: tables with hard black borders on every cell and an accent-filled header row; numbers \
huge and black on accent panels.\n\
- Imagery: avoid AI photography — flat shape compositions and typography carry everything; if an \
image is essential, frame it inside a hard black border panel.",
    },
    DeckStyle {
        id: "crimson-report",
        name_en: "Crimson report",
        name_zh: "红韵报告",
        spec: "Style spec — Crimson report (classic red-and-white corporate report), theme id `crimson-report`:\n\
- Mood: formal, decisive, ceremonial confidence — annual reports, government and enterprise readouts.\n\
- Theme palette: white background, ink-gray text, deep-crimson `accent` for headline keywords, \
section numbers, rules and key figures; steel-gray `accent2` for secondary marks. Never introduce \
other hues.\n\
- Composition: strong horizontal crimson rules (line shapes width 3-5) under titles; small crimson \
squares/flags as bullet anchors; generous white margins; kicker labels in accent above titles; \
symmetric, document-like order. Use `cards-3`/`cards-4` for parallel points with thin crimson top \
borders on each card (line shape on the card's top edge).\n\
- Data: white tables with a crimson header row; key totals as crimson `number` blocks.\n\
- Imagery: AI imagery only on the cover — abstract red silk/ribbon or architectural minimal forms \
matching the palette; body slides stay typographic.",
    },
    DeckStyle {
        id: "teal-breeze",
        name_en: "Teal breeze",
        name_zh: "青澜清风",
        spec: "Style spec — Teal breeze (fresh geometric business), theme id `teal-breeze`:\n\
- Mood: light, optimistic, approachable professionalism — product intros, campus talks, openings.\n\
- Theme palette: white background, slate text, lake-teal `accent` for headings, markers and key \
numbers; lighter aqua `accent2` for decorative shapes; pale-aqua `surface` panels for grouped \
content.\n\
- Composition: circles are the signature — decorative ellipse outlines and dots (stroke accent2, \
width 2-3) scattered sparingly near titles and corners; rounded surface cards (radius 16-24); airy \
spacing. Use `cards-3` and `timeline` layouts freely; section numbers inside an accent-filled \
circle (ellipse + onAccent number).\n\
- Data: clean tables with teal header; trend numbers in accent.\n\
- Imagery: AI imagery suits cover/section — bright minimal scenes, soft teal gradients, clean \
geometry in palette tones.",
    },
    DeckStyle {
        id: "violet-haze",
        name_en: "Violet haze",
        name_zh: "紫霭雅集",
        spec: "Style spec — Violet haze (muted-violet quiet elegance), theme id `violet-haze`:\n\
- Mood: composed, cultured, softly premium — brand reviews, culture and lifestyle topics.\n\
- Theme palette: near-white background, deep-plum text, dusk-violet `accent` for titles' key words \
and rules; pale-lilac `accent2` for soft fills; lavender-gray `surface` cards.\n\
- Composition: soft surface cards (radius 12-20) with hairline strokes; thin violet rules under \
titles (width 2); restrained ornament — one small accent2-filled rounded square beside each card \
title as a marker; balanced two-col spreads. `cards-3` for parallel themes, `quote` slides as \
breathing moments.\n\
- Data: hairline tables with violet header text on surface fill; highlight one figure in accent.\n\
- Imagery: AI imagery on cover/section — misty gradients, soft fabric or floral abstractions in \
muted violet tones.",
    },
    DeckStyle {
        id: "morandi-duotone",
        name_en: "Morandi duotone",
        name_zh: "莫兰迪双色",
        spec: "Style spec — Morandi duotone (sage × blush paired-color calm), theme id `morandi-duotone`:\n\
- Mood: gentle, balanced, tasteful restraint — proposals, education, wellness, brand decks.\n\
- Theme palette: warm-paper background, moss-gray text, sage-green `accent` and blush-clay \
`accent2` as an EQUAL pair — alternate them between sections or between paired cards; white \
`surface` cards on hairline borders.\n\
- Composition: the duotone is the system — paired layouts (`two-col`, `cards-4`) where left/odd \
cards take sage marks and right/even cards take blush marks (thin top rules or small filled \
squares); soft radius 12-16; abundant margins; never let both colors fight inside one text line.\n\
- Data: white tables, header row filled `accent` for primary metrics or `accent2` for secondary \
comparisons; numbers in the matching hue.\n\
- Imagery: AI imagery on cover/section — still-life minimalism, matte ceramics, soft daylight in \
sage/blush tones.",
    },
    DeckStyle {
        id: "jade-serif",
        name_en: "Jade serif",
        name_zh: "松石宋韵",
        spec: "Style spec — Jade serif (green-accented serif literati-business), theme id `jade-serif`:\n\
- Mood: trustworthy, scholarly vitality — ESG, agriculture, health, sustainable-growth narratives.\n\
- Theme palette: white background, ink-green-tinted text, deep-jade `accent` for assertions and \
numbers, bright-jade `accent2` for secondary marks; pale-mint `surface` panels. Serif (Song-style) \
typography is built into the theme — let the serifs carry gravitas.\n\
- Composition: editorial serif headlines flush-left; thin jade rules (width 2) as section anchors; \
vertical hairline between `two-col` columns; growth stories on the `timeline` layout with jade \
dots (small ellipses) on the axis; KPI moments on the `kpi` layout with jade values.\n\
- Data: hairline tables, jade header text, one jade-highlighted row; realistic figures only.\n\
- Imagery: AI imagery on cover/section — botanical close-ups, terraced landscapes, jade-toned \
minimal scenes.",
    },
    DeckStyle {
        id: "cocoa-gold",
        name_en: "Cocoa gold",
        name_zh: "可可鎏金",
        spec: "Style spec — Cocoa gold (warm light-luxury hospitality), theme id `cocoa-gold`:\n\
- Mood: warm, tasteful, quietly premium — food & beverage, hospitality, boutique brand decks.\n\
- Theme palette: cream background, dark-cocoa text, cocoa-brown `accent` for headings and rules, \
honey-gold `accent2` for the single glowing highlight per slide (a number, a keyword, a marker); \
white `surface` cards with warm hairlines.\n\
- Composition: serif headings over sans body (built into the theme); thin gold underlines (line \
shapes accent2, width 2) beneath focal words; cocoa-filled section-number badges (roundRect + \
onAccent number); cards with warm hairline strokes on `cards-3`; menus/offers as elegant tables.\n\
- Data: cream tables with cocoa header row and gold-accented key figures.\n\
- Imagery: AI imagery on cover/section — warm-lit textures (roasted tones, linen, wood, coffee), \
soft shadows, palette-matched browns and golds.",
    },
    DeckStyle {
        id: "scroll-antique",
        name_en: "Scroll antique",
        name_zh: "缃帙古卷",
        spec: "Style spec — Scroll antique (classical Chinese scroll, Kai typography), theme id `scroll-antique`:\n\
- Mood: antiquarian, dignified, literary — culture, history, museums, traditional-craft subjects.\n\
- Theme palette: aged-paper background, prussian-ink text, deep-prussian `accent` for assertions \
and seals, antique-brown `accent2` for secondary marks; lighter-paper `surface` panels. Kai \
(楷/仿宋) typography is built into the theme.\n\
- Composition: scroll discipline — wide margins, vertical hairline dividers between columns \
(line shapes width 1), one prussian seal-square (rect 28-40, fill accent, onAccent character or \
number) beside each title; hairline frames (stroke, no fill) around surface panels like mounted \
calligraphy; never crowd a slide.\n\
- Data: minimal hairline tables; numbers in ink with one accent highlight.\n\
- Imagery: AI imagery on cover/section — ink landscapes, paper texture, antique artifacts in \
palette tones; body slides remain typographic.",
    },
    DeckStyle {
        id: "powder-azure",
        name_en: "Powder azure",
        name_zh: "天青文苑",
        spec: "Style spec — Powder azure (airy azure academic elegance), theme id `powder-azure`:\n\
- Mood: serene, lucid, quietly intellectual — culture courses, reading clubs, light academic talks.\n\
- Theme palette: powder-blue background, navy-ink text, deep-azure `accent` for headings and \
markers, powder `accent2` for soft decorative fills; white `surface` cards.\n\
- Composition: Fangsong display headings (built into the theme) over clean sans body; white cards \
on the powder background with hairline strokes (radius 8-12); thin azure rules under titles; \
small accent2 circles as list markers; `agenda` and `cards-3` carry course/章节 structures well.\n\
- Data: white tables with azure header text; modest, well-spaced figures.\n\
- Imagery: AI imagery on cover only — sky, porcelain, paper-cut clouds in powder-azure tones; \
body slides stay calm and typographic.",
    },
];

pub fn deck_style_spec(id: &str) -> Option<&'static str> {
    DECK_STYLES
        .iter()
        .find(|s| s.id.eq_ignore_ascii_case(id.trim()))
        .map(|s| s.spec)
}

pub fn deck_style_name_en(id: &str) -> Option<&'static str> {
    DECK_STYLES
        .iter()
        .find(|s| s.id.eq_ignore_ascii_case(id.trim()))
        .map(|s| s.name_en)
}

pub fn style_menu() -> String {
    DECK_STYLES
        .iter()
        .map(|s| format!("`{}` ({})", s.id, s.name_en))
        .collect::<Vec<_>>()
        .join(", ")
}
