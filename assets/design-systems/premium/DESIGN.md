# Design System — Premium

> Category: Professional & Corporate
> High-end product UI: deep contrast, gold/accent restraint, and meticulous polish.

## 1. Visual Theme & Atmosphere

Premium design signals high value — confident contrast, meticulous spacing, restrained accent (often metallic or deep), and flawless detail. Every pixel feels intentional and expensive.

**Key characteristics**
- Confident, high-value contrast
- Meticulous spacing and alignment
- Restrained, often deep/metallic accent
- Flawless, intentional detail

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#faf8f4` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `#f0e7d8` |
| Foreground | `--fg` | `#1c1b19` |
| Foreground 2 | `--fg-2` | `#4b4740` |
| Muted | `--muted` | `#746d63` |
| Border | `--border` | `#ded6c9` |
| Accent | `--accent` | `#a06a3b` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#3f8f5f` / `#c4872c` / `#b84a4a` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Canela", Georgia, serif`
- **Body:** `Inter, system-ui, sans-serif`
- **Mono:** `"IBM Plex Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 19px / xl 26px / 2xl 40px / 3xl 60px / 4xl 84px`.
- **Line height:** body `--leading-body 1.58`, display `--leading-tight 1.02`; display tracking `--tracking-display -0.02em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Canela for display, set running text in Inter, and use IBM Plex Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1160px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 112px`, tablet `80px`, phone `56px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 16px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(160, 106, 59, 0.24)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 28px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 10px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 24px 64px rgba(28, 27, 25, 0.12)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 170ms` (hover/focus) and `--motion-base 280ms` (entrances); easing `--ease-standard cubic-bezier(0.22, 1, 0.36, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Keep detail meticulous.
- Use restrained high-value accent.
- Make contrast confident.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't feel cheap or busy.
- Don't overuse the accent.
- Don't allow misalignment.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #faf8f4` canvas; `.eyebrow` uppercase `--muted` → headline Canela 600 `--text-4xl` (`--leading-tight`, tracking `-0.02em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #a06a3b`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 28px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) confident, high-value contrast; (2) accent `#a06a3b` drives interaction; (3) keep type in Canela/Inter; (4) honor the spacing + radius scale exactly.
