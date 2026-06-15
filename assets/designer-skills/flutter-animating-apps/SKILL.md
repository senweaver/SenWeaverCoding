---
name: flutter-animating-apps
description: |
  Implement animated effects, transitions, and motion in Flutter apps. Useful for native iOS/Android motion design.
triggers:
  - "flutter animation"
  - "flutter motion"
  - "mobile animation"
  - "flutter transitions"
od:
  mode: prototype
  category: animation-motion
---

# flutter-animating-apps

## What it does

Implement animated effects, transitions, and motion in Flutter apps. Useful for native iOS/Android motion design.

## Metadata

- Category: `animation-motion`

## How to use in SenWeaverCoding

This skill is embedded in SenWeaverCoding and injected into the Designer
system prompt when it matches the active sub-mode. Invoke it by name
(`flutter-animating-apps`) or via one of its trigger phrases. Its bundled templates,
checklists, and reference files are read on demand with the
`designer_skill_read` tool (`id: flutter-animating-apps`, `path: <relative file>`) —
call `designer_skill_read` with `path: SKILL.md` first if you need the
full body, then pull only the side files a given step requires.

Production flow:

1. **Discovery** — restate the brief, target platform, and the single
   primary action of the screen.
2. **Plan** — choose the active design system and name the one dominant
   entry point; list required states (loading/empty/error/populated/edge).
3. **Generate** — emit a self-contained HTML artifact into the session's
   designer directory (one file, inline CSS, no external CDNs) so the
   canvas can render it immediately.
4. **Critique** — render it, screenshot it, and self-check the P0
   anti-ai-slop rules before declaring it done.

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
- **Ship every interactive state**, not just the populated one, and make
  the design responsive and keyboard-operable.

<!-- swc-strengthened -->
