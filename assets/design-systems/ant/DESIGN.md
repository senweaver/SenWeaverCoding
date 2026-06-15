# Design System — Ant

> Category: Professional & Corporate
> Ant Design enterprise UI: precise components, 4px grid, neutral surfaces, and blue accent.

## 1. Visual Theme & Atmosphere

Ant-style design is a complete enterprise component language — precise spacing on a tight grid, neutral surfaces, a signature blue accent, and exhaustive consistent components for data-rich apps.

**Key characteristics**
- Precise components on a tight grid
- Neutral surfaces, signature blue accent
- Exhaustive consistent component set
- Data-rich enterprise patterns

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#ffffff` |
| Surface | `--surface` | `#f7f8fa` |
| Surface warm | `--surface-warm` | `#fff1f0` |
| Foreground | `--fg` | `#1f1f1f` |
| Foreground 2 | `--fg-2` | `#4b5563` |
| Muted | `--muted` | `#697386` |
| Border | `--border` | `#d9dce3` |
| Accent | `--accent` | `#d32029` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#22a06b` / `#faad14` / `#cf1322` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Ant Sans", "Alibaba PuHuiTi", Inter, Arial, sans-serif`
- **Body:** `"Ant Sans", "Alibaba PuHuiTi", Inter, Arial, sans-serif`
- **Mono:** `"SF Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 18px / xl 22px / 2xl 32px / 3xl 48px / 4xl 64px`.
- **Line height:** body `--leading-body 1.52`, display `--leading-tight 1.08`; display tracking `--tracking-display -0.018em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Ant Sans for display, set running text in Ant Sans, and use SF Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1200px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 96px`, tablet `68px`, phone `48px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 10px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(211, 32, 41, 0.22)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 16px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 6px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 18px 42px rgba(31, 31, 31, 0.10)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 140ms` (hover/focus) and `--motion-base 220ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Use precise grid-based spacing.
- Keep the blue accent consistent.
- Reuse the component set faithfully.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't break grid precision.
- Don't restyle components ad hoc.
- Don't introduce off-system color.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #ffffff` canvas; `.eyebrow` uppercase `--muted` → headline Ant Sans 600 `--text-4xl` (`--leading-tight`, tracking `-0.018em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #d32029`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 16px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) precise components on a tight grid; (2) accent `#d32029` drives interaction; (3) keep type in Ant Sans/Ant Sans; (4) honor the spacing + radius scale exactly.
