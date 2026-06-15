# Design System — Refined

> Category: Modern & Minimal
> Understated elegance: delicate type, careful spacing, and quiet sophistication.

## 1. Visual Theme & Atmosphere

Refined design is quietly sophisticated — delicate type, careful optical spacing, restrained color, and elegant detail. Nothing shouts; everything is considered.

**Key characteristics**
- Delicate, elegant type
- Careful optical spacing
- Restrained, sophisticated color
- Considered, quiet detail

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#fbf6ee` |
| Surface | `--surface` | `#fffdf8` |
| Surface warm | `--surface-warm` | `#f1e3cf` |
| Foreground | `--fg` | `#201914` |
| Foreground 2 | `--fg-2` | `#4c4037` |
| Muted | `--muted` | `#7a6d63` |
| Border | `--border` | `#ded2c3` |
| Accent | `--accent` | `#9b5b32` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#4f8a4f` / `#c9822f` / `#b33a3a` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `Georgia, "Times New Roman", serif`
- **Body:** `Inter, system-ui, sans-serif`
- **Mono:** `"SF Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 17px / lg 20px / xl 28px / 2xl 42px / 3xl 64px / 4xl 88px`.
- **Line height:** body `--leading-body 1.62`, display `--leading-tight 1`; display tracking `--tracking-display -0.025em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Georgia for display, set running text in Inter, and use SF Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1180px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 112px`, tablet `80px`, phone `56px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 16px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(155, 91, 50, 0.24)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 24px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 10px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 20px 52px rgba(32, 25, 20, 0.12)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 240ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Use delicate type and careful spacing.
- Keep color restrained.
- Favor quiet sophistication.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't shout with heavy type or color.
- Don't use careless spacing.
- Don't over-decorate.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #fbf6ee` canvas; `.eyebrow` uppercase `--muted` → headline Georgia 600 `--text-4xl` (`--leading-tight`, tracking `-0.025em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #9b5b32`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 24px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) delicate, elegant type; (2) accent `#9b5b32` drives interaction; (3) keep type in Georgia/Inter; (4) honor the spacing + radius scale exactly.
