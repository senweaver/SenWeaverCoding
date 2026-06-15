# Design System — Pulse (Sports & Fitness)

> Category: Sports & Fitness
> High-energy athletic system for workout trackers, training plans, and run/ride dashboards — a charcoal canvas with an electric volt accent, bold condensed display, and mono metrics. The accent reads like a power line, reserved for effort and "go" moments.

## 1. Visual Theme & Atmosphere

Pulse is kinetic. The canvas is deep charcoal (`--bg #0f1115`) stacking up to `--surface #181b21` and `--surface-warm #20242c`. The accent is an electric volt `--accent #c8ff00` — high-voltage, attention-commanding — reserved for effort metrics, records, primary CTAs, and active states. Display type is bold and condensed (`Archivo`/`Oswald`) with tight leading; metrics run in `JetBrains Mono`; labels are tight uppercase. Motion is fast and athletic (`--motion-fast 110ms`, easing `cubic-bezier(0.16,1,0.3,1)`). The mood: motivating, sweaty, alive.

**Key characteristics**
- Charcoal surfaces; volt accent used like a power line on effort/records.
- Bold condensed display (Archivo) + mono metrics + tight uppercase labels.
- Fast, kinetic motion; energetic but legible on dark.
- Big numbers — the metric is the hero of every card.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background | `--bg` | `#0f1115` | Canvas |
| Surface / warm | `--surface` / `--surface-warm` | `#181b21` / `#20242c` | Cards, raised |
| Foreground | `--fg` | `#f4f6f8` | Headings, primary text |
| Foreground 2 | `--fg-2` | `#c9cfd6` | Body |
| Muted | `--muted` | `#8b939d` | Labels, metadata |
| Border | `--border` | `#2a2f38` | Outlines |
| Accent | `--accent` | `#c8ff00` | Effort/records, CTAs, active, focus |
| Accent on | `--accent-on` | `#0f1115` | Dark text on volt |
| Success / Warn / Danger | — | `#38d96a` / `#ffb020` / `#ff4d4d` | Status |

- Volt on dark is intense — use `--accent-on #0f1115` (dark text) on accent fills.
- Ration the volt to effort/achievement; let charcoal carry the rest.

## 3. Typography

- **Display:** `Archivo, Oswald, Inter, sans-serif` (bold/condensed); **Body:** `Inter`; **Mono:** `JetBrains Mono` for metrics/splits/HR.
- **Scale:** `xs 11 / sm 13 / base 15 / lg 19 / xl 25 / 2xl 34 / 3xl 50 / 4xl 72`.
- **Line height:** body `--leading-body 1.5`, display `--leading-tight 1.0` (tight, punchy); tracking `--tracking-display 0.02em`.
- **Weights:** 400 body, 500 labels (uppercase), 700–800 condensed display. Numbers dominate — set big mono figures.

## 4. Spacing, Grid & Layout

- 8px grid: `4 / 8 / 12 / 16 / 20 / 24 / 32 / 48`; container `--container-max 1240px`; section rhythm `64 / 44 / 28px`.
- Metric-card grids: each card leads with a huge number + unit, then a tight uppercase label and a trend.

## 5. Components

- **Buttons** — `.btn-primary`: `--accent` volt fill, `--accent-on` dark text, `--radius-md 10px`; hover `--accent-hover`. `.btn-secondary`: `--surface` + 1px `--border`, `--fg` text.
- **Cards** (`.panel`, `.tile`): `--surface`, 1px `--border`, `--radius-lg 16px`, `--elev-raised`; metric value `JetBrains Mono` `--text-4xl`, label `--muted` uppercase.
- **Inputs** (`.field`): `--surface`, 1px `--border`, `--radius-sm 4px`, volt `--focus-ring`.
- **Progress/rings:** volt fill on charcoal track; show effort zones with semantic color.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `rgba(0,0,0,.45) 0 8px 24px -6px, rgba(255,255,255,.03) 0 0 0 1px` |

Dark drop with a faint top hairline; keep it athletic and clean.

## 7. Motion & Interaction

- Durations `--motion-fast 110ms` / `--motion-base 190ms`, easing `cubic-bezier(0.16,1,0.3,1)`.
- Snappy counters, ring fills, and progress animations; energetic but never sluggish. Honor `prefers-reduced-motion`.

## 8. Responsive Behavior

- Collapse metric grids to one/two columns on mobile; keep the big-number hierarchy intact.
- Maintain volt contrast and condensed display at every breakpoint; step section padding to `28px`.

## 9. Do's and Don'ts

**Do** — make the metric the hero; ration the volt to effort/records; use condensed display + mono numbers; keep motion fast.

**Don't** — flood the screen with volt; use light/pastel surfaces; bury the numbers; use slow, soft motion.

## 10. Agent Prompt Guide

- **Workout card:** `--surface` on `--bg`, `--radius-lg`, `--elev-raised`; value `JetBrains Mono` `--text-4xl` `--fg`, unit `--muted`, "PR" badge `--accent` volt; CTA `.btn-primary` volt with dark text.
- **Iteration rules:** (1) big mono metric leads; (2) volt = effort/achievement only; (3) condensed uppercase display; (4) fast 110ms kinetic motion.
