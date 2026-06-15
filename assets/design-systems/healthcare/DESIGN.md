# Design System — Vital (Healthcare & Medical)

> Category: Healthcare & Medical
> Calm clinical interface for patient portals, telehealth, and care dashboards — pristine white surfaces, a single trustworthy medical teal, and AAA-leaning legibility. Reassuring and precise, never alarming.

## 1. Visual Theme & Atmosphere

Vital is built for trust under stress. Surfaces are pristine white (`--bg`/`--surface #ffffff`) with a cool clinical tint for grouping (`--surface-warm #f1f6f8`). Ink is a deep desaturated teal-slate (`--fg #16323a`) that reads calm rather than corporate-black. The single accent is a medical teal `--accent #0e7490` — the color of clinical trust — used for primary actions, links, and active states. Status color is strictly rationed: green for normal, amber for attention, red for critical only — so a red badge always means something.

Elevation is whisper-soft (`--elev-raised`), motion is gentle (`--motion-base 240ms`), and contrast leans toward WCAG AAA so the UI stays comfortable across long clinical sessions and for low-vision patients.

**Key characteristics**
- Pristine white surfaces, calm teal-slate ink, one trustworthy teal accent.
- AAA-leaning contrast and a generous reading scale for legibility.
- Rationed status color — red/amber mean "act now", never decoration.
- Soft low elevation and gentle motion; nothing alarming or flashy.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background / Surface | `--bg` / `--surface` | `#ffffff` | Canvas, cards, sheets |
| Surface warm | `--surface-warm` | `#f1f6f8` | Grouped rows, secondary fills |
| Foreground | `--fg` | `#16323a` | Headings, primary text |
| Foreground 2 | `--fg-2` | `#2c4a52` | Body copy |
| Muted | `--muted` | `#5a7178` | Labels, metadata |
| Border | `--border` | `#d4e1e6` | Dividers, card edges |
| Accent | `--accent` | `#0e7490` | Primary actions, links, focus |
| Success / Warn / Danger | — | `#1f9d55` / `#b9770b` / `#c0392b` | Vitals/state semantics only |

- Keep total semantic-color area small; a critical red must never compete with decorative color.
- All body text on `--fg`/`--fg-2`; verify AA+ (aim AAA) for clinical data.

## 3. Typography

- **Display & Body:** `Inter` (system fallback stack); **Mono:** `ui-monospace` for IDs, dosages, lab values.
- **Scale:** `xs 12 / sm 14 / base 16 / lg 19 / xl 23 / 2xl 30 / 3xl 40 / 4xl 52`.
- **Line height:** body `--leading-body 1.55`, display `--leading-tight 1.22`; tracking `--tracking-display -0.01em`.
- **Weights:** 400 body, 500 labels, 600 headings. Favor clarity over flourish; never set clinical data below 14px.

## 4. Spacing, Grid & Layout

- 8px grid: `4 / 8 / 12 / 16 / 20 / 24 / 32 / 48`; container `--container-max 1200px`; section rhythm `64 / 48 / 32px`.
- Group related vitals/records into clearly bordered cards; keep generous padding so dense medical data stays scannable.

## 5. Components

- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on #ffffff`, `--radius-md 10px`; hover `--accent-hover`. `.btn-secondary`: `--surface` + 1px `--border`, `--accent` text.
- **Cards** (`.panel`, `.tile`): `--surface`, 1px `--border`, `--radius-lg 16px`, `--elev-raised`; vital cards pair a big mono figure with a `--muted` label and a semantic status dot.
- **Inputs** (`.field`): `--surface`, 1px `--border`, `--radius-sm 6px`, explicit labels, helper text, and error states; focus `--focus-ring 0 0 0 3px rgba(14,116,144,0.35)`.
- **Status** (`.status`): pill with semantic color + text label (never color alone — accessibility).

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `rgba(16,50,58,.04) 0 1px 2px, rgba(16,50,58,.06) 0 4px 12px -2px` |

Soft, low, reassuring. Cards rest on a faint lift; nothing floats dramatically.

## 7. Motion & Interaction

- Durations `--motion-fast 160ms` / `--motion-base 240ms`, easing `cubic-bezier(0.2,0,0,1)`.
- Gentle fades/slides only; no bouncy or attention-grabbing motion. Always honor `prefers-reduced-motion`.

## 8. Responsive Behavior

- Collapse record/vital grids to one column on mobile; keep tap targets ≥44px for patients of all abilities.
- Preserve AAA contrast and reading scale at every breakpoint; step section padding to `32px`.

## 9. Do's and Don'ts

**Do** — keep one teal accent; ration status color; pair color with text labels; aim for AAA contrast; keep motion gentle.

**Don't** — use alarming reds decoratively; rely on color alone for status; crowd clinical data; introduce playful/loud styling.

## 10. Agent Prompt Guide

- **Patient card:** `--surface` white, 1px `--border`, `--radius-lg`, `--elev-raised`; vital value in `--font-mono` `--text-3xl` `--fg`, label `--muted`, status dot semantic.
- **Iteration rules:** (1) one teal accent; (2) status color = action, with text labels; (3) AAA legibility; (4) gentle, reduced-motion-safe interactions.
