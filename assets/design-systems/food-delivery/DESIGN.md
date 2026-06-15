# Design System — Crave (Food & Delivery)

> Category: Food & Delivery
> Appetizing food-delivery UI: warm white surfaces, a hungry tomato-red accent, rounded photo-forward cards, and fast friendly ordering.

## 1. Visual Theme & Atmosphere

Crave makes food look irresistible and ordering effortless. Warm white surfaces let dish photography pop; a hungry tomato-red accent drives "Add" and "Order" actions. Rounded photo-forward cards, clear prices, ratings, and ETA cues keep the flow fast and friendly.

**Key characteristics**
- Warm white surfaces that flatter food photos
- Tomato-red accent for add/order CTAs
- Rounded, photo-forward dish/restaurant cards
- Fast, friendly ordering with clear price/ETA

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#fffaf6` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `#fdeee4` |
| Foreground | `--fg` | `#2a1d16` |
| Foreground 2 | `--fg-2` | `#46352b` |
| Muted | `--muted` | `#7c6a5e` |
| Border | `--border` | `#f0e1d5` |
| Accent | `--accent` | `#e23744` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#2f9e4f` / `#e08b16` / `#c4392f` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Poppins", "Inter", -apple-system, system-ui, sans-serif`
- **Body:** `"Inter", -apple-system, system-ui, "Segoe UI", Arial, sans-serif`
- **Mono:** `ui-monospace, "SF Mono", "JetBrains Mono", Menlo, Consolas, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 19px / xl 24px / 2xl 32px / 3xl 44px / 4xl 58px`.
- **Line height:** body `--leading-body 1.55`, display `--leading-tight 1.18`; tracking `--tracking-display -0.01em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Poppins for display, set running text in Inter, and use ui-monospace for figures/codes.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px`.
- **Container:** `--container-max 1240px`; gutters `40px / 24px / 16px`.
- **Section rhythm:** `--section-y-desktop 72px`, tablet `48px`, phone `32px`.
- Keep a consistent vertical rhythm; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 16px`, hover `--accent-hover`. `.btn-secondary`: `--surface` + 1px `--border`, `--fg` text. Focus: `--focus-ring 0 0 0 3px rgba(226,55,68,.35)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, 1px `--border`, radius `--radius-lg 24px`, `--elev-raised` when raised.
- **Inputs** (`.field`): `--surface`, 1px `--border`, radius `--radius-sm 10px`, explicit label + focus + error states; placeholders `--muted`.
- **Badges / status** (`.status`): semantic color + text label, `--radius-pill 9999px`.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `rgba(42,29,22,.06) 0 2px 6px 0, rgba(42,29,22,.1) 0 12px 30px -8px` |

Apply elevation deliberately in line with this system's character; never elevate every surface.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 230ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states; honor `prefers-reduced-motion`.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this system's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Let dish photography lead.
- Use the red accent for add/order CTAs.
- Show price, rating, and ETA clearly.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't use cold surfaces that dull food.
- Don't bury the order CTA.
- Don't overcomplicate the ordering flow.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #fffaf6` canvas; `.eyebrow` uppercase `--muted` → headline Poppins 600 `--text-4xl` (`--leading-tight`, tracking `-0.01em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #e23744`) + `.btn-secondary`.
- **Card:** `--surface` fill, 1px `--border`, `--radius-lg 24px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) warm white surfaces that flatter food photos; (2) accent `#e23744` drives interaction; (3) keep type in Poppins/Inter; (4) honor the spacing + radius scale exactly.
