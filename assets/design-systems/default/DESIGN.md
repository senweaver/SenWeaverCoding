# Design System — Neutral Modern

> Category: Starter
> Neutral modern starter: balanced neutrals, one accent, and sensible defaults for any product.

## 1. Visual Theme & Atmosphere

The Neutral Modern starter is a safe, balanced baseline — neutral surfaces, a single clear accent, comfortable density, and sensible defaults. A dependable starting point that adapts to most products.

**Key characteristics**
- Balanced neutral surfaces
- Single clear accent
- Comfortable default density
- Sensible, adaptable defaults

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#fafafa` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `var(--surface)` |
| Foreground | `--fg` | `#111111` |
| Foreground 2 | `--fg-2` | `var(--fg)` |
| Muted | `--muted` | `#6b6b6b` |
| Border | `--border` | `#e5e5e5` |
| Accent | `--accent` | `#2f6feb` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#17a34a` / `#eab308` / `#dc2626` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Inter", -apple-system, system-ui, sans-serif`
- **Body:** `"Inter", -apple-system, system-ui, sans-serif`
- **Mono:** `ui-monospace, "JetBrains Mono", monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 20px / xl 24px / 2xl 32px / 3xl 48px / 4xl 64px`.
- **Line height:** body `--leading-body 1.5`, display `--leading-tight 1.2`; display tracking `--tracking-display -0.01em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Inter for display, set running text in Inter, and use ui-monospace for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1200px`; gutters `24px / 16px / 12px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 80px`, tablet `48px`, phone `32px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 12px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 3px color-mix(in oklab, var(--accent), transparent 70%)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 16px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 8px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 2px 8px color-mix(in oklab, var(--fg), transparent 92%)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 200ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 12px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Keep a balanced neutral base.
- Use one clear accent.
- Favor sensible defaults.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't over-style the baseline.
- Don't use competing accents.
- Don't crowd the layout.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #fafafa` canvas; `.eyebrow` uppercase `--muted` → headline Inter 600 `--text-4xl` (`--leading-tight`, tracking `-0.01em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #2f6feb`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 16px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) balanced neutral surfaces; (2) accent `#2f6feb` drives interaction; (3) keep type in Inter/Inter; (4) honor the spacing + radius scale exactly.
