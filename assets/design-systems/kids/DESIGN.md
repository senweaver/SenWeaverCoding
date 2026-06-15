# Design System — Sprout (Kids & Family)

> Category: Kids & Family
> Playful kids & family UI: bright friendly palette, big rounded shapes, large touch targets, and joyful but legible hierarchy.

## 1. Visual Theme & Atmosphere

Sprout is joyful and safe. Bright friendly color, big pillowy rounded shapes, and generous large touch targets suit small hands and big imaginations. Energy stays organized and legible, with warm encouragement and zero clutter — playful, never chaotic.

**Key characteristics**
- Bright, friendly, joyful palette
- Big pillowy rounded shapes
- Large touch targets for small hands
- Organized, legible playfulness

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#fffdf5` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `#fff3d6` |
| Foreground | `--fg` | `#2b2440` |
| Foreground 2 | `--fg-2` | `#473e63` |
| Muted | `--muted` | `#736a8c` |
| Border | `--border` | `#ffe3a6` |
| Accent | `--accent` | `#ff8a3d` |
| Accent on | `--accent-on` | `#2b2440` |
| Success / Warn / Danger | — | `#34c759` / `#ffb020` / `#ff5d5d` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Baloo 2", "Quicksand", "Inter", system-ui, sans-serif`
- **Body:** `"Inter", -apple-system, system-ui, "Segoe UI", Arial, sans-serif`
- **Mono:** `ui-monospace, "SF Mono", "JetBrains Mono", Menlo, Consolas, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 17px / lg 19px / xl 24px / 2xl 32px / 3xl 44px / 4xl 58px`.
- **Line height:** body `--leading-body 1.55`, display `--leading-tight 1.18`; tracking `--tracking-display -0.01em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Baloo 2 for display, set running text in Inter, and use ui-monospace for figures/codes.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px`.
- **Container:** `--container-max 1120px`; gutters `40px / 24px / 16px`.
- **Section rhythm:** `--section-y-desktop 72px`, tablet `48px`, phone `32px`.
- Keep a consistent vertical rhythm; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 22px`, hover `--accent-hover`. `.btn-secondary`: `--surface` + 1px `--border`, `--fg` text. Focus: `--focus-ring 0 0 0 4px rgba(255,138,61,.4)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, 1px `--border`, radius `--radius-lg 32px`, `--elev-raised` when raised.
- **Inputs** (`.field`): `--surface`, 1px `--border`, radius `--radius-sm 14px`, explicit label + focus + error states; placeholders `--muted`.
- **Badges / status** (`.status`): semantic color + text label, `--radius-pill 9999px`.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `rgba(43,36,64,.06) 0 4px 10px 0, rgba(43,36,64,.1) 0 14px 32px -8px` |

Apply elevation deliberately in line with this system's character; never elevate every surface.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 230ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states; honor `prefers-reduced-motion`.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this system's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Use big rounded shapes and large targets.
- Keep color joyful but organized.
- Reward with warm, friendly feedback.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't use sharp small controls.
- Don't overwhelm with chaotic color.
- Don't use tiny or low-contrast text.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #fffdf5` canvas; `.eyebrow` uppercase `--muted` → headline Baloo 2 600 `--text-4xl` (`--leading-tight`, tracking `-0.01em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #ff8a3d`) + `.btn-secondary`.
- **Card:** `--surface` fill, 1px `--border`, `--radius-lg 32px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) bright, friendly, joyful palette; (2) accent `#ff8a3d` drives interaction; (3) keep type in Baloo 2/Inter; (4) honor the spacing + radius scale exactly.
