# Design System — Retro

> Category: Retro & Nostalgic
> Throwback design: nostalgic palette, period type, and tasteful vintage motifs.

## 1. Visual Theme & Atmosphere

Retro design revives a past era — a nostalgic palette, period-appropriate type, and tasteful vintage motifs, executed with modern legibility and polish.

**Key characteristics**
- Nostalgic period palette
- Era-appropriate type
- Tasteful vintage motifs
- Modern legibility under the nostalgia

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#fff4cf` |
| Surface | `--surface` | `#fffaf0` |
| Surface warm | `--surface-warm` | `#ffdca8` |
| Foreground | `--fg` | `#2a1810` |
| Foreground 2 | `--fg-2` | `#593625` |
| Muted | `--muted` | `#8a6652` |
| Border | `--border` | `#d9aa7a` |
| Accent | `--accent` | `#d24b1f` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#3d8f4f` / `#f2a93b` / `#b83a2f` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Courier New", ui-monospace, monospace`
- **Body:** `Inter, system-ui, sans-serif`
- **Mono:** `"Courier New", ui-monospace, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 18px / xl 24px / 2xl 36px / 3xl 54px / 4xl 76px`.
- **Line height:** body `--leading-body 1.52`, display `--leading-tight 1.06`; display tracking `--tracking-display 0`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Courier New for display, set running text in Inter, and use Courier New for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1180px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 96px`, tablet `68px`, phone `48px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 8px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(210, 75, 31, 0.28)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 12px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 4px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `6px 6px 0 rgba(42, 24, 16, 0.26)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 240ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Commit to the period palette and type.
- Use vintage motifs tastefully.
- Keep modern legibility.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't mix clashing eras.
- Don't sacrifice readability for theme.
- Don't overdo kitsch.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #fff4cf` canvas; `.eyebrow` uppercase `--muted` → headline Courier New 600 `--text-4xl` (`--leading-tight`, tracking `0`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #d24b1f`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 12px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) nostalgic period palette; (2) accent `#d24b1f` drives interaction; (3) keep type in Courier New/Inter; (4) honor the spacing + radius scale exactly.
