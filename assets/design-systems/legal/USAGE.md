# Counsel Usage

Design package guide for SenWeaverCoding agents and reviewers.

## Read Order

1. Read this file for the package contract.
2. Read `DESIGN.md` for visual intent, constraints, and anti-patterns.
3. Paste the `:root` block from `tokens.css` into the first artifact `<style>`.
4. Use `components.manifest.json` for the component inventory and token bindings.

## Design Highlights

- Near-monochrome deep-navy palette on white and parchment surfaces.
- Lora/Tiempos serif display voice + Inter for UI and tables.
- A discreet gold hairline reserved for section rules and seals only.
- Restrained geometry (2/4/8px radii), minimal elevation, generous whitespace.

## Do

- Lead with the serif voice; let whitespace and typography carry authority.
- Use document-grade tables with right-aligned figures for matters and billing.
- Cap reading measure around 68ch.

## Avoid

- Avoid bright accents, rounded "app" styling, gradients, and animated flourishes.
- Avoid using gold for buttons (rules and seals only).
- Avoid raw hex outside the copied `:root` token block.
