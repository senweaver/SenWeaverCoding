# Design System — Shadcn

> Category: Modern & Minimal
> Shadcn/ui aesthetic: neutral zinc surfaces, subtle borders, small radii, and tasteful restraint.

## 1. Visual Theme & Atmosphere

Shadcn design follows the modern component-library look — neutral zinc/slate surfaces, hairline borders, small consistent radii, muted foregrounds, and one accent. Calm, composable, developer-friendly.

**Key characteristics**
- Neutral zinc/slate surfaces
- Hairline borders, small consistent radii
- Muted foreground hierarchy
- Single accent, composable components

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#ffffff` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `var(--surface)` |
| Foreground | `--fg` | `#111827` |
| Foreground 2 | `--fg-2` | `var(--fg)` |
| Muted | `--muted` | `#64748b` |
| Border | `--border` | `#e5e7eb` |
| Accent | `--accent` | `#000000` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#16a34a` / `#d97706` / `#dc2626` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Geist", "Geist Sans", -apple-system, system-ui, "Segoe UI", Arial, sans-serif`
- **Body:** `"Geist", "Geist Sans", -apple-system, system-ui, "Segoe UI", Arial, sans-serif`
- **Mono:** `"Fira Code", ui-monospace, "SF Mono", "JetBrains Mono", Menlo, Monaco, Consolas, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 20px / xl 24px / 2xl 32px / 3xl 40px / 4xl 48px`.
- **Line height:** body `--leading-body 1.5`, display `--leading-tight 1.2`; display tracking `--tracking-display -0.02em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Geist for display, set running text in Geist, and use Fira Code for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1280px`; gutters `24px / 16px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 96px`, tablet `64px`, phone `48px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 8px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 2px var(--bg), 0 0 0 4px var(--accent)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 12px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 6px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 1px 2px 0 color-mix(in oklab, var(--fg), transparent 92%), 0 1px 3px 0 color-mix(in oklab, var(--fg), transparent 88%)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 200ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Use hairline borders and small radii.
- Keep neutrals calm and muted.
- Compose from consistent primitives.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't use heavy shadows or loud color.
- Don't vary radii arbitrarily.
- Don't break the composable rhythm.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #ffffff` canvas; `.eyebrow` uppercase `--muted` → headline Geist 600 `--text-4xl` (`--leading-tight`, tracking `-0.02em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #000000`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 12px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) neutral zinc/slate surfaces; (2) accent `#000000` drives interaction; (3) keep type in Geist/Geist; (4) honor the spacing + radius scale exactly.
