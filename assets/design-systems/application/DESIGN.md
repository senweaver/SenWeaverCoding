# Design System — Application

> Category: Professional & Corporate
> App-shell product UI: persistent nav, panels, toolbars, and workspace density.

## 1. Visual Theme & Atmosphere

Application design is built for software workspaces — persistent navigation, side panels, toolbars, and a content area tuned for sustained work. Clear regions and consistent controls.

**Key characteristics**
- Persistent nav + side panels
- Toolbars and consistent controls
- Workspace-tuned content density
- Clear, separated regions

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#f6f7f9` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `#eef4ff` |
| Foreground | `--fg` | `#172033` |
| Foreground 2 | `--fg-2` | `#3b4658` |
| Muted | `--muted` | `#6b7689` |
| Border | `--border` | `#d8dee8` |
| Accent | `--accent` | `#2563eb` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#16a34a` / `#f59e0b` / `#dc2626` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `Inter, system-ui, sans-serif`
- **Body:** `Inter, system-ui, sans-serif`
- **Mono:** `"SF Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 18px / xl 22px / 2xl 30px / 3xl 42px / 4xl 58px`.
- **Line height:** body `--leading-body 1.5`, display `--leading-tight 1.12`; display tracking `--tracking-display -0.015em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Inter for display, set running text in Inter, and use SF Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1200px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 88px`, tablet `64px`, phone `44px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 12px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(37, 99, 235, 0.22)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 18px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 8px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 16px 40px rgba(23, 32, 51, 0.10)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 140ms` (hover/focus) and `--motion-base 220ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Define clear app regions.
- Keep controls consistent.
- Tune density for sustained work.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't blur region boundaries.
- Don't vary control patterns.
- Don't use marketing-page spacing.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #f6f7f9` canvas; `.eyebrow` uppercase `--muted` → headline Inter 600 `--text-4xl` (`--leading-tight`, tracking `-0.015em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #2563eb`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 18px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) persistent nav + side panels; (2) accent `#2563eb` drives interaction; (3) keep type in Inter/Inter; (4) honor the spacing + radius scale exactly.
