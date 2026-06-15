---
name: digits-fintech-swiss-template
description: |
  Swiss-grid fintech deck template in black / warm paper / neon-lime contrast.
  Use when users ask for premium data-story slides with strict modular layout,
  bold numeric cards, restrained motion, and keyboard/click navigation in one HTML file.
triggers:
  - "swiss fintech template"
  - "data-driven finance deck"
  - "neon lime editorial grid"
  - "high contrast strategy slides"
  - "数字金融瑞士风模板"
od:
  mode: template
  surface: video
  type: hyperframes
  platform: desktop
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  outputs:
    primary: index.html
    secondary:
      - template.html
      - example.html
  example_prompt: "Create a Swiss-grid fintech strategy deck with modular data cards, lime accents, and clean keyboard navigation."
  capabilities_required:
    - file_write
---

# Digits Fintech Swiss Template

A premium three-slide live-artifact template for data-storytelling in a Swiss grid language.

## Resource map

```text
digits-fintech-swiss-template/
├── SKILL.md
├── assets/
│   └── template.html
├── references/
│   └── checklist.md
└── example.html
```

## Workflow

1. Start from `assets/template.html` and keep the three-slide structure intact.
2. Replace copy and metric values while preserving card hierarchy and reading order.
3. Keep interactions:
   - Prev / Next buttons
   - keyboard navigation (`ArrowLeft` / `ArrowRight`)
   - dot navigation
4. Keep motion subtle (slide fade + tiny hover lift only).
5. Keep the file self-contained (inline CSS/JS) with no sandbox-hostile APIs.

## Output contract

Emit one concise orientation sentence and then one HTML artifact:

```xml
<artifact identifier="digits-fintech-swiss" type="text/html" title="Digits Fintech Swiss Deck">
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
