# Color craft rules

Universal color rules applied on top of the active `DESIGN.md`. The
design system supplies the palette tokens; this file enforces how to
*use* them.

## Palette structure

A coherent palette has four layers. Plan all four before writing any CSS.

| Layer | Share of pixels | Tokens |
|---|---|---|
| **Neutrals** | 70–90% | `--bg`, `--surface`, `--fg`, `--muted`, `--border` |
| **Accent** (one) | 5–10% | `--accent` only — never invent a second accent |
| **Semantic** | 0–5% | `--success`, `--warn`, `--danger` |
| **Effect** | <1% | gradients, glows; rarely justified |

## Accent discipline

The single biggest readability failure in AI-generated UIs is accent
overuse. Hard caps:

- **At most 2 visible uses of `--accent` per screen.** Typical pair:
  one eyebrow / chip + one primary CTA. Or one accent card + one tab
  pill. Pick a pair, not a flood.
- Links count as accent; demote to `--fg` underline if you also have a
  CTA on the same screen.
- Hover/focus rings count as accent. Ration accordingly.

## Contrast minimums

Run these as gates, not goals:

| Pair | Minimum |
|---|---|
| Body text (≤16 px) on background | **4.5:1** |
| Large text (>18 px or 14 px bold) | **3:1** |
| UI components against adjacent surfaces | **3:1** |

When the brand color clashes (low-contrast indigo on light background is
common), darken the accent to a `600`-level shade for text use; reserve
the brand-bright variant for fills only.

## Dark themes

Avoid pure black and pure white — both cause vibration and eye strain.

| Token | Dark theme | Light theme |
|---|---|---|
| Background | `#0f0f0f` (not `#000`) | `#fafafa` (not `#fff`) |
| Foreground | `#f0f0f0` (not `#fff`) | `#111111` (not `#000`) |

On dark surfaces, prefer **semi-transparent white borders** over solid
dark borders — a 1px `rgba(255,255,255,0.08)` reads as structure
without adding visual noise.

## Semantic color naming

Always name tokens by **purpose**, never by hue:

```css
/* good */
--accent: #2f6feb;
--success: #17a34a;

/* bad — locks you out of theming */
--blue-500: #2f6feb;
--green-500: #17a34a;
```

## Anti-defaults

- **Indigo `#6366f1`** (Tailwind `indigo-500`) is the most reliable
  AI-slop tell. The active `DESIGN.md` provides `--accent`; use it. If
  the brief truly needs indigo, make the user say so explicitly. If
  your `DESIGN.md` encodes indigo as `--accent`, that is intentional —
  the linter only flags hardcoded hex, so `var(--accent)` uses are
  unaffected even when the resolved color happens to be `#6366f1`.
- **Two-stop "trust" gradient** (purple → blue, blue → cyan, etc.) on a
  hero is the second most reliable tell. A flat surface + one
  type-driven hierarchy beats it every time.
- **Decorative gradients with no functional purpose**. Gradients should
  separate hierarchies (header → body, primary CTA → secondary), not
  decorate empty space.

## SenWeaverCoding production additions

- **Derive states, don't hand-pick them.** Compute hover/active/disabled
  from the token instead of inventing new hex:
  `background: color-mix(in oklab, var(--accent) 88%, black)` for a hover,
  `color-mix(in oklab, var(--accent) 40%, var(--surface))` for a muted
  fill. One source of truth, perfectly consistent.
- **Mix in OKLab/OKLCH, not sRGB.** Perceptual spaces keep mid-tones from
  going muddy or fluorescent. Prefer `color-mix(in oklab, …)` and, for
  ramps, vary lightness in `oklch()` while holding hue/chroma.
- **Respect user color modes.** Support `@media (prefers-color-scheme)`
  for dark/light, `@media (prefers-contrast: more)` to thicken borders and
  push text contrast, and `@media (forced-colors: active)` — never remove
  outlines there; rely on `CanvasText`/`Canvas` system colors.
- **Tint neutrals toward the accent.** Pure-gray surfaces read as
  unfinished. Mix 3–6% of `--accent` into `--surface`/`--border` so the
  neutrals feel part of the same world as the brand color.
- **Contrast is a gate, verified in the canvas.** After generating, eyeball
  the rendered unit (not the source) for the 4.5:1 / 3:1 minimums; the most
  common miss is accent-on-accent text inside CTAs and chips.
