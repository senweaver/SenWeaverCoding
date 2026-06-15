# Design System — Luxury

> Category: Professional & Corporate
> Opulent high-fashion UI: dramatic space, refined serif, and a singular jewel accent.

## 1. Visual Theme & Atmosphere

Luxury design is opulent and restrained at once — dramatic whitespace, a refined serif voice, deep sophisticated surfaces, and a single jewel-like accent. It whispers exclusivity.

**Key characteristics**
- Dramatic luxurious whitespace
- Refined serif voice
- Deep, sophisticated surfaces
- A single jewel-like accent

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#080706` |
| Surface | `--surface` | `#151310` |
| Surface warm | `--surface-warm` | `#241e14` |
| Foreground | `--fg` | `#fff8ea` |
| Foreground 2 | `--fg-2` | `#d8cdb7` |
| Muted | `--muted` | `#9f927c` |
| Border | `--border` | `#3a3020` |
| Accent | `--accent` | `#c6a15b` |
| Accent on | `--accent-on` | `#080706` |
| Success / Warn / Danger | — | `#5fa36a` / `#d8a94f` / `#d85a52` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Didot", "Bodoni 72", Georgia, serif`
- **Body:** `"Avenir Next", Inter, sans-serif`
- **Mono:** `"SF Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 19px / xl 28px / 2xl 44px / 3xl 68px / 4xl 96px`.
- **Line height:** body `--leading-body 1.6`, display `--leading-tight 0.98`; display tracking `--tracking-display -0.015em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Didot for display, set running text in Avenir Next, and use SF Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1160px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 120px`, tablet `84px`, phone `60px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 14px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(198, 161, 91, 0.30)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 24px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 8px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 30px 90px rgba(0, 0, 0, 0.52)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 180ms` (hover/focus) and `--motion-base 300ms` (entrances); easing `--ease-standard cubic-bezier(0.16, 1, 0.3, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Use dramatic whitespace.
- Add a refined serif voice.
- Keep one jewel accent.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't crowd the composition.
- Don't use busy or cheap-looking color.
- Don't overuse the accent.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #080706` canvas; `.eyebrow` uppercase `--muted` → headline Didot 600 `--text-4xl` (`--leading-tight`, tracking `-0.015em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #c6a15b`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 24px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) dramatic luxurious whitespace; (2) accent `#c6a15b` drives interaction; (3) keep type in Didot/Avenir Next; (4) honor the spacing + radius scale exactly.
