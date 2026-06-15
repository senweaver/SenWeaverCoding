# Design System — Talent (HR & Recruiting)

> Category: HR & Recruiting
> Modern HR & recruiting UI: light surfaces, a friendly-professional violet accent, clear candidate pipelines, and humane hierarchy.

## 1. Visual Theme & Atmosphere

Talent humanizes hiring. Light surfaces and a friendly-professional violet accent feel modern and warm; candidate pipelines, stages, and profiles are organized into clear, kanban-friendly cards. Encouraging tone, clear status, and respectful presentation of people throughout.

**Key characteristics**
- Light surfaces, friendly-professional violet accent
- Clear candidate pipeline/stage cards
- Humane, respectful presentation of people
- Modern SaaS density and clarity

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#fbfbfe` |
| Surface | `--surface` | `#ffffff` |
| Surface warm | `--surface-warm` | `#f2f1fb` |
| Foreground | `--fg` | `#1c1b2e` |
| Foreground 2 | `--fg-2` | `#34324b` |
| Muted | `--muted` | `#625f78` |
| Border | `--border` | `#e4e2f1` |
| Accent | `--accent` | `#6d4aff` |
| Accent on | `--accent-on` | `#ffffff` |
| Success / Warn / Danger | — | `#16a34a` / `#d97706` / `#dc2626` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `"Inter", -apple-system, system-ui, "Segoe UI", Arial, sans-serif`
- **Body:** `"Inter", -apple-system, system-ui, "Segoe UI", Arial, sans-serif`
- **Mono:** `ui-monospace, "SF Mono", "JetBrains Mono", Menlo, Consolas, monospace`
- **Scale:** `--text-xs 12px / sm 14px / base 16px / lg 19px / xl 24px / 2xl 32px / 3xl 44px / 4xl 58px`.
- **Line height:** body `--leading-body 1.55`, display `--leading-tight 1.18`; tracking `--tracking-display -0.01em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Inter for display, set running text in Inter, and use ui-monospace for figures/codes.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px`.
- **Container:** `--container-max 1240px`; gutters `40px / 24px / 16px`.
- **Section rhythm:** `--section-y-desktop 72px`, tablet `48px`, phone `32px`.
- Keep a consistent vertical rhythm; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 12px`, hover `--accent-hover`. `.btn-secondary`: `--surface` + 1px `--border`, `--fg` text. Focus: `--focus-ring 0 0 0 3px rgba(109,74,255,.35)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, 1px `--border`, radius `--radius-lg 18px`, `--elev-raised` when raised.
- **Inputs** (`.field`): `--surface`, 1px `--border`, radius `--radius-sm 8px`, explicit label + focus + error states; placeholders `--muted`.
- **Badges / status** (`.status`): semantic color + text label, `--radius-pill 9999px`.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `rgba(28,27,46,.05) 0 1px 2px 0, rgba(28,27,46,.08) 0 8px 24px -6px` |

Apply elevation deliberately in line with this system's character; never elevate every surface.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` (hover/focus) and `--motion-base 230ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states; honor `prefers-reduced-motion`.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this system's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Organize pipelines into clear stage cards.
- Keep tone humane and encouraging.
- Use the violet accent for actions/active stages.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't reduce people to cold rows.
- Don't obscure candidate status.
- Don't use harsh or clinical styling.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #fbfbfe` canvas; `.eyebrow` uppercase `--muted` → headline Inter 600 `--text-4xl` (`--leading-tight`, tracking `-0.01em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #6d4aff`) + `.btn-secondary`.
- **Card:** `--surface` fill, 1px `--border`, `--radius-lg 18px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) light surfaces, friendly-professional violet accent; (2) accent `#6d4aff` drives interaction; (3) keep type in Inter/Inter; (4) honor the spacing + radius scale exactly.
