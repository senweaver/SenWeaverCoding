# Design System — Webflow

> Category: Design & Creative
> Webflow-style design-tool UI: crisp canvas, panel chrome, and confident builder accent.

## 1. Visual Theme & Atmosphere

Webflow-style design is a visual builder aesthetic — a crisp canvas, structured panel chrome, precise controls, and a confident accent that marks active/builder states.

**Key characteristics**
- Crisp builder canvas
- Structured panel chrome
- Precise, confident controls
- Active-state builder accent

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#ffffff` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `var(--surface)` |
| Foreground | `--fg` | `#080808` |
| Foreground 2 | `--fg-2` | `#363636` |
| Muted | `--muted` | `#5a5a5a` |
| Border | `--border` | `#d8d8d8` |
| Accent | `--accent` | `#146ef5` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#00d722` / `#ffae13` / `#ee1d36` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"WF Visual Sans Variable", "Inter", Arial, system-ui, sans-serif`
- **Body:** `"WF Visual Sans Variable", "Inter", Arial, system-ui, sans-serif`
- **Mono:** `"Inconsolata", ui-monospace, "SF Mono", Menlo, Monaco, Consolas, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 20px / xl 24px / 2xl 32px / 3xl 56px / 4xl 80px`.
- **Line height:** body `--leading-body 1.6`, display `--leading-tight 1.04`; display tracking `--tracking-display -0.01em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with WF Visual Sans Variable for display, set running text in WF Visual Sans Variable, and use Inconsolata for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1200px`; gutters `24px / 16px / 12px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 96px`, tablet `64px`, phone `48px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 4px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 3px color-mix(in oklab, var(--accent), transparent 70%)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 8px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 4px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0px 84px 24px rgba(0, 0, 0, 0), 0px 54px 22px rgba(0, 0, 0, 0.01), 0px 30px 18px rgba(0, 0, 0, 0.04), 0px 13px 13px rgba(0, 0, 0, 0.08), 0px 3px 7px rgba(0, 0, 0, 0.09)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 200ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 12px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Keep a crisp canvas with panel chrome.
- Use precise controls.
- Mark active states with the accent.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't blur canvas vs chrome.
- Don't use imprecise controls.
- Don't overuse the accent.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #ffffff` canvas; `.eyebrow` uppercase `--muted` → headline WF Visual Sans Variable 600 `--text-4xl` (`--leading-tight`, tracking `-0.01em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #146ef5`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 8px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) crisp builder canvas; (2) accent `#146ef5` drives interaction; (3) keep type in WF Visual Sans Variable/WF Visual Sans Variable; (4) honor the spacing + radius scale exactly.
