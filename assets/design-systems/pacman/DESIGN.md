# Design System — Pacman

> Category: Themed & Unique
> Arcade-maze theme: bold primary dots, maze-blue lines, and playful 8-bit energy.

## 1. Visual Theme & Atmosphere

Pacman design channels the arcade maze — bold yellow/primary tokens, maze-blue structural lines, dot motifs, and playful 8-bit energy rendered cleanly.

**Key characteristics**
- Bold arcade primary palette
- Maze-blue structural lines
- Dot/pellet motifs
- Playful retro-arcade energy

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#050505` |
| Surface | `--surface` | `#101014` |
| Surface warm | `--surface-warm` | `#1f1b08` |
| Foreground | `--fg` | `#fff7d6` |
| Foreground 2 | `--fg-2` | `#f6e79c` |
| Muted | `--muted` | `#b9a85d` |
| Border | `--border` | `#2338ff` |
| Accent | `--accent` | `#ffcc00` |
| Accent on | `--accent-on` | `#050505` |
| Success / Warn / Danger | — | `#00ff66` / `#ff9900` / `#ff3b3b` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Press Start 2P", "Arial Black", system-ui, sans-serif`
- **Body:** `"Inter", system-ui, sans-serif`
- **Mono:** `"Press Start 2P", ui-monospace, monospace`
- **Scale:** `--text-xs 10px / sm 12px / base 14px / lg 16px / xl 20px / 2xl 30px / 3xl 44px / 4xl 64px`.
- **Line height:** body `--leading-body 1.6`, display `--leading-tight 1.08`; display tracking `--tracking-display 0`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Press Start 2P for display, set running text in Inter, and use Press Start 2P for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1080px`; gutters `32px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 84px`, tablet `60px`, phone `44px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 18px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(255, 204, 0, 0.34)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 9999px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 10px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 0 0 2px #2338ff, 0 22px 60px rgba(0, 0, 0, 0.46)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 90ms` (hover/focus) and `--motion-base 160ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Use bold arcade primaries.
- Add maze-line structure and dot motifs.
- Keep the playful arcade energy.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't go muted or corporate.
- Don't drop the arcade motifs.
- Don't overcomplicate the playful idea.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #050505` canvas; `.eyebrow` uppercase `--muted` → headline Press Start 2P 600 `--text-4xl` (`--leading-tight`, tracking `0`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #ffcc00`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 9999px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) bold arcade primary palette; (2) accent `#ffcc00` drives interaction; (3) keep type in Press Start 2P/Inter; (4) honor the spacing + radius scale exactly.
