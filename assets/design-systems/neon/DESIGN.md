# Design System — Neon

> Category: Morphism & Effects
> High-energy dark UI lit by glowing violet, electric green, and hot-pink accents — deep midnight surfaces, luminous edges, and bloom-based depth.

## 1. Visual Theme & Atmosphere

Neon is a night interface that glows. The canvas is near-black with a violet bias (`--bg #070711`), surfaces step up into deep indigo (`--surface #111126`, `--surface-warm #1e1540`), and light radiates from the accent rather than from above. The hero accent is a luminous violet `--accent #c084fc`; supporting signals are electric (`--success #39ff88`, `--warn #fff34d`, `--danger #ff4d8d`). Depth is a colored bloom — `--elev-raised 0 24px 80px rgba(192,132,252,0.22)` — and focus literally glows (`--focus-ring 0 0 0 4px rgba(192,132,252,0.32)`).

Text is near-white (`--fg #f8f7ff`) with a lavender secondary (`--fg-2 #d6ccff`); borders are dim indigo (`--border #34265e`) so the glowing elements own the spotlight. The mood is gaming/crypto/nightlife — confident, electric, futuristic.

**Key characteristics**
- Dark violet-biased surfaces; light comes from accents, not elevation shadows.
- Glow as a first-class effect: accent text/borders use `text-shadow`/`box-shadow` blooms.
- Luminous violet hero accent with electric green/yellow/pink semantics.
- Generous radii (`--radius-sm 10 / md 16 / lg 24`) and crisp near-white text for contrast.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background | `--bg` | `#070711` | Midnight canvas |
| Surface | `--surface` | `#111126` | Cards, panels |
| Surface warm | `--surface-warm` | `#1e1540` | Nested/active surfaces |
| Foreground | `--fg` | `#f8f7ff` | Headings, primary text |
| Foreground 2 | `--fg-2` | `#d6ccff` | Body |
| Muted | `--muted` | `#9d8ad4` | Captions |
| Border | `--border` | `#34265e` | Dim outlines |
| Accent | `--accent` | `#c084fc` | Glowing CTAs, links, focus |
| Accent on | `--accent-on` | `#13051f` | Text on accent |
| Success / Warn / Danger | — | `#39ff88` / `#fff34d` / `#ff4d8d` | Electric status |

- Maintain AA contrast: keep body text on `--fg`/`--fg-2`, never on dim `--muted` for long copy.
- Reserve glow for accents and focus — glowing everything kills the effect and hurts legibility.

## 3. Typography

- **Families:** display & body `Inter, system-ui, sans-serif`; mono `"SF Mono"` for code/HUD labels.
- **Scale:** `xs 12 / sm 14 / base 16 / lg 18 / xl 24 / 2xl 36 / 3xl 54 / 4xl 76`.
- **Line height:** body `1.52`, display `--leading-tight 1.06`; tracking `--tracking-display -0.025em` at large sizes.
- **Weights:** 400 body, 500 UI, 600–700 display. Consider subtle accent glow on the hero headline only.

## 4. Spacing, Grid & Layout

- Spacing `4 / 8 / 12 / 16 / 20 / 24 / 32 / 48`; container `1180px`; section rhythm `96 / 68 / 48px`.
- Let dark negative space dominate so glowing elements read as light sources; cluster the glow, isolate it.

## 5. Components

- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on #13051f` text, `--radius-md 16px`, with a soft accent box-shadow glow; hover intensifies the bloom. `.btn-secondary`: `--surface` + 1px `--border`, `--accent` text.
- **Cards / panels** (`.panel`, `.tile`): `--surface`, 1px `--border`, `--radius-lg 24px`, `--elev-raised` violet bloom; active state uses `--surface-warm` + brighter border.
- **Inputs** (`.field`): `--surface`, dim border, `--radius-sm 10px`, glowing `--focus-ring`.
- **Badges** (`.status`): electric semantic text on `--surface-warm`, optional matching glow.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` — dim outline |
| Raised | `--elev-raised` | `0 24px 80px rgba(192,132,252,0.22)` — violet bloom |

Depth is emitted light. Combine a dim ring with the violet bloom; brighten on hover/focus.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` / `--motion-base 240ms`, easing `cubic-bezier(0.2,0,0,1)`.
- Animate glow intensity and border brightness on hover/focus; subtle pulse loops are acceptable on hero accents.

## 8. Responsive Behavior

- Keep the dark field and accent glows on mobile; reduce bloom radius if performance suffers.
- Stack cards in a single column; preserve generous dark space so glows still read as light.

## 9. Do's and Don'ts

**Do** — keep surfaces dark and violet-biased; emit light from accents; ration glow to accents/focus; keep near-white text for AA contrast.

**Don't** — use light backgrounds; glow every element; place body copy on dim `--muted`; mix more than the defined electric semantics.

## 10. Agent Prompt Guide

- **Hero:** `--bg #070711`, headline Inter 700 `--text-4xl` `--fg` with subtle violet text-glow; `.btn-primary` `--accent #c084fc` with box-shadow bloom; supporting metric in `--success #39ff88`.
- **Iteration rules:** (1) light comes from accents; (2) glow only on accents/focus; (3) dark violet surfaces; (4) near-white body text always.
