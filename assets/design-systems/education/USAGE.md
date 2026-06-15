# Campus Usage

Design package guide for SenWeaverCoding agents and reviewers.

## Read Order

1. Read this file for the package contract.
2. Read `DESIGN.md` for visual intent, constraints, and anti-patterns.
3. Paste the `:root` block from `tokens.css` into the first artifact `<style>`.
4. Use `components.manifest.json` for the component inventory and token bindings.

## Design Highlights

- Single indigo accent (`#4f46e5`) on a warm-tinted white canvas.
- Lexend headings + Inter body, 16px at 1.62 leading for comfortable reading.
- Encouraging progress, streak, and quiz cues; completion flips to green.
- Soft rounded geometry and an always-visible indigo focus ring.

## Do

- Celebrate progress positively; keep one accent per viewport.
- Pair quiz/answer state color with icon + text.
- Cap reading measure around 70ch.

## Avoid

- Avoid childish clip-art and harsh red for routine "incomplete" states.
- Avoid cramped, dense layouts that fatigue learners.
- Avoid raw hex outside the copied `:root` token block.
