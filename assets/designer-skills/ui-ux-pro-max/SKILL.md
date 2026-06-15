---
name: ui-ux-pro-max
description: |
  Catalog-only UI/UX Pro Max entry. The full upstream templates, data, and search workflow are not bundled in SenWeaverCoding.
triggers:
  - "ui ux patterns"
  - "design patterns"
  - "ux heuristics"
  - "usability"
od:
  mode: design-system
  category: design-systems
---

# ui-ux-pro-max

## What it does

Catalog-only UI/UX Pro Max entry. The full upstream templates, data, and search workflow are not bundled in SenWeaverCoding.

## Current SenWeaverCoding scope

SenWeaverCoding currently ships this entry as discovery metadata only. If this `SKILL.md`
is the only file under `skills/ui-ux-pro-max/`, the upstream UI/UX Pro Max
workflow is not available locally.

The full upstream workflow expects additional assets such as the searchable
`data` CSVs, the `scripts/search.py` helper, reference material, templates, and
related upstream skill instructions. Without those files, do not tell users that
the full UI/UX Pro Max pattern library or template search is active.

## Metadata

- Category: `design-systems`

## How to use in SenWeaverCoding

This skill is embedded in SenWeaverCoding and injected into the Designer
system prompt when it matches the active sub-mode. Invoke it by name
(`ui-ux-pro-max`) or via one of its trigger phrases. Its bundled templates,
checklists, and reference files are read on demand with the
`designer_skill_read` tool (`id: ui-ux-pro-max`, `path: <relative file>`) —
call `designer_skill_read` with `path: SKILL.md` first if you need the
full body, then pull only the side files a given step requires.

Production flow:

1. Use this as design intelligence layered on top of the active
   `DESIGN.md`: it informs palette, type, spacing, and component
   decisions for whatever artifact the current sub-mode produces.
2. Never invent a second accent or off-token palette — extend the active
   design system, don't replace it.

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
- **Stay on-token** — refine or extend the active design system rather than
  overriding it.

<!-- swc-strengthened -->
