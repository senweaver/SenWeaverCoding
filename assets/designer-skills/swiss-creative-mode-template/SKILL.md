---
name: swiss-creative-mode-template
description: |
  Swiss-inspired creative-mode presentation template skill with bold editorial
  typography, high-contrast geometric cards, interactive slide navigation,
  theme switching, hotspot overlays, and palette choreography in a single-file
  HTML artifact. Use when users ask for a premium presentation-style landing,
  a Swiss/brutalist deck look, or a creative launch page with rich interactions.
triggers:
  - "swiss creative mode template"
  - "editorial presentation template"
  - "brutalist deck style html"
  - "creative mode deck"
  - "瑞士风演示模板"
  - "高级设计语言模板"
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
  capabilities_required:
    - file_write
---

# Swiss Creative Mode Template

Produce a premium Swiss/editorial-style HTML template with strong visual rhythm
and meaningful interactions, then emit it as a single-file artifact.

## Resource map

```text
swiss-creative-mode-template/
├── SKILL.md
├── assets/
│   └── template.html
├── references/
│   └── checklist.md
└── example.html
```

## Workflow

1. Read active `DESIGN.md` and map palette/type/layout decisions into root CSS variables.
2. Copy `assets/template.html` to `index.html`.
3. Keep this structure intact:
   - Hero scene with bold title and geometric frame.
   - Four-step process card row.
   - Stack/architecture diagram scene.
4. Keep these interactions working:
   - Prev/next slide navigation + dot nav.
   - Theme toggle (paper/dark).
   - Palette cycle button (changes accent colors across the template).
   - Hotspot toggle for annotations/details.
5. Keep output self-contained (`<!doctype html>`, inline CSS/JS, no external runtime dependency).
6. Validate against `references/checklist.md` before emitting.

## Output contract

One short sentence before artifact, then:

```xml
<artifact identifier="swiss-creative-mode" type="text/html" title="Swiss Creative Mode Template">
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
