# Civic Usage

Design package guide for SenWeaverCoding agents and reviewers.

## Read Order

1. Read this file for the package contract.
2. Read `DESIGN.md` for principles, constraints, and anti-patterns.
3. Paste the `:root` block from `tokens.css` into the first artifact `<style>`.
4. Use `components.manifest.json` for the component inventory and token bindings.

## Design Highlights

- Near-black ink on white; a single functional blue for links and actions.
- Large 19px default body for low-vision and older users.
- Signature high-visibility yellow focus highlight on every interactive element.
- Rectangular controls, flat surfaces, no decoration.

## Do

- Use plain language and one transaction step per page.
- Keep the yellow focus highlight everywhere; never remove focus styling.
- Cap line length (~66ch) and left-align text.

## Avoid

- Avoid decorative gradients, rounded "app" styling, and shadows-for-style.
- Avoid color-only meaning and contrast below WCAG AA.
- Avoid raw hex outside the copied `:root` token block.
