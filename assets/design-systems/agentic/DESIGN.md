# Design System — Agentic

> Category: Themed & Unique
> AI-agent product UI: chat-and-canvas layout, tool/run states, and calm technical surfaces.

## 1. Visual Theme & Atmosphere

Agentic design is built for AI agents — a chat-plus-canvas layout, clear tool-call and run states, streaming feedback, and calm technical surfaces that keep long autonomous runs legible.

**Key characteristics**
- Chat + canvas dual layout
- Clear tool-call and run states
- Streaming, legible feedback
- Calm technical surfaces

## 2. Color & Roles

Bind these `:root` tokens verbatim and reference every value through `var(--*)` — never hard-code hex outside `:root`.

| Role | Token | Value |
|------|-------|-------|
| Background | `--bg` | `#0b1020` |
| Surface | `--surface` | `#131b2f` |
| Surface warm | `--surface-warm` | `#182343` |
| Foreground | `--fg` | `#f8fafc` |
| Foreground 2 | `--fg-2` | `#cbd5e1` |
| Muted | `--muted` | `#8ea0b8` |
| Border | `--border` | `#293653` |
| Accent | `--accent` | `#60a5fa` |
| Accent on | `--accent-on` | `#06111f` |
| Success / Warn / Danger | — | `#22c55e` / `#fbbf24` / `#fb7185` |

- Use `--accent` for the primary action, links, focus, and active states; `--accent-hover` / `--accent-active` for interaction.
- Reserve `--success` / `--warn` / `--danger` for state semantics only; keep body copy on `--fg` / `--fg-2` for contrast.

## 3. Typography

- **Display:** `Inter, system-ui, sans-serif`
- **Body:** `Inter, system-ui, sans-serif`
- **Mono:** `"JetBrains Mono", ui-monospace, Menlo, monospace`
- **Scale:** `--text-xs 12px / sm 13px / base 15px / lg 17px / xl 22px / 2xl 32px / 3xl 48px / 4xl 66px`.
- **Line height:** body `--leading-body 1.55`, display `--leading-tight 1.06`; display tracking `--tracking-display -0.02em`.
- **Weights:** 400 body, 500 UI/labels, 600–700 headings. Lead with Inter for display, set running text in Inter, and use JetBrains Mono for code/technical labels.

## 4. Spacing, Grid & Layout

- **Spacing scale:** `4px / 8px / 12px / 16px / 20px / 24px / 32px / 48px` (`--space-1`…`--space-12`).
- **Container:** `--container-max 1200px`; gutters `36px / 24px / 16px` (desktop / tablet / phone).
- **Section rhythm:** `--section-y-desktop 96px`, tablet `68px`, phone `48px`.
- Keep a consistent vertical rhythm and align modules to a predictable grid; lead each block with headline → support → primary action.

## 5. Components

Match the bundled fixture vocabulary (`.btn`, `.btn-primary`, `.btn-secondary`, `.field`, `.panel`, `.tile`, `.status`, `.eyebrow`, `.lead`).
- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on` text, radius `--radius-md 12px`, padding `--space-2`/`--space-5`; hover → `--accent-hover`. `.btn-secondary`: `--surface` fill, 1px `--border`, `--fg` text. Focus-visible: `--focus-ring 0 0 0 4px rgba(96, 165, 250, 0.28)`.
- **Cards / panels** (`.panel`, `.tile`): `--surface` fill, structure via `--border`, radius `--radius-lg 20px`, elevation `--elev-raised` for raised states.
- **Inputs** (`.field`): `--surface` fill, 1px `--border`, radius `--radius-sm 8px`, explicit label + focus (`--focus-ring`) + error states; placeholders use `--muted`.
- **Badges / status** (`.status`): semantic color only, `--radius-pill 9999px` pills.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `0 24px 72px rgba(0, 0, 0, 0.36)` |

Apply elevation deliberately, in line with this style's character — never elevate every surface at once.

## 7. Motion & Interaction

- Durations `--motion-fast 130ms` (hover/focus) and `--motion-base 220ms` (entrances); easing `--ease-standard cubic-bezier(0.2, 0, 0, 1)`.
- Transition color, border, shadow, and transform — not layout. Define hover, focus-visible (`--focus-ring`), active, disabled, and loading states for every interactive element.

## 8. Responsive Behavior

- Collapse multi-column layouts to a single column under ~640px; step section padding down the `--section-y-*` ladder and reduce gutters to `--container-gutter-phone 16px`.
- Preserve the spacing rhythm and this style's signature treatments at every breakpoint; keep tap targets comfortable.

## 9. Do's and Don'ts

**Do**
- Pair chat with a working canvas.
- Surface tool/run states clearly.
- Keep streaming output legible.
- Bind `tokens.css` into `:root` and reference every value via `var(--*)`.

**Don't**
- Don't hide agent activity.
- Don't use noisy distracting chrome.
- Don't blur run-state feedback.
- Don't invent off-palette colors or redefine the token contract.

## 10. Agent Prompt Guide

- **Hero:** `--bg #0b1020` canvas; `.eyebrow` uppercase `--muted` → headline Inter 600 `--text-4xl` (`--leading-tight`, tracking `-0.02em`) `--fg` → `.lead` `--fg-2` `--text-xl` → `.btn-primary` (`--accent #60a5fa`) + `.btn-secondary`.
- **Card:** `--surface` fill, `--border` structure, `--radius-lg 20px`, `--elev-raised` when raised; title `--fg` 600, body `--fg-2`.
- **Iteration rules:** (1) chat + canvas dual layout; (2) accent `#60a5fa` drives interaction; (3) keep type in Inter/Inter; (4) honor the spacing + radius scale exactly.
