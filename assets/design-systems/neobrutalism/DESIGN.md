# Design System — Neobrutalism

> Category: Bold & Expressive
> Loud, confident neo-brutalist UI: thick hard outlines, blunt offset shadows that never blur, warm cream paper, and a shouting orange-red accent.

## 1. Visual Theme & Atmosphere

Neobrutalism rejects polish. Surfaces are flat warm cream (`--bg #fff4cf`, `--surface #fffaf0`, `--surface-warm #ffdca8`), edges are thick and uncompromising, and depth is a hard, zero-blur offset block: `--elev-raised: 6px 6px 0 rgba(42,24,16,0.26)`. Nothing fades; shadows are solid rectangles that make elements look stamped onto the page. The accent is a loud `--accent #d24b1f` orange-red used without apology for primary actions and emphasis.

Display type is heavy and tight — `--font-display: Arial Black, Impact, sans-serif` with `--tracking-display 0` — so headlines hit like posters. Borders read as ink-drawn (use the warm `--border #d9aa7a` or a near-black for maximum punch). The personality is honest, energetic, and a little punk.

**Key characteristics**
- Hard offset shadows with **zero blur** (`6px 6px 0`) — the defining move.
- Thick, visible borders; flat warm cream fills; no gradients.
- Heavy condensed display type (Arial Black / Impact) at poster scale.
- One loud accent (`#d24b1f`) that is meant to shout.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background | `--bg` | `#fff4cf` | Warm cream canvas |
| Surface | `--surface` | `#fffaf0` | Cards, panels |
| Surface warm | `--surface-warm` | `#ffdca8` | Highlight blocks |
| Foreground | `--fg` | `#2a1810` | Headings, body, outlines |
| Foreground 2 | `--fg-2` | `#593625` | Secondary text |
| Muted | `--muted` | `#8a6652` | Captions |
| Border | `--border` | `#d9aa7a` | Outlines (go darker toward `--fg` for max punch) |
| Accent | `--accent` | `#d24b1f` | CTAs, emphasis, focus |
| Success / Warn / Danger | — | `#3d8f4f` / `#f2a93b` / `#b83a2f` | Status |

- Borders should be 2–3px and clearly visible — thin hairlines betray the style.
- The offset shadow uses `--fg`-toned ink; keep it solid (no blur, no opacity fade beyond the token).

## 3. Typography

- **Display:** `Arial Black, Impact, sans-serif`, weight 700–900, tracking `0`, `--leading-tight 1.06`.
- **Body:** `Inter, system-ui, sans-serif`; mono `"SF Mono"`.
- **Scale:** `xs 12 / sm 14 / base 16 / lg 18 / xl 24 / 2xl 36 / 3xl 54 / 4xl 76`.
- Headlines are uppercase or sentence case but always heavy; body stays plain Inter for readability.

## 4. Spacing, Grid & Layout

- Spacing `4 / 8 / 12 / 16 / 20 / 24 / 32 / 48`; container `1180px`; section rhythm `96 / 68 / 48px`.
- Layouts are blocky and grid-snapped; let elements overlap slightly and cast their offset shadow onto neighbors.

## 5. Components

- **Buttons** — `.btn-primary`: `--accent` fill, `--accent-on #ffffff` text, 2–3px `--fg` border, `--radius-sm 4px`, hard `--elev-raised`; on press, translate by the shadow offset so it "clicks" flat.
- **Cards / panels** (`.panel`, `.tile`): `--surface`, thick border, `--radius-lg 12px`, hard offset shadow.
- **Inputs** (`.field`): `--surface`, thick border, `--radius-sm`, `--focus-ring 0 0 0 4px rgba(210,75,31,0.28)`.
- **Badges** (`.status`): solid blocks with thick borders; no soft pills unless using `--radius-pill` intentionally.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `6px 6px 0 rgba(42,24,16,0.26)` — **hard, no blur** |

The hard offset is non-negotiable. On interaction, move the element toward its shadow to flatten it.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` / `--motion-base 240ms`, easing `cubic-bezier(0.2,0,0,1)`.
- Prefer snappy translate/press effects over fades; the UI should feel physical and immediate.

## 8. Responsive Behavior

- Keep borders and offset shadows at full weight on mobile — they are the identity.
- Stack blocks vertically; reduce display size but never below a confident, heavy scale.

## 9. Do's and Don'ts

**Do** — use thick visible borders; hard zero-blur offset shadows; heavy display type; one loud accent.

**Don't** — blur shadows or soften edges; use thin hairlines; introduce gradients or pastel washes; mute the accent.

## 10. Agent Prompt Guide

- **Hero:** cream `--bg`, heavy `Arial Black` headline `--text-4xl` `--fg`, `.btn-primary` orange-red with 3px `--fg` border + `6px 6px 0` shadow.
- **Card:** `--surface`, 3px `--fg` border, `--radius-lg`, hard offset shadow; press translates `6px 6px`.
- **Iteration rules:** (1) shadows never blur; (2) borders stay thick; (3) display type is heavy; (4) the accent shouts.
