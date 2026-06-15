# Design System — Lingo

> Category: Creative & Artistic
> Language-learning UI: friendly bright color, progress cues, and bite-sized lesson cards.

## 1. Visual Theme & Atmosphere

Lingo design supports learning by doing — friendly bright color, clear progress and streak cues, encouraging feedback, and bite-sized lesson cards that reward momentum.

**Key characteristics**
- Friendly bright color
- Clear progress/streak cues
- Encouraging feedback states
- Bite-sized lesson cards

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#fbf9ff` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `#f1ecff` |
| Foreground | `--fg` | `#1d1b2a` |
| Foreground 2 | `--fg-2` | `#4f4863` |
| Muted | `--muted` | `#786f8f` |
| Border | `--border` | `#ded7f0` |
| Accent | `--accent` | `#6d4aff` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#22a06b` / `#e6a700` / `#e5484d` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `Inter, system-ui, sans-serif`
- **Body:** `Inter, system-ui, sans-serif`
- **Mono:** `"Roboto Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 18px / xl 24px / 2xl 34px / 3xl 50px / 4xl 70px`.
- **Line height:** body `--leading-body 1.55`, display `--leading-tight 1.05`; display tracking `--tracking-display -0.025em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Inter for display, set running text in Inter, and use Roboto Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1160px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 96px`, tablet `68px`, phone `48px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 16px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(109, 74, 255, 0.24)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 24px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 10px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 20px 48px rgba(45, 36, 85, 0.12)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 230ms` (entrances); easing `--ease-standard cubic-bezier(0.22, 1, 0.36, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Use friendly bright color.
- Surface progress and streaks.
- Reward momentum with feedback.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't feel dry or punishing.
- Don't hide progress.
- Don't overload a single lesson view.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #fbf9ff` canvas; `.eyebrow` uppercase `--muted` → headline Inter 600 `--text-4xl` (`--leading-tight`, tracking `-0.025em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #6d4aff`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 24px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) friendly bright color; (2) accent `#6d4aff` drives interaction; (3) keep type in Inter/Inter; (4) honor the spacing + radius scale exactly.
