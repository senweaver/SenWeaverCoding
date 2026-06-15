# Design System — Terra (Sustainability & Environment)

> Category: Sustainability & Environment
> Earthy system for climate, impact-reporting, and conscious-commerce products — recycled-paper warmth, a grounded forest-green accent, clay/soil neutrals, and a calm serif voice. Credibility through restraint, never greenwashed neon.

## 1. Visual Theme & Atmosphere

Terra feels honest and natural. Surfaces are recycled-paper warm (`--bg #f6f4ee`, `--surface #fffdf8`) with a clay alternate (`--surface-warm #ece7da`) — never stark white. Ink is a deep forest-soil (`--fg #25321f`); the accent is a grounded forest green `--accent #2f6b3f`, calm rather than electric. Display type is a warm serif (`Fraunces`/`Lora`) for an unhurried, editorial voice; body is `Inter` at a comfortable `--text-base 17px`. Geometry is soft and organic; elevation and motion are gentle. The system signals environmental credibility through warmth and restraint, not loud "eco" neon green.

**Key characteristics**
- Recycled-paper warm surfaces; clay/soil neutrals; no stark white.
- Grounded forest-green accent — calm and credible, never neon.
- Warm serif display (Fraunces) + comfortable 17px body for trust.
- Soft organic radii, gentle elevation, unhurried motion.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background | `--bg` | `#f6f4ee` | Recycled-paper canvas |
| Surface / warm | `--surface` / `--surface-warm` | `#fffdf8` / `#ece7da` | Cards, grouped sections |
| Foreground | `--fg` | `#25321f` | Headings, body |
| Foreground 2 | `--fg-2` | `#3c4a32` | Secondary text |
| Muted | `--muted` | `#6b7561` | Labels, metadata |
| Border | `--border` | `#d8d2c2` | Dividers, card edges |
| Accent | `--accent` | `#2f6b3f` | Actions, links, impact highlights |
| Success / Warn / Danger | — | `#3f8f4f` / `#b5791f` / `#b23a2e` | Status (earth-tuned) |

- Keep greens grounded; avoid bright "eco neon" — credibility comes from warmth and restraint.
- Use `--surface-warm` clay for impact-report sections and data callouts.

## 3. Typography

- **Display:** `Fraunces, Lora, Georgia, serif`; **Body:** `Inter`; **Mono:** `ui-monospace` for figures/units (CO₂, kWh).
- **Scale:** `xs 12 / sm 14 / base 17 / lg 21 / xl 26 / 2xl 34 / 3xl 46 / 4xl 60`.
- **Line height:** body `--leading-body 1.6`, display `--leading-tight 1.15`; tracking `--tracking-display -0.01em`.
- **Weights:** 400 body, 500 labels, 600 serif headings. Comfortable reading scale supports long impact narratives.

## 4. Spacing, Grid & Layout

- 8px grid: `4 / 8 / 12 / 16 / 20 / 24 / 32 / 48`; container `--container-max 1160px`; section rhythm `80 / 56 / 36px`.
- Impact dashboards: pair narrative prose with mono figures; group metrics on clay `--surface-warm` callouts.

## 5. Components

- **Buttons** — `.btn-primary`: `--accent` forest fill, `--accent-on #fffdf8`, `--radius-md 14px`; hover `--accent-hover`. `.btn-secondary`: `--surface` + 1px `--border`, `--accent` text.
- **Cards** (`.panel`, `.tile`): `--surface`, 1px `--border`, `--radius-lg 22px`, `--elev-raised`; impact metric in mono + serif label.
- **Inputs** (`.field`): `--surface`, 1px `--border`, `--radius-sm 8px`, focus `--focus-ring 0 0 0 3px rgba(47,107,63,0.4)`.
- **Data callouts:** clay `--surface-warm` blocks with mono figures and forest accent highlights.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `rgba(37,50,31,.05) 0 1px 2px, rgba(37,50,31,.08) 0 10px 28px -8px` |

Soft, low, natural — earthy shadows, never crisp tech drops.

## 7. Motion & Interaction

- Durations `--motion-fast 180ms` / `--motion-base 280ms`, easing `cubic-bezier(0.2,0,0,1)`.
- Unhurried, organic transitions; gentle growth/fill animations for impact data. Honor `prefers-reduced-motion`.

## 8. Responsive Behavior

- Stack narrative + metric callouts on mobile; keep the comfortable 17px reading base.
- Preserve warm surfaces and serif hierarchy at every breakpoint; step section padding to `36px`.

## 9. Do's and Don'ts

**Do** — use recycled-paper warmth and clay neutrals; keep greens grounded; pair serif narrative with mono figures; keep motion unhurried.

**Don't** — use stark white or neon green; over-saturate; rush the pacing; let data feel cold/clinical.

## 10. Agent Prompt Guide

- **Impact section:** `--bg` recycled paper; serif `Fraunces` headline `--text-4xl` `--fg`; metric callout on `--surface-warm` clay with mono figure + forest `--accent` highlight.
- **Iteration rules:** (1) warm paper, never stark white; (2) grounded forest green, no neon; (3) Fraunces serif + 17px body; (4) soft organic radii and gentle motion.
