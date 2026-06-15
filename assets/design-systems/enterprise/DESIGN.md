# Design System — Enterprise

> Category: Professional & Corporate
> Data-dense enterprise UI: compact tables, calm chrome, and high information throughput.

## 1. Visual Theme & Atmosphere

Enterprise design optimizes for information throughput — compact data tables, calm low-distraction chrome, clear status semantics, and dense but legible layouts for power users.

**Key characteristics**
- Compact, scannable data tables
- Calm low-distraction chrome
- Clear status/semantic color
- Dense yet legible layouts

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#f8fafc` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `#eef2f7` |
| Foreground | `--fg` | `#0f172a` |
| Foreground 2 | `--fg-2` | `#334155` |
| Muted | `--muted` | `#64748b` |
| Border | `--border` | `#d8dee8` |
| Accent | `--accent` | `#1d4ed8` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#15803d` / `#b45309` / `#b91c1c` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `Inter, system-ui, sans-serif`
- **Body:** `Inter, system-ui, sans-serif`
- **Mono:** `"IBM Plex Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 18px / xl 24px / 2xl 34px / 3xl 48px / 4xl 64px`.
- **Line height:** body `--leading-body 1.52`, display `--leading-tight 1.08`; display tracking `--tracking-display -0.018em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Inter for display, set running text in Inter, and use IBM Plex Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1240px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 96px`, tablet `68px`, phone `48px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 10px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(29, 78, 216, 0.22)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 16px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 6px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 20px 52px rgba(15, 23, 42, 0.10)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 230ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Optimize for data density and scanning.
- Keep chrome calm.
- Use clear status semantics.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't waste space with oversized chrome.
- Don't distract from data.
- Don't sacrifice legibility for density.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #f8fafc` canvas; `.eyebrow` uppercase `--muted` → headline Inter 600 `--text-4xl` (`--leading-tight`, tracking `-0.018em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #1d4ed8`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 16px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) compact, scannable data tables; (2) accent `#1d4ed8` drives interaction; (3) keep type in Inter/Inter; (4) honor the spacing + radius scale exactly.
