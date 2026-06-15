# Design System — Miro

> Category: Design & Creative
> Miro-style whiteboard UI: infinite canvas, sticky-note color, and collaborative playfulness.

## 1. Visual Theme & Atmosphere

Miro-style design is a collaborative whiteboard — an infinite canvas, bright sticky-note color, hand-friendly objects, and playful but organized collaboration cues.

**Key characteristics**
- Infinite canvas surface
- Bright sticky-note color
- Hand-friendly draggable objects
- Playful, organized collaboration cues

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#fff7c2` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `#fff3a3` |
| Foreground | `--fg` | `#050038` |
| Foreground 2 | `--fg-2` | `#322b6b` |
| Muted | `--muted` | `#605a8a` |
| Border | `--border` | `#ded9ff` |
| Accent | `--accent` | `#ffd02f` |
| Accent on | `--accent-on` | `#050038` |
| Success / Warn / Danger | — | `#1a7f37` / `#ff9f1c` / `#d92d20` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Formular", Inter, Arial, sans-serif`
- **Body:** `"Formular", Inter, Arial, sans-serif`
- **Mono:** `"IBM Plex Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 18px / xl 24px / 2xl 38px / 3xl 56px / 4xl 76px`.
- **Line height:** body `--leading-body 1.5`, display `--leading-tight 1.02`; display tracking `--tracking-display -0.025em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Formular for display, set running text in Formular, and use IBM Plex Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1220px`; gutters `36px / 28px / 18px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 104px`, tablet `72px`, phone `52px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 18px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(255, 208, 47, 0.38)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 28px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 10px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 16px 40px rgba(5, 0, 56, 0.14)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 230ms` (entrances); easing `--ease-standard cubic-bezier(0.22, 1, 0.36, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 18px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Treat the surface as an infinite canvas.
- Use bright sticky-note color.
- Make objects hand-friendly.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't use rigid page-bound layout.
- Don't go monochrome and stiff.
- Don't hide collaboration cues.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #fff7c2` canvas; `.eyebrow` uppercase `--muted` → headline Formular 600 `--text-4xl` (`--leading-tight`, tracking `-0.025em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #ffd02f`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 28px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) infinite canvas surface; (2) accent `#ffd02f` drives interaction; (3) keep type in Formular/Formular; (4) honor the spacing + radius scale exactly.
