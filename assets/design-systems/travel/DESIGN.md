# Design System — Atlas (Travel & Hospitality)

> Category: Travel & Hospitality
> Wanderlust system for travel, booking, and hospitality — warm sand surfaces, a sunset-coral accent, editorial serif headlines, and generous photo-forward radii. The photography leads; the interface frames it warmly.

## 1. Visual Theme & Atmosphere

Atlas is inviting and aspirational. Surfaces are warm sand (`--bg #fffdf9`, `--surface #ffffff`, `--surface-warm #fbf2e6`) rather than cold white, so destination photography feels like a sunlit window. Ink is a warm espresso (`--fg #2a2018`); the accent is a sunset coral `--accent #e0563b` (with an ocean-teal kept inline) for CTAs, prices, and highlights. Display type is an editorial serif (`Fraunces`/`Playfair Display`) for romance; body is `Inter` for clarity. Radii are generous (`--radius-lg 24px`) so image cards feel like polished travel cards, and elevation is warm and soft. Imagery leads; the chrome frames and gets out of the way.

**Key characteristics**
- Warm sand surfaces that flatter destination photography.
- Sunset-coral accent for CTAs/prices; editorial serif headlines.
- Generous photo-forward radii and warm soft elevation.
- Image-led layouts — the interface frames the trip, doesn't compete.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background | `--bg` | `#fffdf9` | Warm canvas |
| Surface / warm | `--surface` / `--surface-warm` | `#ffffff` / `#fbf2e6` | Cards, grouped sections |
| Foreground | `--fg` | `#2a2018` | Headlines, body |
| Foreground 2 | `--fg-2` | `#43372c` | Secondary text |
| Muted | `--muted` | `#7a6c5d` | Labels, metadata |
| Border | `--border` | `#ecdfce` | Dividers, card edges |
| Accent | `--accent` | `#e0563b` | CTAs, prices, highlights, focus |
| Success / Warn / Danger | — | `#2f9e6f` / `#d08a1a` / `#c4452f` | Status |

- Use coral for "Book"/price/CTA; an ocean-teal may appear inline for secondary destination accents.
- Let large imagery dominate; keep chrome warm and minimal around it.

## 3. Typography

- **Display:** `Fraunces, "Playfair Display", Georgia, serif`; **Body:** `Inter`; **Mono:** `ui-monospace` for codes/dates.
- **Scale:** `xs 12 / sm 14 / base 16 / lg 20 / xl 26 / 2xl 36 / 3xl 52 / 4xl 72`.
- **Line height:** body `--leading-body 1.58`, display `--leading-tight 1.1`; tracking `--tracking-display -0.02em`.
- **Weights:** 400 body, 500 labels, 600 serif headlines. Romantic serif headers over photography; clean sans for details/prices.

## 4. Spacing, Grid & Layout

- 8px grid: `4 / 8 / 12 / 16 / 20 / 24 / 32 / 48`; container `--container-max 1280px`; section rhythm `80 / 56 / 36px`.
- Destination/listing grids of large rounded image cards; full-bleed hero photography with a serif headline overlay.

## 5. Components

- **Buttons** — `.btn-primary`: `--accent` coral fill, `--accent-on #ffffff`, `--radius-md 16px`; hover `--accent-hover`. `.btn-secondary`: `--surface` + 1px `--border`, `--fg` text.
- **Cards** (`.panel`, `.tile`): image-topped, `--surface`, 1px `--border`, `--radius-lg 24px`, `--elev-raised`; price in `--accent`, rating + location in `--muted`.
- **Inputs** (`.field`): `--surface`, 1px `--border`, `--radius-sm 10px`, focus `--focus-ring 0 0 0 3px rgba(224,86,59,0.4)`; search/date pickers prominent.
- **Listings:** photo-forward, with coral price and clear CTA.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `rgba(42,32,24,.05) 0 2px 6px, rgba(42,32,24,.1) 0 12px 32px -8px` |

Warm, soft elevation lifts image cards off the sand; hover lifts a touch more.

## 7. Motion & Interaction

- Durations `--motion-fast 160ms` / `--motion-base 260ms`, easing `cubic-bezier(0.2,0,0,1)`.
- Gentle image zooms/reveals and card lifts on hover; nothing jarring. Honor `prefers-reduced-motion`.

## 8. Responsive Behavior

- Collapse destination grids to one column on mobile; keep hero photography full-bleed with legible serif overlay.
- Preserve warm surfaces and generous radii at every breakpoint; step section padding to `36px`.

## 9. Do's and Don'ts

**Do** — let photography lead; use warm sand surfaces; use serif headlines + coral CTAs; round image cards generously.

**Don't** — use cold white that flattens photos; bury imagery under chrome; use harsh shadows; overuse the coral beyond CTAs/prices.

## 10. Agent Prompt Guide

- **Destination card:** image top, `--surface` body, `--radius-lg 24px`, `--elev-raised`; title `Fraunces` `--fg`, price `--accent` coral, "4.8 ★ · Bali" `--muted`; CTA `.btn-primary` "Book".
- **Iteration rules:** (1) photography leads, warm sand chrome; (2) serif headlines + coral CTA/price; (3) generous photo radii; (4) gentle hover zooms/lifts.
