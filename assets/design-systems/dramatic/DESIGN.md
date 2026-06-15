# Design System — Dramatic

> Category: Bold & Expressive
> Cinematic high-contrast UI: deep darks, spotlight accents, and theatrical scale.

## 1. Visual Theme & Atmosphere

Dramatic design is cinematic — deep dark fields, a single spotlight accent, and theatrical scale. Negative space and contrast create tension; the hero element commands the stage.

**Key characteristics**
- Deep dark fields, high contrast
- Single spotlight accent
- Theatrical scale and negative space
- Moody, focused atmosphere

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#090b12` |
| Surface | `--surface` | `#121722` |
| Surface warm | `--surface-warm` | `#1b2233` |
| Foreground | `--fg` | `#f8fafc` |
| Foreground 2 | `--fg-2` | `#cbd5e1` |
| Muted | `--muted` | `#94a3b8` |
| Border | `--border` | `#2a3447` |
| Accent | `--accent` | `#60a5fa` |
| Accent on | `--accent-on` | `#06101d` |
| Success / Warn / Danger | — | `#22c55e` / `#fbbf24` / `#fb7185` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `Inter, system-ui, sans-serif`
- **Body:** `Inter, system-ui, sans-serif`
- **Mono:** `"SF Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 18px / xl 24px / 2xl 36px / 3xl 54px / 4xl 76px`.
- **Line height:** body `--leading-body 1.52`, display `--leading-tight 1.06`; display tracking `--tracking-display -0.025em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Inter for display, set running text in Inter, and use SF Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1180px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 96px`, tablet `68px`, phone `48px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 16px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(96, 165, 250, 0.28)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 24px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 10px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 24px 72px rgba(0, 0, 0, 0.42)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 240ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Use deep darks and a spotlight accent.
- Let negative space build tension.
- Make the hero element dominate.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't flatten contrast into grey mush.
- Don't scatter multiple competing accents.
- Don't fill every inch — drama needs space.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #090b12` canvas; `.eyebrow` uppercase `--muted` → headline Inter 600 `--text-4xl` (`--leading-tight`, tracking `-0.025em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #60a5fa`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 24px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) deep dark fields, high contrast; (2) accent `#60a5fa` drives interaction; (3) keep type in Inter/Inter; (4) honor the spacing + radius scale exactly.
