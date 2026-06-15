# Design System — Claymorphism

> Category: Morphism & Effects
> Soft, puffy "clay" UI: chunky rounded shapes, doubled inner/outer shadows, and candy-bright accents.

## 1. Visual Theme & Atmosphere

Claymorphism inflates every surface into a soft 3D clay shape — large radii, a light outer drop shadow plus a subtle inner highlight, and friendly saturated colors. It feels tactile, playful, and toy-like.

**Key characteristics**
- Chunky oversized radii on every surface
- Outer drop + inner highlight = puffy clay depth
- Bright, friendly saturated accents
- Generous padding so shapes stay pillowy

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#f7eee6` |
| Surface | `--surface` | `#fff8f1` |
| Surface warm | `--surface-warm` | `#ead6c7` |
| Foreground | `--fg` | `#2b211c` |
| Foreground 2 | `--fg-2` | `#5a4b43` |
| Muted | `--muted` | `#8a7a70` |
| Border | `--border` | `#dac8b9` |
| Accent | `--accent` | `#b46a46` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#4d8f5a` / `#c88735` / `#b84c4c` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `Inter, system-ui, sans-serif`
- **Body:** `Inter, system-ui, sans-serif`
- **Mono:** `"SF Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 18px / xl 24px / 2xl 36px / 3xl 54px / 4xl 76px`.
- **Line height:** body `--leading-body 1.52`, display `--leading-tight 1.06`; display tracking `--tracking-display -0.025em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Inter for display, set running text in Inter, and use SF Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1180px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 96px`, tablet `68px`, phone `48px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 22px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(180, 106, 70, 0.24)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 34px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 14px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `8px 10px 24px rgba(128, 92, 70, 0.18), -8px -8px 20px rgba(255, 255, 255, 0.70)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 240ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Use large radii and puffy doubled shadows.
- Keep colors bright and friendly.
- Give shapes generous padding.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't use sharp corners or flat edges.
- Don't use thin borders instead of soft depth.
- Don't crowd the pillowy shapes.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #f7eee6` canvas; `.eyebrow` uppercase `--muted` → headline Inter 600 `--text-4xl` (`--leading-tight`, tracking `-0.025em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #b46a46`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 34px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) chunky oversized radii on every surface; (2) accent `#b46a46` drives interaction; (3) keep type in Inter/Inter; (4) honor the spacing + radius scale exactly.
