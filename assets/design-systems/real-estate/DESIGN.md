# Design System — Estate (Real Estate & Property)

> Category: Real Estate & Property
> Premium real-estate UI: warm neutral surfaces, a refined deep-green accent, serif display, and large photo-forward listing cards.

## 1. Visual Theme & Atmosphere

Estate frames property as an aspiration. Warm neutral surfaces and a refined deep-green accent feel upscale and grounded; a serif display lends editorial calm. Large photo-forward listing cards lead with imagery, then price, location, and key specs in a clear, confident hierarchy.

**Key characteristics**
- Warm neutral surfaces, upscale feel
- Refined deep-green accent
- Serif display for editorial calm
- Large photo-forward listing cards

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#fbfaf7` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `#f2efe8` |
| Foreground | `--fg` | `#221f1a` |
| Foreground 2 | `--fg-2` | `#3d3830` |
| Muted | `--muted` | `#736d62` |
| Border | `--border` | `#e4ded2` |
| Accent | `--accent` | `#1f5d4c` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#2f7d4f` / `#a87514` / `#a8392f` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Fraunces", "Lora", Georgia, "Times New Roman", serif`
- **Body:** `"Inter", -apple-system, system-ui, "Segoe UI", Arial, sans-serif`
- **Mono:** `ui-monospace, "SF Mono", "JetBrains Mono", Menlo, Consolas, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 19px / xl 24px / 2xl 32px / 3xl 44px / 4xl 64px`.
- **Line height:** body `--leading-body 1.55`, display `--leading-tight 1.18`; tracking `--tracking-display -0.01em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Fraunces for display, set running text in Inter, and use ui-monospace for figures/codes.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px`.
- **Container:** `--container-max 1280px`; gutters `40px / 24px / 16px`.
- **Section rhythm:** `--section-y-desktop 80px`, tablet `48px`, phone `32px`.
- Keep a consistent vertical rhythm; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 14px`, hover `--accent-hover`. `.btn-secondary`: `--surface` + 1px `--border`, `--fg` text. Focus: `--focus-ring 0 0 0 3px rgba(31,93,76,.35)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, 1px `--border`, radius `--radius-lg 22px`, `--elev-raised` when raised.
- **Inputs** (`.field`): `--surface`, 1px `--border`, radius `--radius-sm 8px`, explicit label + focus + error states; placeholders `--muted`.
- **Badges / status** (`.status`): semantic color + text label, `--radius-pill 9999px`.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `rgba(34,31,26,.05) 0 2px 6px 0, rgba(34,31,26,.1) 0 12px 32px -8px` |

Apply elevation deliberately in line with this system's character; never elevate every surface.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 230ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states; honor `prefers-reduced-motion`.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this system's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Lead listings with large photography.
- Use serif display for an upscale voice.
- Present price/location/specs clearly.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't use cold sterile chrome.
- Don't crowd listing photos.
- Don't overuse the accent beyond CTAs/price.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #fbfaf7` canvas; `.eyebrow` uppercase `--muted` → headline Fraunces 600 `--text-4xl` (`--leading-tight`, tracking `-0.01em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #1f5d4c`) + `.btn-secondary`.
- **Card:** `--surface` fill, 1px `--border`, `--radius-lg 22px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) warm neutral surfaces, upscale feel; (2) accent `#1f5d4c` drives interaction; (3) keep type in Fraunces/Inter; (4) honor the spacing + radius scale exactly.
