# Design System — Bento

> Category: Layout & Structure
> Modular "bento box" layout language: a tidy grid of rounded tiles of varying size, soft cool borders, balanced density, and a clear blue accent.

## 1. Visual Theme & Atmosphere

Bento composes a page like a Japanese lunchbox — a grid of self-contained rounded tiles, each holding one idea, packed at varying sizes into a satisfying whole. The field is a cool near-white (`--bg #f5f8ff`) with pure-white tiles (`--surface #ffffff`) and a soft cool secondary fill (`--surface-warm #eaf1ff`). Structure is gentle: a soft cool border (`--border #d7e0ef`) plus a wide, low-opacity lift (`--elev-raised 0 20px 52px rgba(16,24,40,0.11)`) so tiles feel placed, not boxed. Ink is a deep slate (`--fg #101828`) and the accent is a confident product blue `--accent #2563eb`.

The atmosphere is modern-SaaS: friendly, organized, scannable — ideal for feature grids, dashboards, and landing sections.

**Key characteristics**
- Variable-size tiles snapped to a consistent grid with equal gutters.
- Generous rounded corners (`--radius-sm 10 / md 16 / lg 24`) and soft cool borders.
- One idea per tile; balanced visual density across the composition.
- Wide soft elevation gives tiles a placed, tactile feel.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background | `--bg` | `#f5f8ff` | Cool canvas behind the grid |
| Surface | `--surface` | `#ffffff` | Tiles |
| Surface warm | `--surface-warm` | `#eaf1ff` | Accent/nested tiles |
| Foreground | `--fg` | `#101828` | Headings, primary text |
| Foreground 2 | `--fg-2` | `#344054` | Body |
| Muted | `--muted` | `#667085` | Captions, labels |
| Border | `--border` | `#d7e0ef` | Tile outline |
| Accent | `--accent` | `#2563eb` | CTAs, links, highlight tiles |
| Success / Warn / Danger | — | `#16a34a` / `#f59e0b` / `#ef4444` | Status |

- Use `--surface-warm` (or an `--accent`-tinted tile) to make one feature tile "pop" within the grid.
- Keep gutters equal and consistent — uneven gaps break the bento rhythm.

## 3. Typography

- **Families:** display & body `Inter, system-ui, sans-serif`; mono `"SF Mono"`.
- **Scale:** `xs 12 / sm 14 / base 16 / lg 18 / xl 24 / 2xl 36 / 3xl 54 / 4xl 76`.
- **Line height:** body `1.52`, display `--leading-tight 1.06`; tracking `--tracking-display -0.025em` at large sizes.
- **Weights:** 400 body, 500 labels, 600–700 tile titles. Each tile gets a compact title + short supporting line.

## 4. Spacing, Grid & Layout

- Spacing `4 / 8 / 12 / 16 / 20 / 24 / 32 / 48`; container `--container-max 1180px`; section rhythm `96 / 68 / 48px`.
- Build on a 12-column grid; tiles span 4/6/8/12 columns and 1–2 rows. Gutters ~16–24px, equal in both axes.
- Mix one large hero tile with several smaller supporting tiles for an engaging asymmetric-but-ordered grid.

## 5. Components

- **Tiles** (`.tile`, `.panel`): `--surface`, 1px `--border`, `--radius-lg 24px`, `--elev-raised`; hover lifts/saturates border toward `--accent`.
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on #ffffff`, `--radius-md 16px`. `.btn-secondary`: `--surface` + 1px `--border`.
- **Inputs** (`.field`): `--surface`, `--radius-sm 10px`, focus `--focus-ring 0 0 0 4px rgba(37,99,235,0.22)`.
- **Metrics** (`.metric`): big `--text-3xl` figure + `--muted` label inside a tile.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` — tile outline |
| Raised | `--elev-raised` | `0 20px 52px rgba(16,24,40,0.11)` — placed tile |

Tiles combine the cool ring with a wide soft lift. Increase lift slightly on hover for interactivity.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` / `--motion-base 240ms`, easing `cubic-bezier(0.2,0,0,1)`.
- On hover, lift the tile and tint its border toward `--accent`; keep grid positions stable.

## 8. Responsive Behavior

- Collapse the 12-column grid to 2 columns on tablet, 1 column on phone; large tiles become full-width.
- Preserve equal gutters and `--radius-lg` corners at every breakpoint; step section padding down.

## 9. Do's and Don'ts

**Do** — keep one idea per tile; use equal gutters; round corners generously; spotlight a single feature tile with tint.

**Don't** — overfill tiles with mixed content; use uneven gaps; drop the soft elevation (tiles look pasted-on); let the grid become a plain stacked list with no rhythm.

## 10. Agent Prompt Guide

- **Feature grid:** `--bg #f5f8ff`; tiles `--surface`, 1px `--border`, `--radius-lg 24px`, `--elev-raised`, 20px gutters on a 12-col grid; one hero tile spans 8 cols with `--surface-warm`.
- **Iteration rules:** (1) one idea per tile; (2) equal gutters, grid-snapped; (3) generous rounding + soft lift; (4) blue accent for the spotlight tile.
