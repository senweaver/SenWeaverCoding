# Design System — Glassmorphism

> Category: Morphism & Effects
> Translucent frosted panels floating over a luminous sky-blue field, edged with bright hairlines and lit by a cyan-violet action color.

## 1. Visual Theme & Atmosphere

Glassmorphism turns the interface into layers of frosted glass suspended over light. The background is a soft luminous blue (`--bg #eef6ff`) that shows through every panel, because surfaces are deliberately translucent: `--surface rgba(255,255,255,0.74)` and `--surface-warm rgba(238,246,255,0.72)`. The illusion only works with blur behind the glass — pair every translucent surface with `backdrop-filter: blur(20–28px)`.

Edges are defined by light, not darkness: borders are bright and semi-transparent (`--border rgba(255,255,255,0.64)`, `--border-soft rgba(255,255,255,0.38)`), giving panels a lit rim. Depth is a wide, colored bloom (`--elev-raised 0 24px 80px rgba(79,140,255,0.18)`) rather than a hard drop shadow. Ink stays deep navy (`--fg #102033`) for legibility against the airy field, and the action color is a confident azure `--accent #4f8cff`.

**Key characteristics**
- Translucent surfaces + mandatory `backdrop-filter: blur()` — the blur is the system.
- Bright, semi-transparent light-rim borders instead of dark outlines.
- Large generous radii (`--radius-sm 16 / md 24 / lg 36`) so glass reads as smooth tiles.
- Colored ambient bloom for elevation; azure accent for every interactive signal.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background | `--bg` | `#eef6ff` | Luminous canvas (place gradients/imagery behind glass) |
| Surface | `--surface` | `rgba(255,255,255,0.74)` | Frosted panels — requires backdrop-blur |
| Surface warm | `--surface-warm` | `rgba(238,246,255,0.72)` | Nested glass |
| Foreground | `--fg` | `#102033` | Headings, primary text |
| Foreground 2 | `--fg-2` | `#34465f` | Body copy |
| Muted | `--muted` | `#60708a` | Captions, metadata |
| Border | `--border` | `rgba(255,255,255,0.64)` | Lit panel rim |
| Accent | `--accent` | `#4f8cff` | CTAs, links, focus, active |
| Accent on | `--accent-on` | `#ffffff` | Text on accent |
| Success / Warn / Danger | — | `#22c55e` / `#f59e0b` / `#ef4444` | Status |

- Never set translucent surfaces without backdrop blur — they collapse into muddy fills.
- Keep text on `--fg`/`--fg-2`; translucency must never reduce body-copy contrast below AA.

## 3. Typography

- **Families:** display & body `Inter, system-ui, sans-serif`; mono `"SF Mono", ui-monospace`.
- **Scale:** `xs 12 / sm 14 / base 16 / lg 18 / xl 24 / 2xl 36 / 3xl 54 / 4xl 76`.
- **Line height:** body `1.55`, display `--leading-tight 1.04`; tracking `--tracking-display -0.025em` at large sizes.
- **Weights:** 400 body, 500 UI, 600–700 display. Crisp dark text anchors the airy surfaces.

## 4. Spacing, Grid & Layout

- Spacing `4 / 8 / 12 / 16 / 20 / 24 / 32 / 48`; container `--container-max 1180px`.
- Section rhythm `104 / 72 / 52px`. Float cards with comfortable gaps so the background shows between them.
- Layer composition: blurred imagery/gradient → glass panels → crisp content; let the field read between tiles.

## 5. Components

- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, `--radius-md 24px`, hover `--accent-hover`. `.btn-secondary`: `--surface` glass + 1px `--border` rim, `--fg` text, with backdrop blur. Focus: `--focus-ring 0 0 0 4px rgba(79,140,255,0.28)`.
- **Cards / panels** (`.panel`, `.tile`): translucent `--surface`, `backdrop-filter: blur(24px)`, 1px light `--border`, `--radius-lg 36px`, `--elev-raised` bloom.
- **Inputs** (`.field`): frosted `--surface`, `--radius-sm 16px`, bright focus rim + `--focus-ring`.
- **Badges** (`.status`): translucent tinted pills with `--radius-pill`.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` — lit rim |
| Raised | `--elev-raised` | `0 24px 80px rgba(79,140,255,0.18)` — colored bloom |

Elevation is glow, not a hard shadow. Combine the lit rim with the azure bloom for floating panels.

## 7. Motion & Interaction

- Durations `--motion-fast 180ms` / `--motion-base 280ms`; easing `--ease-standard cubic-bezier(0.22,1,0.36,1)` for a soft, gliding settle.
- Animate opacity, blur strength, and bloom on hover; avoid abrupt position jumps — glass drifts.

## 8. Responsive Behavior

- Maintain blur on mobile but reduce radius slightly; keep at least one layer of visible background between cards.
- Collapse panel grids to a single floating column; step section padding down to `52px`.
- Watch contrast on small text over translucent surfaces — increase surface opacity if needed.

## 9. Do's and Don'ts

**Do** — pair every translucent surface with `backdrop-filter: blur()`; use bright light-rim borders; let the background show through; keep azure as the single action color.

**Don't** — use translucency without blur; apply dark/hard drop shadows; place long body copy on heavily transparent fills; introduce competing accent hues.

## 10. Agent Prompt Guide

- **Hero:** luminous `--bg` with a soft blurred gradient behind. Frosted glass card: `--surface`, blur 28px, 1px `--border` rim, `--radius-lg`, bloom `--elev-raised`. Headline Inter 700 `--text-4xl` `--fg`; CTA `.btn-primary` azure.
- **Iteration rules:** (1) blur is mandatory on glass; (2) borders are light, not dark; (3) elevation = colored bloom; (4) azure `#4f8cff` is the only interactive color.
