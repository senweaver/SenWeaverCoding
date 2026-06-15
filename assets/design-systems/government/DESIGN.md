# Design System — Civic (Government & Public Sector)

> Category: Government & Public Sector
> Accessibility-first system for public services: government portals, benefit applications, tax forms, and civic information. Near-black ink on white, one functional blue, a high-visibility yellow focus highlight, and almost no rounding. It must work for every citizen, on every device, at every ability level.

## 1. Visual Theme & Atmosphere

Civic puts clarity and access above brand expression. Surfaces are plain white (`--bg`/`--surface #ffffff`) with a light grey alternate (`--surface-warm #f3f2f1`). Ink is near-black `--fg #0b0c0c` for maximum contrast; the single accent is a functional blue `--accent #1d70b8` for links and primary actions. The signature is the focus state: a high-visibility yellow underline highlight (`--focus-ring 0 -2px #ffdd00, 0 4px var(--fg)`) modeled on government design systems (GDS) — unmissable for keyboard users. Geometry is near-square (`--radius-sm/md 0px`), motion is minimal (`--motion-fast 0ms`, `--ease-standard linear`), and the type scale is large (`--text-base 19px`) for readability. Every decision serves comprehension and accessibility.

**Key characteristics**
- Maximum contrast: near-black ink on white, large 19px base text.
- One functional blue for links/actions; zero decorative color.
- Signature yellow focus highlight (GDS-style) — accessibility is the rule.
- Square geometry, minimal motion, generous tap/click targets.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background / Surface | `--bg` / `--surface` | `#ffffff` | Canvas, content |
| Surface warm | `--surface-warm` | `#f3f2f1` | Grouped sections, sidebars |
| Foreground | `--fg` | `#0b0c0c` | Headings, body |
| Foreground 2 | `--fg-2` | `#2b2b2b` | Secondary text |
| Muted | `--muted` | `#505a5f` | Hints, metadata (still AA) |
| Border | `--border` | `#b1b4b6` | Inputs, dividers, table rules |
| Accent | `--accent` | `#1d70b8` | Links, primary actions |
| Success / Warn / Danger | — | `#00703c` / `#f47738` / `#d4351c` | Confirmations, warnings, errors |
| Focus | `--focus-ring` | `0 -2px #ffdd00, 0 4px var(--fg)` | Yellow highlight + black underline |

- Never use color as the sole signal; always pair with text and icons.
- Keep `--muted` at AA minimum even for hints — every citizen must read everything.

## 3. Typography

- **Display & Body:** `"GDS Transport", Inter, Arial, sans-serif`; **Mono:** `ui-monospace` for reference numbers.
- **Scale (large by design):** `xs 14 / sm 16 / base 19 / lg 24 / xl 27 / 2xl 36 / 3xl 48 / 4xl 64`.
- **Line height:** body `--leading-body 1.47`, display `--leading-tight 1.09`; tracking `--tracking-display 0` (no compression).
- **Weights:** 400 body, 700 headings/labels. Plain language, one idea per line, clear question/answer form structure.

## 4. Spacing, Grid & Layout

- **5px-rooted gov grid:** `5 / 10 / 15 / 20 / 25 / 30 / 40 / 60`; container `--container-max 960px` (narrow for readability); section rhythm `60 / 40 / 30px`.
- Single-column forms with one question per step; left-aligned labels above inputs; visible required/optional and error summaries at the top.

## 5. Components

- **Buttons** — `.btn-primary`: `--success`-toned or `--accent` fill per pattern, `--accent-on #ffffff`, `--radius-md 0px` (square), large hit area; visible focus uses the yellow highlight.
- **Inputs** (`.field`): white, 2px `--border`, square `--radius-sm 0px`, large 19px text, explicit `<label>`, hint text, and inline error with `--danger` text + icon; focus `--focus-ring` yellow.
- **Cards/sections** (`.panel`): `--surface`/`--surface-warm`, 1px `--border`, minimal `--elev-raised`; mostly border-structured.
- **Error/notification banners:** strong border + semantic color + heading + list of issues linking to fields.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` — default |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `rgba(11,12,12,.12) 0 2px 6px 0` |

Structure comes from borders and spacing, not shadow. Keep it flat and unambiguous.

## 7. Motion & Interaction

- Durations `--motion-fast 0ms` / `--motion-base 120ms`, easing `linear` — minimal, functional only.
- No decorative animation. Honor `prefers-reduced-motion` fully; never animate essential content into view.

## 8. Responsive Behavior

- Single-column at all sizes; the 960px max keeps line length readable on desktop.
- Maintain large text, square inputs, and the yellow focus highlight at every breakpoint; targets stay large for touch.

## 9. Do's and Don'ts

**Do** — maximize contrast; keep text large (19px base); use the yellow focus highlight; pair color with text/icon; write in plain language; one question per step.

**Don't** — use decorative color or imagery; round inputs/buttons; rely on color alone; animate content; cram multi-column forms.

## 10. Agent Prompt Guide

- **Form step:** white `--bg`, 960px column; question as `GDS Transport` `--text-2xl` `--fg`; input white with 2px `--border`, square, 19px; yellow `--focus-ring` on focus; `.btn-primary` large with visible focus.
- **Iteration rules:** (1) accessibility first — AAA contrast, large text; (2) one functional blue, no decoration; (3) yellow focus highlight always; (4) square geometry, minimal motion.
