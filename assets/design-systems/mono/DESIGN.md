# Design System — Mono

> Category: Modern & Minimal
> Monochrome system UI: single-hue scale, monospace voice, and structural precision.

## 1. Visual Theme & Atmosphere

Mono design lives on a single-hue tonal scale with a monospace voice — technical, precise, and grid-locked. Contrast and structure replace color entirely.

**Key characteristics**
- Single-hue tonal scale
- Monospace typographic voice
- Grid-locked structural precision
- Contrast instead of color

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#ffffff` |
| Surface | `--surface` | `#f7f7f7` |
| Surface warm | `--surface-warm` | `#eeeeee` |
| Foreground | `--fg` | `#111111` |
| Foreground 2 | `--fg-2` | `#3a3a3a` |
| Muted | `--muted` | `#707070` |
| Border | `--border` | `#d9d9d9` |
| Accent | `--accent` | `#111111` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#168a46` / `#b7791f` / `#c53030` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"IBM Plex Mono", ui-monospace, monospace`
- **Body:** `"IBM Plex Mono", ui-monospace, monospace`
- **Mono:** `"IBM Plex Mono", ui-monospace, monospace`
- **Scale:** `--text-xs 11px / sm 12px / base 14px / lg 16px / xl 20px / 2xl 28px / 3xl 40px / 4xl 56px`.
- **Line height:** body `--leading-body 1.45`, display `--leading-tight 1.06`; display tracking `--tracking-display -0.025em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with IBM Plex Mono for display, set running text in IBM Plex Mono, and use IBM Plex Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1280px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 80px`, tablet `60px`, phone `42px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 8px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 3px rgba(17, 17, 17, 0.18)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 12px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 4px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 16px 40px rgba(0, 0, 0, 0.10)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 100ms` (hover/focus) and `--motion-base 180ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Stay on the monochrome scale.
- Use monospace for technical voice.
- Lock to a precise grid.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't introduce off-hue color.
- Don't mix decorative fonts.
- Don't break the grid.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #ffffff` canvas; `.eyebrow` uppercase `--muted` → headline IBM Plex Mono 600 `--text-4xl` (`--leading-tight`, tracking `-0.025em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #111111`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 12px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) single-hue tonal scale; (2) accent `#111111` drives interaction; (3) keep type in IBM Plex Mono/IBM Plex Mono; (4) honor the spacing + radius scale exactly.
