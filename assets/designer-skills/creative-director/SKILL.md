---
name: creative-director
description: |
  AI creative director with recursive self-assessment: 20+ methodologies (SIT, TRIZ, Bisociation, SCAMPER, Synectics), 3-axis evaluation calibrated against Cannes/D&AD/HumanKind, 5-phase process from brief to presentation.
triggers:
  - "creative director"
  - "campaign concept"
  - "creative critique"
  - "cannes review"
  - "scamper"
od:
  mode: design-system
  category: creative-direction
---

# creative-director

## What it does

AI creative director with recursive self-assessment: 20+ methodologies (SIT, TRIZ, Bisociation, SCAMPER, Synectics), 3-axis evaluation calibrated against Cannes/D&AD/HumanKind, 5-phase process from brief to presentation.

## SenWeaverCoding orchestration mode

When this skill is invoked inside SenWeaverCoding, treat it as the design-flow
director, not as a single polish checklist.

1. Define what "good-looking" means before changing pixels: audience, product
   goal, brand posture, style references, information density, typography,
   palette, motion tone, asset needs, and explicit anti-patterns such as
   generic AI gradients, empty cards, vague copy, and template symmetry.
2. Inspect the current target: HTML/page element, browser tab, design file,
   active design system, attached image, or project folder.
3. Search across every available SenWeaverCoding resource, not only this skill:
   skills, plugins, MCP servers and templates, connected connectors, design
   files, active browser/context, and user-provided assets.
4. Match resources into a staged workflow. Typical lanes are critique,
   style-direction selection, visual asset generation, motion, data/proof
   grounding, implementation polish, responsive/accessibility hardening, and
   final verification.
5. When the design target or aesthetic bar is ambiguous, present a small
   guided UI-style choice set or form with a recommended default. Continue the
   workflow after the choice instead of stopping at a generic question.
6. If the best resource is not configured yet, explain why it is needed and
   guide setup; otherwise use the closest configured alternative and mark the
   tradeoff.

## Metadata

- Category: `creative-direction`

## How to use in SenWeaverCoding

This skill is embedded in SenWeaverCoding and injected into the Designer
system prompt when it matches the active sub-mode. Invoke it by name
(`creative-director`) or via one of its trigger phrases. Its bundled templates,
checklists, and reference files are read on demand with the
`designer_skill_read` tool (`id: creative-director`, `path: <relative file>`) —
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
