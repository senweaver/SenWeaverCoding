# Arcade Usage

Design package guide for SenWeaverCoding agents and reviewers.

## Read Order

1. Read this file for the package contract.
2. Read `DESIGN.md` for visual intent, constraints, and anti-patterns.
3. Paste the `:root` block from `tokens.css` into the first artifact `<style>`.
4. Use `components.manifest.json` for the component inventory and token bindings.

## Design Highlights

- Three-step dark stack with a single neon-magenta accent (cyan secondary inline).
- Condensed geometric display face (uppercase, +0.04em) + mono for scores/timers.
- Glow-edged elevation used sparingly; double neon focus ring on dark surfaces.
- Sharp technical radii (4/8/14px).

## Do

- Keep darkness dominant; aim neon at live, winning, and actionable elements.
- Use mono numerals for all competitive numbers (scores, KDA, timers).
- Cap accent + glow usage per viewport so it stays special.

## Avoid

- Avoid rainbow neon everywhere and low-contrast gray-on-black text.
- Avoid glow on every element — it stops meaning anything.
- Avoid raw hex outside the copied `:root` token block.
