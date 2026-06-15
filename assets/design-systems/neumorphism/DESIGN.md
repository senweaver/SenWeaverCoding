# Design System — Neumorphism

> Category: Morphism & Effects
> Soft, monochromatic "extruded clay" UI where controls appear pressed from or into a single warm surface using paired light and dark shadows.

## 1. Visual Theme & Atmosphere

Neumorphism (new skeuomorphism) makes the interface feel molded from one continuous material. Background and surface sit only a half-step apart in tone — a warm sand field `--bg #f7eee6` with cards in a creamier `--surface #fff8f1` and recesses in `--surface-warm #ead6c7`. There are no hard edges; depth is sculpted entirely by a dual shadow: a dark warm shadow on one side and a bright highlight on the other. That signature is captured in `--elev-raised: 8px 10px 24px rgba(128,92,70,0.18), -8px -8px 20px rgba(255,255,255,0.70)` — elements look gently raised from the clay; invert the offsets to make them look pressed in.

Ink is warm brown-black (`--fg #2b211c`) and the single accent is a terracotta `--accent #b46a46`, kept low-contrast so it harmonizes with the tonal surface rather than fighting it.

**Key characteristics**
- One material, two shadows: dark warm + bright highlight on opposite sides creates extrusion.
- Surfaces stay within a tiny tonal range — contrast lives in shadow, not color.
- Large soft radii (`--radius-sm 14 / md 22 / lg 34`) so controls read as rounded pebbles.
- Borders are nearly invisible; the dual shadow does all the structural work.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background | `--bg` | `#f7eee6` | The clay field |
| Surface | `--surface` | `#fff8f1` | Raised cards/controls |
| Surface warm | `--surface-warm` | `#ead6c7` | Pressed-in recesses, tracks |
| Foreground | `--fg` | `#2b211c` | Headings, primary text |
| Foreground 2 | `--fg-2` | `#5a4b43` | Body copy |
| Muted | `--muted` | `#8a7a70` | Captions |
| Border | `--border` | `#dac8b9` | Faint rim only when shadows aren't enough |
| Accent | `--accent` | `#b46a46` | CTAs, focus, active |
| Success / Warn / Danger | — | `#4d8f5a` / `#c88735` / `#b84c4c` | Status (warm-tuned) |

- Keep background, surface, and recess within their warm tonal band — the effect breaks if contrast is too high.
- Use `--accent-on #ffffff` text on accent.

## 3. Typography

- **Families:** display & body `Inter, system-ui, sans-serif`; mono `"SF Mono", ui-monospace`.
- **Scale:** `xs 12 / sm 14 / base 16 / lg 18 / xl 24 / 2xl 36 / 3xl 54 / 4xl 76`.
- **Line height:** body `--leading-body 1.52`, display `--leading-tight 1.06`; tracking `-0.025em` at large sizes.
- Keep text on the surface (never on a strong shadow); medium weights (500–600) read best on the soft material.

## 4. Spacing, Grid & Layout

- Spacing `4 / 8 / 12 / 16 / 20 / 24 / 32 / 48`; container `1180px`; section rhythm `96 / 68 / 48px`.
- Give controls room — the dual shadow needs ~16–24px of clearance to read; never crowd extruded elements.

## 5. Components

- **Buttons** — `.btn-primary`: `--accent` fill on the clay with the raised dual shadow, `--radius-md 22px`; on press, swap to the inset (pressed-in) variant. `.btn-secondary`: `--surface` with raised shadow, `--fg` text.
- **Cards / panels** (`.panel`, `.tile`): `--surface`, `--radius-lg 34px`, raised dual shadow; no hard border.
- **Inputs** (`.field`): pressed-in `--surface-warm` recess (inverted shadow), `--radius-sm 14px`, `--focus-ring 0 0 0 4px rgba(180,106,70,0.24)`.
- **Toggles/sliders:** track is a pressed recess; thumb is a raised pebble.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` (rare) |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` — faint assist only |
| Raised | `--elev-raised` | `8px 10px 24px rgba(128,92,70,0.18), -8px -8px 20px rgba(255,255,255,0.70)` |
| Pressed | inverted raised | swap offsets to recess inputs/active controls |

The dual shadow is the entire elevation language. Raised = available; pressed = active/input.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` / `--motion-base 240ms`, easing `cubic-bezier(0.2,0,0,1)`.
- On press, animate from raised to pressed shadow — the control physically sinks into the clay.

## 8. Responsive Behavior

- Preserve shadow clearance on mobile; reduce radii one step if controls get tight.
- Collapse grids to one column; keep the warm tonal field edge-to-edge.

## 9. Do's and Don'ts

**Do** — keep surfaces within one tonal band; always use paired light+dark shadows; recess inputs; give controls breathing room.

**Don't** — use high-contrast surfaces or dark backgrounds; add hard borders or flat drop shadows; place accent text below AA contrast on the clay; crowd extruded elements.

## 10. Agent Prompt Guide

- **Card:** `--surface #fff8f1` on `--bg #f7eee6`, `--radius-lg 34px`, dual shadow `--elev-raised`. Title `--fg` 600, body `--fg-2`.
- **Input:** pressed recess in `--surface-warm`, inverted shadow, `--radius-sm`, focus `--focus-ring`.
- **Iteration rules:** (1) one material, two shadows; (2) raised vs pressed encodes state; (3) tonal contrast stays low; (4) terracotta accent only.
