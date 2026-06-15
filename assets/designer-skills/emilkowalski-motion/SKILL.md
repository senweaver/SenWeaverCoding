---
name: emilkowalski-motion
description: |
  Motion-design follow-up skill inspired by Emil Kowalski's animation guidance. Use after an interface exists to add tasteful micro-interactions, state transitions, and page motion with product-grade restraint.
triggers:
  - "emil kowalski"
  - "motion polish"
  - "micro interaction"
  - "interaction animation"
  - "tasteful animation"
  - "动效润色"
od:
  mode: prototype
  surface: web
  platform: desktop
  category: animation-motion
  preview:
    type: html
  design_system:
    requires: true
  craft:
    requires:
      - animation-discipline
      - accessibility-baseline
  example_prompt: |
    Use emilkowalski-motion on the current HTML artifact: add restrained micro-interactions, state transitions, and reduced-motion fallbacks without changing the core layout.
---

# Emil Kowalski Motion Follow-Up

Use this skill after a design artifact already exists. The goal is to make the interface feel alive without turning it into a motion demo.

## Workflow

1. Inspect the current HTML, component, or selected page element before adding animation.
2. Pick the smallest set of motion moments that clarify state or hierarchy:
   - entry reveal for the primary content
   - hover / active feedback for important controls
   - transition between UI states
   - scroll reveal only when it helps the story
3. Prefer `transform` and `opacity`. Avoid animating layout properties such as `top`, `left`, `width`, or `height`.
4. Use one motion language across the artifact. Do not mix unrelated easings, durations, or physics.
5. Add `prefers-reduced-motion` fallbacks for any automatic or scroll-linked motion.
6. Keep copy, data, and layout intent intact unless the user explicitly asks for a redesign.

## Motion Rules

- Default UI transitions should feel quick and useful: 140-220ms for most controls.
- Larger page reveals can be slower, but must not block reading.
- Avoid endless decorative loops unless they communicate status or progress.
- Do not add custom cursors, noisy particle effects, or motion that competes with content.
- Stagger only small groups. Long staggered lists make interfaces feel slow.

## Implementation Notes

- For plain HTML, CSS keyframes and small JavaScript observers are enough.
- For React or framework code, use the local stack already present in the repo.
- If GSAP is available and the motion needs sequencing, pair this with `gsap-core`, `gsap-timeline`, or `gsap-scrolltrigger`.
- Always clean up observers, timers, and animation instances.

## SenWeaverCoding strengthening

Hard requirements for every artifact this skill produces inside Designer
mode, regardless of the workflow above:

- **Bind to the active design system.** Pull palette, type, spacing, and
  radius from the selected `DESIGN.md` / `tokens.css`; never hardcode an
  off-token palette or invent a second accent.
- **Apply the craft layer.** Honour the injected craft references
  (typography, color, anti-ai-slop, state-coverage, accessibility) — they
  are P0 self-checks at the critique stage, not suggestions.
- **Write into the session directory** so the artifact appears in the
  per-session canvas; keep files self-contained (inline assets, no external
  CDNs that break in the sandboxed iframe).
- **Use real content, never filler.** No Lorem Ipsum, no invented metrics,
  no placeholder copy — compose to fill empty space.
- **Ship every interactive state**, not just the populated one, and make
  the design responsive and keyboard-operable.

<!-- swc-strengthened -->
