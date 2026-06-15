# Design System — Minimal

> Category: Modern & Minimal
> Stripped-back product language built on white space, near-black ink, hairline borders, and quiet interaction. Clarity is the only ornament.

## 1. Visual Theme & Atmosphere

Minimal is restraint as a feature, not an absence of decisions. The canvas is pure white (`--bg #ffffff`) with a faintly cooler surface tier (`--surface #fafafa`, `--surface-warm #f5f5f5`) so panels read as a whisper above the page rather than a box drawn on top of it. Ink is near-black `--fg #111111` — never pure `#000` — which softens contrast just enough to feel composed instead of harsh. Structure comes from hairline borders (`--border #e2e2e2`, `--border-soft #eeeeee`) and generous spacing, not from fills, gradients, or heavy shadows.

The accent is monochrome by design: `--accent #111111` on `--accent-on #ffffff`. The interface signals importance through hierarchy, weight, and position — not color. Color is reserved almost entirely for state semantics (`--success #168a46`, `--warn #b7791f`, `--danger #c53030`).

**Key characteristics**
- Pure-white canvas, near-black ink, achromatic accent — color is functional, never decorative.
- Hairline 1px borders (`--border`) carry all structure; shadows stay flat or a single soft ring.
- Tight display tracking (`--tracking-display -0.02em`) on a calm Inter type system.
- Whitespace is the primary layout tool; `--section-y-desktop 112px` gives sections room to breathe.

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference them with `var(--*)`.

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background | `--bg` | `#ffffff` | Page canvas |
| Surface | `--surface` | `#fafafa` | Cards, panels, inputs |
| Surface warm | `--surface-warm` | `#f5f5f5` | Nested / secondary fills |
| Foreground | `--fg` | `#111111` | Headings, primary text, accent |
| Foreground 2 | `--fg-2` | `#3a3a3a` | Body copy, lead text |
| Muted | `--muted` | `#777777` | Captions, metadata, placeholders |
| Border | `--border` | `#e2e2e2` | Dividers, card outlines |
| Border soft | `--border-soft` | `#eeeeee` | Inner separators |
| Accent | `--accent` | `#111111` | Primary buttons, focus, active nav |
| Accent on | `--accent-on` | `#ffffff` | Text on accent |
| Success / Warn / Danger | `--success` / `--warn` / `--danger` | `#168a46` / `#b7791f` / `#c53030` | Status only |

- Use `--accent` (#111111) for CTA emphasis and `--accent-hover` / `--accent-active` for state.
- Keep large fills on `--surface`; reserve `--surface-warm` for nesting one level deeper.
- Never introduce off-palette hues when a token already solves the need.

## 3. Typography

- **Families:** display & body both `Inter, system-ui, sans-serif` (`--font-display`, `--font-body`); mono `"SF Mono", ui-monospace, Menlo, monospace`.
- **Scale:** `--text-xs 12 / sm 14 / base 16 / lg 18 / xl 22 / 2xl 32 / 3xl 48 / 4xl 64`.
- **Line height:** body `--leading-body 1.55`, display `--leading-tight 1.08`.
- **Tracking:** apply `--tracking-display -0.02em` at `--text-2xl` and above; body stays at normal tracking.
- **Weights:** 400 body, 500 UI/labels, 600 headings. Avoid 700+ — hierarchy comes from size and space, not heaviness.
- Use `.eyebrow` (uppercase `--muted`, `--text-sm`) above headings and `.lead` (`--fg-2`, `--text-lg–xl`) for intros.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `--space-1 4 / 2 8 / 3 12 / 4 16 / 5 20 / 6 24 / 8 32 / 12 48`.
- **Container:** `--container-max 1120px`; gutters `36 / 24 / 16` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 112px`, `--section-y-tablet 80px`, `--section-y-phone 56px`.
- Prefer a single readable column with obvious hierarchy: eyebrow → headline → lead → primary action.
- Separate concerns with whitespace first, a hairline border second, elevation last.

## 5. Components

Component vocabulary mirrors the bundled fixture (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.metric`, `.eyebrow`, `.lead`).

- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, `--radius-md 4px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 3px rgba(17,17,17,0.18)`.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, `--radius-sm 2px`, clear `<label>`, explicit focus and error states; placeholders use `--muted`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, 1px `--border`, `--radius-lg 8px`, `--elev-flat` by default; lift to `--elev-raised 0 12px 30px rgba(0,0,0,0.08)` only on hover or for the single hero panel.
- **Badges / status** (`.status`): text-led with a small leading dot; semantic color only.
- **Metrics** (`.metric`): large `--text-3xl/4xl` figure in `--fg` over a `--muted` label.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` — the default for almost everything |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` — hairline structure |
| Raised | `--elev-raised` | `0 12px 30px rgba(0,0,0,0.08)` — reserved, single soft lift |

Depth is rationed. Most surfaces are flat; one element per view may rise. Never stack heavy shadows.

## 7. Motion & Interaction

- Durations: `--motion-fast 140ms` (hover/focus), `--motion-base 220ms` (entrances), easing `--ease-standard cubic-bezier(0.2,0,0,1)`.
- Transition color/border/shadow, not layout. Keep movement purposeful and short.
- Every interactive element defines hover, focus-visible (`--focus-ring`), active, disabled, and loading states explicitly.

## 8. Responsive Behavior

- Single column scales gracefully; collapse multi-column metric/tile grids to one column under ~640px.
- Step section padding down the `--section-y-*` ladder; reduce gutters to `--container-gutter-phone 16px`.
- Maintain the 4px spacing rhythm at every breakpoint; never introduce ad-hoc offsets.

## 9. Do's and Don'ts

**Do** — use `--fg #111111` (not pure black); let whitespace separate before borders; keep the accent achromatic; ration elevation to one raised element per view.

**Don't** — add gradients, glows, or decorative color; flatten hierarchy by using one size/weight for all text; use weight 700+ on body; draw boxes where space would do.

## 10. Agent Prompt Guide

- **Hero:** white `--bg`. `.eyebrow` uppercase `--muted` 14px → headline Inter 600 `--text-4xl` `--leading-tight` tracking `-0.02em` `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent`) + `.btn-secondary` (`--surface`, 1px `--border`).
- **Card:** `--surface` fill, 1px `--border`, `--radius-lg`, flat; title `--fg` 600, body `--fg-2`, divider `--border-soft`.
- **Iteration rules:** (1) color is functional, never decorative; (2) one raised element per view; (3) tracking `-0.02em` only at 32px+; (4) weights 400/500/600 only.
