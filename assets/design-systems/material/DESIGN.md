# Design System — Material

> Category: Professional & Corporate
> Material Design surface logic: clean white cards on a tinted canvas, soft tonal elevation, rounded controls, Roboto/Google Sans type, and an accessible blue accent.

## 1. Visual Theme & Atmosphere

Material treats the UI as sheets of digital paper at different elevations. A faintly tinted canvas (`--bg #f8fafd`) carries pure-white cards (`--surface #ffffff`) with a tonal-blue secondary fill (`--surface-warm #e8f0fe`). Depth is communicated by soft, neutral elevation — `--elev-raised 0 3px 8px rgba(60,64,67,0.18)` — rather than borders; the `--border #dadce0` hairline is a quiet assist, not the primary structure. Ink follows Google's grays (`--fg #202124`, `--fg-2 #3c4043`, `--muted #5f6368`) and the accent is the accessible Google blue `--accent #1a73e8`.

The result is calm, trustworthy, and highly legible — engineered for dense product UIs and dashboards.

**Key characteristics**
- Tonal elevation (soft neutral shadow) encodes hierarchy; cards float on a tinted canvas.
- Roboto / Google Sans type system with a steady 1.5 body rhythm.
- Accessible blue accent with AA-safe state colors.
- Moderate radii (`--radius-sm 4 / md 12 / lg 24`) and an 8px spacing grid.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background | `--bg` | `#f8fafd` | Tinted canvas |
| Surface | `--surface` | `#ffffff` | Cards, sheets, dialogs |
| Surface warm | `--surface-warm` | `#e8f0fe` | Selected/secondary tonal fill |
| Foreground | `--fg` | `#202124` | Headings, primary text |
| Foreground 2 | `--fg-2` | `#3c4043` | Body |
| Muted | `--muted` | `#5f6368` | Captions, secondary |
| Border | `--border` | `#dadce0` | Hairline dividers/outlines |
| Accent | `--accent` | `#1a73e8` | Buttons, links, focus, selection |
| Success / Warn / Danger | — | `#188038` / `#f9ab00` / `#d93025` | Status |

- Prefer elevation over borders to separate surfaces; use `--border` only for outlined variants.
- `--surface-warm` marks selection/active rows (tonal highlight), with `--accent` for the control itself.

## 3. Typography

- **Display:** `"Google Sans", Roboto, Arial, sans-serif`; **Body:** `Roboto, Arial, sans-serif`; **Mono:** `"Roboto Mono"`.
- **Scale:** `xs 12 / sm 14 / base 16 / lg 18 / xl 24 / 2xl 32 / 3xl 48 / 4xl 64`.
- **Line height:** body `--leading-body 1.5`, display `--leading-tight 1.12`; tracking `0`.
- **Weights:** 400 body, 500 medium (buttons/labels), 500–700 headings. Roboto Medium is the workhorse for controls.

## 4. Spacing, Grid & Layout

- 8px grid: `4 / 8 / 12 / 16 / 20 / 24 / 32 / 48`; container `--container-max 1200px`; section rhythm `96 / 68 / 48px`.
- Use consistent card padding (16–24px) and a 4/8/12-column responsive grid for product layouts.

## 5. Components

- **Buttons** — `.btn-primary` (filled): `--accent` fill, `--accent-on #ffffff`, `--radius-pill` or `--radius-md 12px`, Roboto Medium; hover `--accent-hover` + subtle elevation. `.btn-secondary` (outlined/text): `--accent` text, 1px `--border` or none.
- **Cards** (`.panel`, `.tile`): `--surface`, `--radius-lg 24px`, `--elev-raised`; raise elevation on hover.
- **Inputs** (`.field`): filled or outlined; label floats; focus uses `--accent` underline/outline + `--focus-ring 0 0 0 4px rgba(26,115,232,0.24)`.
- **Chips/badges** (`.status`): tonal `--surface-warm` fill with `--accent`/semantic text, `--radius-pill`.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` — canvas |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` — outlined variant |
| Raised | `--elev-raised` | `0 3px 8px rgba(60,64,67,0.18)` — resting card |

Higher elevation = higher priority / closer to user. Animate elevation on press and hover.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` / `--motion-base 250ms`, easing `cubic-bezier(0.2,0,0,1)` (standard Material curve).
- Use ripple/state-layer feedback on press; transition elevation and tonal state layers, not layout.

## 8. Responsive Behavior

- Reflow multi-column grids to single column under ~640px; bottom-anchor primary actions on mobile.
- Maintain the 8px grid and card padding at every breakpoint; step section padding down the `--section-y-*` ladder.

## 9. Do's and Don'ts

**Do** — encode hierarchy with elevation; use Roboto Medium for controls; keep the blue accent AA-accessible; use tonal `--surface-warm` for selection.

**Don't** — rely on heavy borders instead of elevation; mix non-Roboto display fonts; use low-contrast text; over-elevate every surface (flatten the hierarchy).

## 10. Agent Prompt Guide

- **Card:** `--surface #ffffff` on `--bg #f8fafd`, `--radius-lg 24px`, `--elev-raised`. Title Google Sans `--fg`, body Roboto `--fg-2`.
- **Filled button:** `--accent #1a73e8`, white text, pill radius, Roboto Medium, ripple on press.
- **Iteration rules:** (1) elevation over borders; (2) Roboto/Google Sans only; (3) accent is AA blue; (4) 8px grid everywhere.
