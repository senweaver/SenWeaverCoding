# Design System — Gradient

> Category: Morphism & Effects
> Saturated gradient-forward UI: flowing multi-stop color, luminous CTAs, and vivid surfaces.

## 1. Visual Theme & Atmosphere

Gradient design leads with flowing color — multi-stop blends across heroes, buttons, and accents. Color is the hero; surfaces glow, CTAs shimmer, and motion lets gradients drift.

**Key characteristics**
- Multi-stop gradients on heroes and CTAs
- Luminous, saturated color as the focal point
- Gradient-tinted shadows for cohesive glow
- Crisp text kept legible over color

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#f7f3ff` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `#efe7ff` |
| Foreground | `--fg` | `#191225` |
| Foreground 2 | `--fg-2` | `#443856` |
| Muted | `--muted` | `#746985` |
| Border | `--border` | `#ddd2f2` |
| Accent | `--accent` | `#7c3aed` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#10b981` / `#f59e0b` / `#ef4444` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `Inter, system-ui, sans-serif`
- **Body:** `Inter, system-ui, sans-serif`
- **Mono:** `"Geist Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 19px / xl 26px / 2xl 40px / 3xl 62px / 4xl 86px`.
- **Line height:** body `--leading-body 1.52`, display `--leading-tight 1.02`; display tracking `--tracking-display -0.03em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Inter for display, set running text in Inter, and use Geist Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1180px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 104px`, tablet `72px`, phone `52px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 20px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(124, 58, 237, 0.26)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 32px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 12px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 24px 72px rgba(124, 58, 237, 0.20)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 160ms` (hover/focus) and `--motion-base 260ms` (entrances); easing `--ease-standard cubic-bezier(0.22, 1, 0.36, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Use rich multi-stop gradients on focal elements.
- Tint shadows with the gradient hue.
- Keep text contrast high over color.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't gradient every element into noise.
- Don't use muddy or low-contrast blends.
- Don't place long copy directly on busy gradients.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #f7f3ff` canvas; `.eyebrow` uppercase `--muted` → headline Inter 600 `--text-4xl` (`--leading-tight`, tracking `-0.03em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #7c3aed`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 32px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) multi-stop gradients on heroes and ctas; (2) accent `#7c3aed` drives interaction; (3) keep type in Inter/Inter; (4) honor the spacing + radius scale exactly.
