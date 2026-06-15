---
name: after-hours-editorial-template
description: |
  Luxury dark-editorial HyperFrames template for three-page cinematic storyboards,
  inspired by haute couture title cards and magazine chapter spreads. Use when the
  user asks for premium fashion-style motion pages, moody serif-led storytelling,
  or a high-end dark presentation aesthetic with rich transitions.
triggers:
  - "after hours editorial template"
  - "dark fashion hyperframes template"
  - "haute couture motion pages"
  - "magazine style cinematic slides"
  - "高级暗黑时尚风模板"
  - "高定杂志风动效"
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
  example_prompt: "Create a three-page HyperFrames editorial sequence in a dark haute-couture style: premium serif typography, magenta accent, elegant chapter transitions, and cinematic grain. Keep each page under 3 seconds."
  capabilities_required:
    - file_write
---

# After Hours Editorial Template

Produce a self-contained HTML editorial motion artifact in a dark luxury style,
with three short pages, cinematic typography, and premium transition language.

## Resource map

```text
after-hours-editorial-template/
├── SKILL.md
├── assets/
│   └── template.html
├── references/
│   └── checklist.md
└── example.html
```

## Workflow

1. Read active `DESIGN.md` and map colors, typography tone, and layout rhythm
   into CSS variables while preserving a dark editorial baseline.
2. Copy `assets/template.html` to `index.html`.
3. Keep three narrative pages in sequence; do not increase default page dwell
   above 3 seconds.
4. Preserve premium motion behavior:
   - staged text reveal hierarchy
   - chapter wipe transitions
   - ambient grain/vignette depth
   - restrained cursor-light interaction for local preview
5. Keep output single-file HTML with inline CSS and JS.
6. Avoid sandbox-hostile browser APIs (e.g. localStorage and confirm).
7. Validate with `references/checklist.md` before emitting.

## Output contract

One short orientation sentence, then:

```xml
<artifact identifier="after-hours-editorial" type="text/html" title="After Hours Editorial Template">
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
