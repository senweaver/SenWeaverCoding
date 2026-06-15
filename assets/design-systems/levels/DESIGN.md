# Design System — Levels

> Category: Layout & Structure
> Layered depth UI: stacked z-planes, clear elevation tiers, and structured overlap.

## 1. Visual Theme & Atmosphere

Levels design organizes content into clear depth tiers — stacked planes, deliberate elevation steps, and structured overlap that makes hierarchy spatial and legible.

**Key characteristics**
- Clear stacked z-planes
- Deliberate elevation tiers
- Structured intentional overlap
- Spatial, legible hierarchy

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#fbf7ef` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `#eef7ed` |
| Foreground | `--fg` | `#1f2a24` |
| Foreground 2 | `--fg-2` | `#435147` |
| Muted | `--muted` | `#788276` |
| Border | `--border` | `#dbe3d7` |
| Accent | `--accent` | `#2f8f46` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#16a34a` / `#d97706` / `#dc2626` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `Inter, system-ui, sans-serif`
- **Body:** `Inter, system-ui, sans-serif`
- **Mono:** `"SF Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 18px / xl 24px / 2xl 34px / 3xl 48px / 4xl 66px`.
- **Line height:** body `--leading-body 1.56`, display `--leading-tight 1.08`; display tracking `--tracking-display -0.02em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Inter for display, set running text in Inter, and use SF Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1160px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 96px`, tablet `68px`, phone `48px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 18px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(47, 143, 70, 0.24)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 28px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 12px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 20px 48px rgba(31, 42, 36, 0.11)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 160ms` (hover/focus) and `--motion-base 240ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Use clear elevation tiers.
- Overlap planes deliberately.
- Make hierarchy spatial.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't flatten everything to one plane.
- Don't overlap randomly.
- Don't blur the depth tiers.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #fbf7ef` canvas; `.eyebrow` uppercase `--muted` → headline Inter 600 `--text-4xl` (`--leading-tight`, tracking `-0.02em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #2f8f46`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 28px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) clear stacked z-planes; (2) accent `#2f8f46` drives interaction; (3) keep type in Inter/Inter; (4) honor the spacing + radius scale exactly.
