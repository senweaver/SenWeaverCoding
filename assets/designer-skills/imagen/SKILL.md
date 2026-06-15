---
name: imagen
description: |
  Generate images using Google Gemini's image generation API for UI mockups, icons, illustrations, and visual assets.
triggers:
  - "gemini image"
  - "imagen"
  - "google image gen"
  - "illustration"
  - "icon"
od:
  mode: image
  category: image-generation
---

# imagen

## What it does

Generate images using Google Gemini's image generation API for UI mockups, icons, illustrations, and visual assets.

## Metadata

- Category: `image-generation`

## How to use in SenWeaverCoding

This skill is embedded in SenWeaverCoding and injected into the Designer
system prompt when it matches the active sub-mode. Invoke it by name
(`imagen`) or via one of its trigger phrases. Its bundled templates,
checklists, and reference files are read on demand with the
`designer_skill_read` tool (`id: imagen`, `path: <relative file>`) —
call `designer_skill_read` with `path: SKILL.md` first if you need the
full body, then pull only the side files a given step requires.

Production flow:

1. **Plan** the shot: subject, composition, palette (drawn from the active
   design system), aspect ratio, and count.
2. **Generate** real assets through the `media_generate` tool
   (`surface: image`) using the selected provider model — do not emit
   placeholder `<img>` tags.
3. **Present** the returned files in the canvas and label each with real
   `alt` text.

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
- **Match brand palette and style** in the generation prompt; respect the
  requested aspect ratio and count; provide `alt` text.

<!-- swc-strengthened -->
