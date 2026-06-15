---
name: deck-swiss-international
zh_name: "瑞士国际主义 Deck"
en_name: "Swiss International Deck"
emoji: "🟦"
description: "Swiss International Typographic Style for the structured deck spec: rigorous grid thinking, one saturated accent, typographic confidence."
zh_description: "瑞士国际主义风格的结构化 deck spec 创作指南：网格思维、单一饱和 accent、版面级排印自信"
en_description: "Swiss International Typographic Style for the structured deck spec: rigorous grid thinking, one saturated accent, typographic confidence."
category: slides
scenario: marketing
aspect_hint: "16:9"
featured: 1
recommended: 1
tags: ["swiss", "grid", "international", "ikb", "editorial", "facts"]
od:
  mode: deck
  scenario: marketing
  featured: 0.001
  design_system:
    requires: false
  example_prompt: "Turn my content into a Swiss International deck: rigorous grid, one saturated accent, oversized typography, real content and data only."
  example_prompt_i18n:
    zh-CN: "把我的内容做成一套瑞士国际主义风格的幻灯片：严格网格、单一饱和 accent、超大号排印，只用真实内容和数据。"
---

【Skill: Swiss International deck craft — applies ON TOP of the deck spec format】
Intent: facts, product, analysis, methodology. Cold, rational, academic confidence. No decoration
for its own sake. This skill teaches HOW to compose slides inside the structured deck spec
(`deck.json` + `slides/*.json`); it never changes the file format.

【Accent discipline (the signature)】
One saturated accent carries the whole deck. Default to the theme's accent; when the brief allows a
bolder statement, override `palette.accent` in deck.json with exactly ONE of:
- Klein Blue (IKB) `#002FA7` — business / AI / design.
- Lemon Yellow `#FFD500` — youth / retail / sports (keep text near-black on it).
- Neon Green `#C5E803` — sustainability / startups / Gen-Z (text near-black).
- Safety Orange `#FF6B35` — industrial / automotive / urgency (bold white text on it).
Never mix two accents; never introduce a second hue anywhere.

【Composition vocabulary (build these from spec blocks, slot-anchored or framed)】
- Statement slide — `quote` layout or `content` with one oversized `display`/`title` text block
  (assertion, <=12 words), a hairline rule (line shape, width 3-4, color text), and one caption.
- KPI tower / trio — 3-4 `roundRect`-free rect shape panels (fill surface, radius 0) on explicit
  frames across the width; each carries a `number` block + `caption` label. Vary panel heights to
  encode magnitude when numbers are comparable (heights proportional to REAL data).
- H-bar ranking — per row: a rect shape whose width is proportional to the real value (fill
  accent for the leader, surface for the rest) + a text label + a `number` at the bar end.
- Duo compare — `two-col` layout; a 2px vertical line shape on the seam; left "Before" / right
  "After" with `label` kickers.
- Section divider — `section` layout: oversized slide number in accent, flush-left title; add one
  full-width line shape under the title.
- Ledger — stacked rows separated by 1px hairline line shapes; each row: big `number` left,
  label middle, short text right (explicit frames, equal row heights).
- Closing manifesto — `ending` layout; one accent-filled rect panel behind the title zone with
  onAccent text.

【Iron rules】
- Right angles only: every shape uses `radius: 0`. Rounded corners break the style.
- 1px hairlines (line shapes in `text` or `accent` color) are the only dividers; no decorative fills.
- Typographic scale carries the design: display/title roles oversized, captions small and
  letterspaced in spirit — never center body text; flush-left everything (`align: left`).
- Page furniture stays on: keep `pageNumbers: true` and a short `footer` in deck.json.
- Numbers must come from the user's content; bar/panel proportions reflect real data. Never invent
  metrics.
- Photography excluded unless the brief supplies real assets; geometric abstraction (solid rect /
  ellipse shapes in ink and accent) is the only imagery.

## SenWeaverCoding strengthening

Hard requirements for every deck this skill produces inside Designer mode:

- **Stay inside the deck spec format.** All output is `deck/deck.json` +
  `deck/slides/*.json`; never emit HTML for a deck.
- **One accent end to end.** Lock the accent in deck.json once and reference
  it only via the `accent` token afterwards.
- **Use real content, never filler.** No lorem ipsum, no invented metrics,
  no placeholder copy — compose to fill space with hierarchy, not padding.
- **Vary slide layouts** and keep one clear focal point per slide; run
  `deck_compile` at the end and fix every P0.

<!-- swc-strengthened -->
