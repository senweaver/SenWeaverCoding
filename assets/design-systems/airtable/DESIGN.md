# Design System — Airtable

> Category: Design & Creative
> Airtable-style productivity UI: colorful field chips, grid/table surfaces, and friendly clarity.

## 1. Visual Theme & Atmosphere

Airtable-style design blends spreadsheet power with friendly polish — colorful field/record chips, clean table and grid surfaces, rounded controls, and an approachable, organized feel.

**Key characteristics**
- Colorful field/record chips
- Clean table and grid surfaces
- Rounded friendly controls
- Organized, approachable clarity

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#ffffff` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `var(--surface)` |
| Foreground | `--fg` | `#181d26` |
| Foreground 2 | `--fg-2` | `#333333` |
| Muted | `--muted` | `rgba(4, 14, 32, 0.69)` |
| Border | `--border` | `#e0e2e6` |
| Accent | `--accent` | `#1b61c9` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#006400` / `#eab308` / `#dc2626` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Haas Groot Disp", Haas, -apple-system, system-ui, "Segoe UI", Roboto, sans-serif`
- **Body:** `Haas, -apple-system, system-ui, "Segoe UI", Roboto, sans-serif`
- **Mono:** `ui-monospace, "SF Mono", "JetBrains Mono", Menlo, Monaco, Consolas, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 20px / xl 24px / 2xl 32px / 3xl 40px / 4xl 48px`.
- **Line height:** body `--leading-body 1.35`, display `--leading-tight 1.2`; display tracking `--tracking-display 0`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Haas Groot Disp for display, set running text in Haas, and use ui-monospace for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1200px`; gutters `24px / 16px / 12px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 96px`, tablet `64px`, phone `48px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 16px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 3px color-mix(in oklab, var(--accent), transparent 70%)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 24px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 12px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 0 1px rgba(0, 0, 0, 0.32), 0 0 2px rgba(0, 0, 0, 0.08), 0 1px 3px rgba(45, 127, 249, 0.28), inset 0 0 0 0.5px rgba(0, 0, 0, 0.06)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 200ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 12px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Use colorful field chips for categories.
- Keep table/grid surfaces clean.
- Round controls for friendliness.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't use drab uncategorized tables.
- Don't crowd the grid.
- Don't feel cold or rigid.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #ffffff` canvas; `.eyebrow` uppercase `--muted` → headline Haas Groot Disp 600 `--text-4xl` (`--leading-tight`, tracking `0`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #1b61c9`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 24px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) colorful field/record chips; (2) accent `#1b61c9` drives interaction; (3) keep type in Haas Groot Disp/Haas; (4) honor the spacing + radius scale exactly.
