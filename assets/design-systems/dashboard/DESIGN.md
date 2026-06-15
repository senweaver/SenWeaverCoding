# Design System — Dashboard

> Category: Professional & Corporate
> Analytics dashboard UI: metric cards, chart-ready surfaces, and at-a-glance hierarchy.

## 1. Visual Theme & Atmosphere

Dashboard design surfaces data at a glance — metric cards, chart-ready surfaces, clear KPI hierarchy, and calm chrome that lets data lead. Scannable and decision-oriented.

**Key characteristics**
- Metric cards and KPI hierarchy
- Chart-ready calm surfaces
- At-a-glance scannability
- Status/semantic color for signals

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#f4f7fb` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `#eef6ff` |
| Foreground | `--fg` | `#111827` |
| Foreground 2 | `--fg-2` | `#334155` |
| Muted | `--muted` | `#64748b` |
| Border | `--border` | `#d8e2ee` |
| Accent | `--accent` | `#0ea5e9` |
| Accent on | `--accent-on` | `#04131d` |
| Success / Warn / Danger | — | `#10b981` / `#f59e0b` / `#ef4444` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `Inter, system-ui, sans-serif`
- **Body:** `Inter, system-ui, sans-serif`
- **Mono:** `"IBM Plex Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 11px / sm 13px / base 15px / lg 17px / xl 22px / 2xl 30px / 3xl 42px / 4xl 56px`.
- **Line height:** body `--leading-body 1.48`, display `--leading-tight 1.1`; display tracking `--tracking-display -0.015em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Inter for display, set running text in Inter, and use IBM Plex Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1280px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 84px`, tablet `60px`, phone `42px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 12px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(14, 165, 233, 0.22)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 18px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 8px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 18px 46px rgba(15, 23, 42, 0.10)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 120ms` (hover/focus) and `--motion-base 200ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Lead with metrics and KPIs.
- Keep chrome calm for charts.
- Use semantic color for signals.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't bury the key numbers.
- Don't over-decorate around charts.
- Don't overload one view.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #f4f7fb` canvas; `.eyebrow` uppercase `--muted` → headline Inter 600 `--text-4xl` (`--leading-tight`, tracking `-0.015em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #0ea5e9`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 18px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) metric cards and kpi hierarchy; (2) accent `#0ea5e9` drives interaction; (3) keep type in Inter/Inter; (4) honor the spacing + radius scale exactly.
