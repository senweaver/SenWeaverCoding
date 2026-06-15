# Design System — Tetris

> Category: Themed & Unique
> Block-grid theme: tetromino color blocks, snapped grid, and tidy modular stacking.

## 1. Visual Theme & Atmosphere

Tetris design is built from colored blocks on a snapped grid — tetromino-like modular tiles, tidy stacking, and a bright primary palette that turns layout into play.

**Key characteristics**
- Tetromino-like color blocks
- Snapped modular grid
- Tidy stacking and packing
- Bright primary block palette

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#050816` |
| Surface | `--surface` | `#10162a` |
| Surface warm | `--surface-warm` | `#17203a` |
| Foreground | `--fg` | `#f8fafc` |
| Foreground 2 | `--fg-2` | `#cbd5e1` |
| Muted | `--muted` | `#94a3b8` |
| Border | `--border` | `#26324f` |
| Accent | `--accent` | `#00f0f0` |
| Accent on | `--accent-on` | `#061018` |
| Success / Warn / Danger | — | `#00f000` / `#f0f000` / `#f00000` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Press Start 2P", "Arial Black", system-ui, sans-serif`
- **Body:** `"Inter", system-ui, sans-serif`
- **Mono:** `"Press Start 2P", ui-monospace, monospace`
- **Scale:** `--text-xs 10px / sm 12px / base 14px / lg 16px / xl 20px / 2xl 28px / 3xl 40px / 4xl 56px`.
- **Line height:** body `--leading-body 1.6`, display `--leading-tight 1.1`; display tracking `--tracking-display 0`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Press Start 2P for display, set running text in Inter, and use Press Start 2P for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1100px`; gutters `32px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 80px`, tablet `60px`, phone `44px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 0px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(0, 240, 240, 0.32)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 4px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 0px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 18px 0 rgba(0, 0, 0, 0.32)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 80ms` (hover/focus) and `--motion-base 140ms` (entrances); easing `--ease-standard steps(2, end)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Compose from snapped color blocks.
- Stack modules tidily.
- Use bright primary blocks.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't use loose off-grid placement.
- Don't blur the block identity.
- Don't mute the primaries.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #050816` canvas; `.eyebrow` uppercase `--muted` → headline Press Start 2P 600 `--text-4xl` (`--leading-tight`, tracking `0`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #00f0f0`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 4px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) tetromino-like color blocks; (2) accent `#00f0f0` drives interaction; (3) keep type in Press Start 2P/Inter; (4) honor the spacing + radius scale exactly.
