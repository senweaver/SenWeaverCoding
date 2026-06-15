---
name: field-notes-editorial-template
description: |
  Editorial "Field Notes" report template with soft paper background, serif hero
  typography, rounded pastel insight cards, and a retention chart panel.
  Use when users ask for a premium magazine-style business report, board memo
  one-pager, or elegant data storytelling layout.
triggers:
  - "field notes editorial template"
  - "editorial report template"
  - "magazine style business report"
  - "pastel insight dashboard"
  - "高级编辑风报告模板"
  - "奶油底粉彩卡片数据报告"
od:
  mode: template
  surface: video
  type: hyperframes
  platform: desktop
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: true
    sections: [color, typography, layout, components]
  outputs:
    primary: index.html
    secondary:
      - template.html
      - example.html
  example_prompt: "Create an editorial Field Notes style report with three insight cards, key metrics blocks, and a retention line chart in one polished single-file HTML page."
  capabilities_required:
    - file_write
---

# Field Notes Editorial Template

Produce a premium editorial data report in a single self-contained HTML file.

## Resource map

```text
field-notes-editorial-template/
├── SKILL.md
├── assets/
│   └── template.html
├── references/
│   └── checklist.md
└── example.html
```

## Workflow

1. Read active `DESIGN.md` and map palette/typography to root CSS variables.
2. Copy `assets/template.html` to `index.html` as the working artifact.
3. Keep the editorial frame language:
   - paper-like background and subtle vignette
   - serif display headlines plus clean sans-serif body copy
   - rounded pastel metric / insight cards
   - chart panel with legend and axis labels
4. Keep interactions lightweight and presentation-safe:
   - page view switcher (metrics / insights / retention)
   - number count-up animation for key metrics
   - chart line reveal animation
5. Use honest placeholders (`—` or neutral labels) where data is unknown.
6. Validate against `references/checklist.md` before emitting.

## Output contract

One short orientation sentence, then:

```xml
<artifact identifier="field-notes-editorial" type="text/html" title="Field Notes Editorial Report">
<!doctype html>
<html>...</html>
</artifact>
```

## SenWeaverCoding strengthening

Hard requirements for every artifact this skill produces inside Designer
mode, regardless of the workflow above:

- **Bind to the active design system.** Pull palette, type, spacing, and
  radius from the selected `DESIGN.md` / `tokens.css`; never hardcode an
  off-token palette or invent a second accent.
- **Apply the craft layer.** Honour the injected craft references
  (typography, color, anti-ai-slop, state-coverage, accessibility) — they
  are P0 self-checks at the critique stage, not suggestions.
- **Write into the session directory** so the artifact appears in the
  per-session canvas; keep files self-contained (inline assets, no external
  CDNs that break in the sandboxed iframe).
- **Use real content, never filler.** No Lorem Ipsum, no invented metrics,
  no placeholder copy — compose to fill empty space.
- **Generate, don't transcribe** — the template is scaffolding; the
  shipped artifact must carry the brief's real content.

<!-- swc-strengthened -->
