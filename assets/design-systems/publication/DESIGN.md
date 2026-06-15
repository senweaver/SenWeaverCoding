# Design System — Publication

> Category: Creative & Artistic
> Digital magazine UI: strong masthead, multi-column flow, and editorial hierarchy.

## 1. Visual Theme & Atmosphere

Publication design is a digital magazine — a strong masthead, multi-column article flow, pull quotes, bylines, and a confident editorial hierarchy for long-form content.

**Key characteristics**
- Strong masthead and sections
- Multi-column article flow
- Pull quotes and bylines
- Confident editorial hierarchy

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#ffffff` |
| Surface | `--surface` | `#f6f6f6` |
| Surface warm | `--surface-warm` | `#fff2f0` |
| Foreground | `--fg` | `#0b0b0b` |
| Foreground 2 | `--fg-2` | `#333333` |
| Muted | `--muted` | `#666666` |
| Border | `--border` | `#d6d6d6` |
| Accent | `--accent` | `#c1121f` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#0f8a3b` / `#d99a00` / `#b00020` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Franklin Gothic", Arial, sans-serif`
- **Body:** `Georgia, "Times New Roman", serif`
- **Mono:** `"IBM Plex Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 17px / lg 20px / xl 28px / 2xl 42px / 3xl 64px / 4xl 88px`.
- **Line height:** body `--leading-body 1.58`, display `--leading-tight 0.98`; display tracking `--tracking-display -0.018em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Franklin Gothic for display, set running text in Georgia, and use IBM Plex Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1180px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 88px`, tablet `64px`, phone `44px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 0px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(193, 18, 31, 0.24)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 0px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 0px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 16px 42px rgba(0, 0, 0, 0.12)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 120ms` (hover/focus) and `--motion-base 200ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Establish a strong masthead.
- Use editorial hierarchy.
- Support long-form reading.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't flatten editorial hierarchy.
- Don't cramp the reading measure.
- Don't ignore bylines/metadata.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #ffffff` canvas; `.eyebrow` uppercase `--muted` → headline Franklin Gothic 600 `--text-4xl` (`--leading-tight`, tracking `-0.018em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #c1121f`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 0px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) strong masthead and sections; (2) accent `#c1121f` drives interaction; (3) keep type in Franklin Gothic/Georgia; (4) honor the spacing + radius scale exactly.
