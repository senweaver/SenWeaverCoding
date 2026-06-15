# Design System — Counsel (Legal & Professional)

> Category: Legal & Professional
> Authoritative system for law firms, advisory, and compliance products — deep navy ink, parchment surfaces, a serif display voice, and restrained geometry. Trust signaled through typographic discipline, not color or motion.

## 1. Visual Theme & Atmosphere

Counsel reads as established and discreet. Surfaces are clean white with a parchment alternate (`--surface-warm #f4f2ec`) for letterhead-like sections. Ink is a deep authoritative navy (`--fg #16213a`); the accent is a confident navy `--accent #1a3a6b` reserved for primary actions and links. Display type is serif (`Lora`) to convey heritage and credibility, while body runs in `Inter` for screen legibility. Geometry is restrained — small radii (`--radius-sm 2px`, `--radius-md 4px`) — and motion is measured. Generous whitespace and disciplined typographic hierarchy do the persuading.

**Key characteristics**
- Serif display (Lora) + sans body (Inter): heritage headline, legible copy.
- Deep navy accent; parchment alternate surface for formal sections.
- Restrained near-square geometry and minimal elevation.
- Trust through whitespace and typographic discipline, not color or animation.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background / Surface | `--bg` / `--surface` | `#ffffff` | Canvas, document surfaces |
| Surface warm | `--surface-warm` | `#f4f2ec` | Parchment sections, sidebars |
| Foreground | `--fg` | `#16213a` | Headlines, body |
| Foreground 2 | `--fg-2` | `#2c3650` | Secondary text |
| Muted | `--muted` | `#5a6478` | Metadata, citations |
| Border | `--border` | `#d9dde4` | Rules, dividers |
| Accent | `--accent` | `#1a3a6b` | Primary actions, links |
| Success / Warn / Danger | — | `#2f7d4f` / `#9a6b16` / `#a32f2f` | Status only |

- Keep color sober and minimal; the navy accent and ink carry the brand.
- Use `--surface-warm` parchment for formal blocks (engagement letters, terms, profiles).

## 3. Typography

- **Display:** `Lora, "Tiempos", Georgia, serif`; **Body:** `Inter`; **Mono:** `ui-monospace` for citations/clause numbers.
- **Scale:** `xs 12 / sm 14 / base 16 / lg 20 / xl 25 / 2xl 32 / 3xl 44 / 4xl 58`.
- **Line height:** body `--leading-body 1.6`, display `--leading-tight 1.18`; tracking `--tracking-display 0.005em` (a hair open, formal).
- **Weights:** 400 body, 500 labels, 600 serif headings. Long-form readability is paramount; keep a constrained measure.

## 4. Spacing, Grid & Layout

- 8px grid: `4 / 8 / 12 / 16 / 20 / 24 / 32 / 48`; container `--container-max 1100px`; section rhythm `80 / 56 / 36px`.
- Constrain running text to a comfortable measure; use hairline `--border` rules and whitespace to structure dense documents.

## 5. Components

- **Buttons** — `.btn-primary`: `--accent` navy fill, `--accent-on #ffffff`, `--radius-md 4px`; hover `--accent-hover`. `.btn-secondary`: `--surface` + 1px `--border`, `--accent` text. A discreet gold may be used inline for emphasis sparingly.
- **Cards** (`.panel`, `.tile`): `--surface` or parchment `--surface-warm`, 1px `--border`, `--radius-lg 8px`, minimal `--elev-raised`.
- **Inputs** (`.field`): `--surface`, 1px `--border`, `--radius-sm 2px`, clear labels; focus `--focus-ring 0 0 0 3px rgba(26,58,107,0.35)`.
- **Tables/clauses:** hairline-ruled rows, mono clause numbers, generous row padding.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` — default |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `rgba(22,33,58,.04) 0 1px 2px, rgba(22,33,58,.06) 0 6px 18px -6px` |

Mostly flat with rules and whitespace; reserve subtle elevation for key cards.

## 7. Motion & Interaction

- Durations `--motion-fast 160ms` / `--motion-base 240ms`, easing `cubic-bezier(0.2,0,0,1)`.
- Measured, understated transitions; nothing playful. The interface should feel composed and deliberate.

## 8. Responsive Behavior

- Stack document sidebars below the main column on mobile; preserve the reading measure and serif hierarchy.
- Step section padding to `36px`; keep hairline rules crisp at every breakpoint.

## 9. Do's and Don'ts

**Do** — use serif display + sans body; keep geometry restrained; rely on whitespace and rules; keep the palette sober.

**Don't** — use bright/playful color; round corners heavily; add expressive motion; crowd long-form text.

## 10. Agent Prompt Guide

- **Practice hero:** white `--bg`, serif `Lora` headline `--text-4xl` `--fg` tracking `0.005em`; standfirst `.lead` `--fg-2` `--text-xl`; `.btn-primary` navy.
- **Iteration rules:** (1) Lora display, Inter body; (2) navy accent, sober palette; (3) restrained 2–8px radii; (4) discipline and whitespace over decoration.
