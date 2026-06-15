---
name: slides
description: |
  Create and edit .pptx presentation decks with PptxGenJS. Useful for sales decks, kickoff briefs, and design-system showcases.
triggers:
  - "slides"
  - "pptxgenjs"
  - "sales deck"
  - "design showcase deck"
od:
  mode: deck
  category: slides
---

# slides

## What it does

Create and edit .pptx presentation decks with PptxGenJS. Useful for sales decks, kickoff briefs, and design-system showcases.

## Metadata

- Category: `slides`

## How to use in SenWeaverCoding

This skill is embedded in SenWeaverCoding and injected into the Designer
system prompt when it matches the active sub-mode. Invoke it by name
(`slides`) or via one of its trigger phrases. Its bundled templates,
checklists, and reference files are read on demand with the
`designer_skill_read` tool (`id: slides`, `path: <relative file>`) —
call `designer_skill_read` with `path: SKILL.md` first if you need the
full body, then pull only the side files a given step requires.

Production flow:

1. **Plan** the narrative arc (one idea per slide) and lock the theme to
   the active design system.
2. **Generate** a 16:9 (1920×1080) self-contained `deck.html` with
   varied per-slide layouts — never the same template ten times.
3. **Critique** every slide for type hierarchy, contrast, and one bold
   visual move per section.

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
- **Vary slide layouts** and keep one clear focal point per slide; lock the
  theme to the design system end to end.

<!-- swc-strengthened -->
