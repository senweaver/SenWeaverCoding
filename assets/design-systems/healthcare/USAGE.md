# Vital Usage

Design package guide for SenWeaverCoding agents and reviewers.

## Read Order

1. Read this file for the package contract.
2. Read `DESIGN.md` for visual intent, constraints, and anti-patterns.
3. Paste the `:root` block from `tokens.css` into the first artifact `<style>` before writing component CSS.
4. Use `components.manifest.json` for the component inventory and token bindings.

## Design Highlights

- Single medical-teal accent (`#0e7490`) on pure-white surfaces.
- AAA-leaning contrast, 16px body at 1.55 leading for long clinical reading.
- Status color (green / amber / red) is rationed and always paired with icon + label.
- 44px minimum targets; an always-visible teal focus ring.

## Do

- Keep the page calm and let data lead; cap accent fills at two per viewport.
- Show units, ranges, and timestamps explicitly; right-align numeric columns.
- Use `tabular-nums` for vitals, dosages, and lab values.

## Avoid

- Avoid alarm-red fills for routine actions and decorative gradients.
- Avoid raw hex outside the copied `:root` token block.
- Avoid color-only status encoding (fails accessibility).
