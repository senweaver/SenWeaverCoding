# Design System — Arcade (Gaming & Esports)

> Category: Gaming & Esports
> Dark-native system for game launchers, tournament hubs, leaderboards, and streaming overlays — a near-black canvas charged with neon magenta and cyan, glow-edged surfaces, and a confident geometric display face.

## 1. Visual Theme & Atmosphere

Arcade lives in the dark. Surfaces stack from a near-black canvas (`--bg #0a0a0f`) up through `--surface #14141f` and `--surface-warm #1c1c2b`. The hero accent is a neon magenta `--accent #ff2d78` with a cyan secondary (`#2dd4bf`, used inline) — energy as a weapon, applied to what matters (CTAs, live states, winners), never smeared across the screen. Depth is a black drop plus a magenta hairline glow (`--elev-raised`), and focus literally glows (`--focus-ring 0 0 0 2px var(--bg), 0 0 0 4px var(--accent)`). Display type is condensed geometric (`Rajdhani`/`Orbitron`) with open uppercase tracking; motion is snappy (`--motion-fast 120ms`, easing `cubic-bezier(0.16,1,0.3,1)`).

**Key characteristics**
- Dark stacked surfaces; light comes from neon accents, not elevation.
- Neon magenta hero + cyan secondary, rationed to high-stakes moments.
- Condensed geometric display (Rajdhani) with open uppercase tracking.
- Glow-edged cards and focus; snappy, high-energy motion.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background | `--bg` | `#0a0a0f` | Canvas |
| Surface / warm | `--surface` / `--surface-warm` | `#14141f` / `#1c1c2b` | Cards, raised/active |
| Foreground | `--fg` | `#f0f0fa` | Headings, primary text |
| Foreground 2 | `--fg-2` | `#c7c7da` | Body |
| Muted | `--muted` | `#8a8aa6` | Labels, metadata |
| Border | `--border` | `#2a2a3d` | Outlines |
| Accent | `--accent` | `#ff2d78` | CTAs, live/winner states, focus |
| Success / Warn / Danger | — | `#2dd4bf` / `#fbbf24` / `#ff4757` | Status (teal/amber/red) |

- Keep near-white `--fg` for legibility on dark; never run body copy on `--muted`.
- Ration neon: a glowing element should signal something important.

## 3. Typography

- **Display:** `Rajdhani, Orbitron, Inter, sans-serif`; **Body:** `Inter`; **Mono:** `JetBrains Mono` for stats/timers/scores.
- **Scale:** `xs 11 / sm 13 / base 15 / lg 19 / xl 24 / 2xl 32 / 3xl 46 / 4xl 64`.
- **Line height:** body `--leading-body 1.5`, display `--leading-tight 1.05`; tracking `--tracking-display 0.04em` (open, technical).
- **Weights:** 400 body, 500 UI, 600–700 condensed display, often uppercase for labels/headings.

## 4. Spacing, Grid & Layout

- 8px grid: `4 / 8 / 12 / 16 / 20 / 24 / 32 / 48`; container `--container-max 1320px`; section rhythm `64 / 44 / 28px`.
- HUD-like density is welcome: leaderboards, match cards, and stat panels packed but framed by dark space.

## 5. Components

- **Buttons** — `.btn-primary`: `--accent` magenta fill, `--accent-on #0a0a0f`, `--radius-md 8px`, optional glow; hover `--accent-hover`. `.btn-secondary`: `--surface` + 1px `--border`, `--accent` text.
- **Cards** (`.panel`, `.tile`): `--surface`, 1px `--border`, `--radius-lg 14px`, `--elev-raised` (black drop + magenta hairline); live cards brighten the border/glow.
- **Inputs** (`.field`): `--surface`, 1px `--border`, `--radius-sm 4px`, glowing `--focus-ring`.
- **Leaderboard/stat:** mono figures, rank chips, winner row tinted toward accent.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `rgba(0,0,0,.5) 0 8px 24px -4px, rgba(255,45,120,.25) 0 0 0 1px` |

Depth = black shadow + neon hairline. Intensify on hover/live state.

## 7. Motion & Interaction

- Durations `--motion-fast 120ms` / `--motion-base 200ms`, easing `cubic-bezier(0.16,1,0.3,1)` (snappy overshoot-free).
- Quick, punchy feedback; subtle accent pulses on live/featured elements. Respect `prefers-reduced-motion`.

## 8. Responsive Behavior

- Keep the dark field + glows on mobile; collapse leaderboards/match grids to single column.
- Preserve generous dark space around glowing elements so they still read as light.

## 9. Do's and Don'ts

**Do** — keep surfaces dark; ration neon to key moments; use condensed uppercase display; glow focus and live states.

**Don't** — use light backgrounds; glow everything; run body on `--muted`; exceed the magenta/cyan/semantic palette.

## 10. Agent Prompt Guide

- **Match card:** `--surface` on `--bg`, 1px `--border`, `--radius-lg`, `--elev-raised`; team names `Rajdhani` uppercase, score `JetBrains Mono` `--text-3xl`, "LIVE" chip `--accent` with glow.
- **Iteration rules:** (1) dark-native, accent-lit; (2) neon = important; (3) condensed uppercase display; (4) snappy 120ms motion.
