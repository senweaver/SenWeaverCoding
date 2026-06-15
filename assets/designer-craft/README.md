# Craft references

Brand-agnostic craft knowledge. Each file is a small, dense rulebook on one
dimension of professional UI craft (typography, color, motion, …). Skills
opt into the references they need; the daemon injects only the requested
ones into the system prompt above the active skill body.

## Why a third axis next to `skills/` and `design-systems/`

| Axis | Scope | Example |
|---|---|---|
| `skills/` | Artifact shape | `saas-landing`, `dashboard`, `pricing-page` |
| `design-systems/` | Brand visual language (the 9-section `DESIGN.md`) | `linear-app`, `apple`, `notion` |
| `craft/` | **Universal** craft knowledge — true regardless of brand | letter-spacing rules, accent-overuse caps, anti-AI-slop |

`DESIGN.md` tells the agent which colors and fonts a brand uses. `craft/`
tells the agent the universal rules a competent designer applies on top —
e.g. ALL CAPS always needs ≥0.06em tracking, regardless of the brand.

## How a skill opts in

Add an `od.craft.requires` array to the skill's front-matter. Only the
listed sections are injected, so a skill that needs only typography pays
no token cost for color/motion content.

```yaml
od:
  craft:
    requires: [typography, color, anti-ai-slop]
```

Use the layered stack for editorial skills that require authored hierarchy
and sustained reading behavior:

```yaml
od:
  craft:
    requires: [typography, typography-hierarchy, typography-hierarchy-editorial]
```

Allowed values match the file names in this directory minus the `.md`
extension. Unknown values are silently ignored (forward-compatible).

### Why silent fallback instead of fail-fast?

A skeptical reader will ask: "If a skill requests a planned-but-not-yet-vendored
section and the corresponding file doesn't exist yet, shouldn't we warn
the user?" We chose forward-compatibility over fail-fast: a skill
authored today can list a planned slug and start benefiting the moment
the matching `craft/<slug>.md` is vendored in a follow-up PR, with no
skill edit needed. The cost of a missed reference is a missing
paragraph in the system prompt, not a broken skill — so the loud
failure mode is not worth the friction.

Note for skill authors arriving from older guidance: an earlier draft
used `motion` as the future-slug placeholder. The shipped equivalent
today is `animation-discipline`. Use that one if your skill emits
motion.

### Enforcement levels

In SenWeaverCoding craft rules are enforced by the agent itself, not by a
separate linter daemon. The discipline lives in the design pipeline:

- **P0 (must-fix).** The hard rules — currently the P0 list in
  `anti-ai-slop.md` (Tailwind-indigo accent, two-stop hero gradients,
  emoji-as-icons, etc.). Self-verify each one during the **critique**
  stage before declaring an artifact done; a P0 hit is a regression.
- **P1 / judgement.** The rest. The agent reads the rules and applies
  them; deliberate, defensible exceptions are allowed.

A purely behavioral craft file (state-coverage, animation-discipline) is
self-checked the same way: render the artifact in the Designer canvas,
screenshot it, and confirm the rule holds in the *rendered* unit, not
just in the source.

## Files

| File | Section name | When to require |
|---|---|---|
| `typography.md` | `typography` | Any skill that emits typed content (~all skills) |
| `typography-hierarchy.md` | `typography-hierarchy` | Any skill that emits typed content where hierarchy must feel authored, not assembled — especially surfaces with a strong entry point, varied levels, or intentional rhythm. Compose with `typography`. |
| `typography-hierarchy-editorial.md` | `typography-hierarchy-editorial` | Skills whose primary artifact is a sustained reading surface: `blog-post`, `docs-page`, `digital-eguide`. Requires `typography` + `typography-hierarchy`. |
| `color.md` | `color` | Any skill that emits styled output (~all skills) |
| `anti-ai-slop.md` | `anti-ai-slop` | Marketing pages, landing pages, decks |
| `state-coverage.md` | `state-coverage` | Any skill with stateful UI (dashboards, mobile apps, forms, list/table views) |
| `animation-discipline.md` | `animation-discipline` | Any skill that ships motion: mobile apps, multi-screen flows, gamified UI, transitions, microinteractions |
| `accessibility-baseline.md` | `accessibility-baseline` | Any skill that ships interactive UI: dashboards, forms, mobile flows, anything with focus/labels/keyboard paths |
| `rtl-and-bidi.md` | `rtl-and-bidi` | Any skill that ships localized text or layout: blogs, docs, financial tables, mobile apps, anything that may render Arabic / Hebrew / Persian |
| `form-validation.md` | `form-validation` | Any skill whose primary artifact contains an interactive form: lead capture, sign-in, signup, settings, multi-step intake |
| `laws-of-ux.md` | `laws-of-ux` | Any skill whose composition decisions hit named cognitive limits: pricing pages (Hick's, Choice Overload, Von Restorff), dashboards (Pareto, Selective Attention, Working Memory), onboarding (Goal-Gradient, Zeigarnik, Peak-End), modals (Fitts's, Tesler's). Sibling axis to the rendering-rule files above — covers what to compose, not how to render. |

**Partial-stateful skills.** A skill that's mostly static but contains an embedded form, data table, or query surface should opt in. State-coverage rules apply to the stateful component, not the whole page.

## How SenWeaverCoding injects craft

When a skill declares `od.craft.requires`, the Designer backend reads each
listed `craft/<slug>.md` and injects it into the system prompt under
**"Active craft references"**, layered above the active design system and
the skill body. This is the live wiring in `src/agent/designer/skill.rs`
(`injection`) — it resolves the craft files for the selected skill at
prompt-build time, so adding a new `craft/<slug>.md` and referencing it
from a skill's front-matter is all that's needed to ship a new rulebook.
