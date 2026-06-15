# Design System — Brutalism

> Category: Bold & Expressive
> Raw, unpolished brutalist web: exposed structure, system type, heavy rules, and zero ornament.

## 1. Visual Theme & Atmosphere

Brutalism celebrates the honesty of the document — visible borders, default-feeling type, monospace accents, and stark contrast. Nothing is hidden behind gloss; the grid, the links, and the structure are the aesthetic.

**Key characteristics**
- Exposed structure and thick visible rules
- System/monospace type, default-web honesty
- Stark high-contrast surfaces, no gradients
- Function over decoration — every element is utilitarian

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#f5f1e8` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `#ffef5a` |
| Foreground | `--fg` | `#000000` |
| Foreground 2 | `--fg-2` | `#222222` |
| Muted | `--muted` | `#555555` |
| Border | `--border` | `#000000` |
| Accent | `--accent` | `#ffef5a` |
| Accent on | `--accent-on` | `#000000` |
| Success / Warn / Danger | — | `#00b050` / `#ff8c00` / `#ff2b2b` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `Arial Black, Impact, sans-serif`
- **Body:** `Arial, Helvetica, sans-serif`
- **Mono:** `"Courier New", ui-monospace, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 17px / lg 20px / xl 28px / 2xl 42px / 3xl 64px / 4xl 88px`.
- **Line height:** body `--leading-body 1.35`, display `--leading-tight 0.98`; display tracking `--tracking-display 0`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Arial Black for display, set running text in Arial, and use Courier New for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1160px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 88px`, tablet `64px`, phone `44px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 0px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px #000000, 0 0 0 8px #ffef5a`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 0px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 0px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `8px 8px 0 #000000` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 90ms` (hover/focus) and `--motion-base 140ms` (entrances); easing `--ease-standard steps(2, end)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Keep borders thick and visible.
- Use stark contrast and plain type.
- Expose structure and grid lines.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't add gradients, soft shadows, or rounded polish.
- Don't hide structure behind decoration.
- Don't use delicate hairlines.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #f5f1e8` canvas; `.eyebrow` uppercase `--muted` → headline Arial Black 600 `--text-4xl` (`--leading-tight`, tracking `0`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #ffef5a`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 0px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) exposed structure and thick visible rules; (2) accent `#ffef5a` drives interaction; (3) keep type in Arial Black/Arial; (4) honor the spacing + radius scale exactly.
