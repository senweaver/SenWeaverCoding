# Design System — Editorial

> Category: Creative & Artistic
> Magazine-grade publishing language: warm paper surfaces, large serif display type, long-form reading comfort, and composed story cards with a clay-orange accent.

## 1. Visual Theme & Atmosphere

Editorial designs for the reader. Surfaces evoke fine paper — `--bg #fbf7f0` with story cards in a brighter `--surface #fffdf8` and pull-quotes/sidebars in a warmer `--surface-warm #f1e6d6`. Display type is unapologetically serif (`--font-display: Georgia, "Times New Roman", serif`) at billboard scale, while body copy runs in a comfortable reading serif (`--font-body: "Source Serif Pro", Georgia, serif`) at a larger-than-usual base (`--text-base 18px`) with a roomy `--leading-body 1.65`. Ink is a warm near-black `--fg #1f1a16`; the accent is a restrained clay-orange `--accent #9a5a2f` for links, kickers, and rules.

The mood is literary and considered: white space, strong typographic hierarchy, and rhythm carry the page; chrome stays minimal.

**Key characteristics**
- Serif display + serif body; long-form reading is the priority.
- Larger base size (18px) and generous line height (1.65) for sustained reading.
- Warm paper palette with a single clay-orange accent for editorial marks.
- Tall section rhythm (`--section-y-desktop 112px`) and a measured ~680px reading column.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background | `--bg` | `#fbf7f0` | Paper canvas |
| Surface | `--surface` | `#fffdf8` | Story cards, article body |
| Surface warm | `--surface-warm` | `#f1e6d6` | Pull-quotes, sidebars, kickers |
| Foreground | `--fg` | `#1f1a16` | Headlines, body |
| Foreground 2 | `--fg-2` | `#4b4038` | Captions, deck/standfirst |
| Muted | `--muted` | `#7d7168` | Bylines, metadata |
| Border | `--border` | `#ded3c5` | Rules, dividers |
| Accent | `--accent` | `#9a5a2f` | Links, kickers, section rules |
| Success / Warn / Danger | — | `#4f8a4f` / `#c9822f` / `#b33a3a` | Status |

- Use `--accent` for kickers/eyebrows and inline links (with a refined underline), not as a button-spam color.
- Hairline `--border` rules separate sections; let whitespace do most of the work.

## 3. Typography

- **Display:** `Georgia, "Times New Roman", serif`, tracking `--tracking-display -0.02em`, `--leading-tight 1`.
- **Body:** `"Source Serif Pro", Georgia, serif`; **Mono:** `"IBM Plex Mono"` for captions/credits.
- **Scale:** `xs 12 / sm 14 / base 18 / lg 21 / xl 30 / 2xl 44 / 3xl 66 / 4xl 92`.
- **Hierarchy:** kicker (`.eyebrow`, uppercase `--accent` 14px) → headline (serif `--text-3xl/4xl`) → standfirst (`.lead`, `--fg-2` `--text-xl`) → body (`--text-base 18px`, 1.65).
- Drop caps, small caps for bylines, and hanging punctuation are encouraged where appropriate.

## 4. Spacing, Grid & Layout

- Spacing `4 / 8 / 12 / 16 / 20 / 24 / 32 / 48`; container `--container-max 1120px`; section rhythm `112 / 80 / 56px`.
- Constrain running text to a ~640–680px measure; allow figures, pull-quotes, and full-bleed images to break the column.

## 5. Components

- **Story cards** (`.panel`, `.tile`): `--surface`, hairline `--border`, `--radius-lg 12px`, `--elev-raised 0 20px 50px rgba(31,26,22,0.12)` for featured stories.
- **Pull-quote:** `--surface-warm`, large serif, `--accent` rule or quotation mark.
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on #ffffff`, `--radius-md 8px` (sparingly). `.btn-secondary`: text-style link with `--accent` underline.
- **Inputs** (`.field`): `--surface`, hairline `--border`, focus `--focus-ring 0 0 0 4px rgba(154,90,47,0.24)`.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` — the page default |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` — card outline |
| Raised | `--elev-raised` | `0 20px 50px rgba(31,26,22,0.12)` — featured story only |

Print sensibility: mostly flat, with rules and whitespace for structure; reserve elevation for hero features.

## 7. Motion & Interaction

- Durations `--motion-fast 180ms` / `--motion-base 280ms`, easing `cubic-bezier(0.22,1,0.36,1)`.
- Quiet, refined transitions (link underlines, gentle image reveals); never let motion distract from reading.

## 8. Responsive Behavior

- Scale serif display down a step on mobile while keeping `--leading-tight`; preserve the comfortable reading measure.
- Full-bleed figures stay edge-to-edge; sidebars stack below the main column; step section padding to `56px`.

## 9. Do's and Don'ts

**Do** — use serif display and body; keep a constrained reading measure; deploy the clay accent for kickers and links; let whitespace and rules structure the page.

**Don't** — switch body to sans-serif; run text edge-to-edge across the full width; over-elevate cards; spam the accent as button color everywhere.

## 10. Agent Prompt Guide

- **Article hero:** paper `--bg`, kicker `.eyebrow` uppercase `--accent` → serif headline `--text-4xl` `--fg` tracking `-0.02em` → standfirst `.lead` `--fg-2` `--text-xl` → byline `--muted` IBM Plex Mono.
- **Iteration rules:** (1) serif everywhere; (2) 18px base, 1.65 line height; (3) ~680px measure; (4) clay accent for editorial marks only.
