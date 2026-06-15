# Design System — Campus (Education & Learning)

> Category: Education & Learning
> Friendly, focus-driven system for e-learning, courseware, lesson players, and student dashboards — approachable indigo on warm-tinted white, reading-optimized type, soft rounded geometry, and encouraging progress cues. Supportive, never childish.

## 1. Visual Theme & Atmosphere

Campus is built to keep learners engaged and unintimidated. Surfaces are a warm-tinted white (`--bg #fbfaff`, `--surface #ffffff`) with an indigo-tinted alternate (`--surface-warm #f3f1fd`) for lesson groupings. Ink is a deep indigo-slate (`--fg #1e1b3a`); the accent is an approachable indigo `--accent #4f46e5` used for primary actions, links, and progress. Display type is `Lexend` — engineered for reading proficiency — and body is `Inter` with a generous `--leading-body 1.62` for comprehension. Geometry is soft and friendly (`--radius-sm 8`, `--radius-lg 20`); progress and encouragement cues are first-class.

**Key characteristics**
- Warm-tinted white surfaces, approachable indigo accent.
- Lexend display tuned for reading; generous 1.62 line height for comprehension.
- Soft rounded geometry; encouraging progress/streak cues.
- Supportive and motivating for learners of every age — friendly, not childish.

## 2. Color & Roles

| Role | Token | Value | Use |
|------|-------|-------|-----|
| Background | `--bg` | `#fbfaff` | Warm-tinted canvas |
| Surface / warm | `--surface` / `--surface-warm` | `#ffffff` / `#f3f1fd` | Cards, lesson groups |
| Foreground | `--fg` | `#1e1b3a` | Headings, primary text |
| Foreground 2 | `--fg-2` | `#34305a` | Body, lesson copy |
| Muted | `--muted` | `#635e84` | Labels, metadata |
| Border | `--border` | `#e1ddf2` | Dividers, card edges |
| Accent | `--accent` | `#4f46e5` | Actions, links, progress |
| Success / Warn / Danger | — | `#16a34a` / `#d97706` / `#dc2626` | Correct/attention/incorrect |

- Use `--success` for correct answers/completion, `--danger` sparingly for incorrect — always with text/icon, not color alone.
- Indigo accent drives progress bars, active steps, and primary CTAs.

## 3. Typography

- **Display:** `Lexend, Inter, sans-serif`; **Body:** `Inter`; **Mono:** `ui-monospace` for code lessons.
- **Scale:** `xs 12 / sm 14 / base 16 / lg 20 / xl 24 / 2xl 32 / 3xl 42 / 4xl 56`.
- **Line height:** body `--leading-body 1.62` (reading-optimized), display `--leading-tight 1.2`; tracking `--tracking-display -0.015em`.
- **Weights:** 400 body, 500 labels, 600 headings. Keep lesson text generously spaced for sustained reading.

## 4. Spacing, Grid & Layout

- 8px grid: `4 / 8 / 12 / 16 / 20 / 24 / 32 / 48`; container `--container-max 1140px`; section rhythm `72 / 48 / 32px`.
- Lesson layouts: clear step sequence, visible progress, one primary action ("Continue"); constrain reading width for comfort.

## 5. Components

- **Buttons** — `.btn-primary`: `--accent` indigo fill, `--accent-on #ffffff`, `--radius-md 12px`; hover `--accent-hover`. `.btn-secondary`: `--surface` + 1px `--border`, `--accent` text.
- **Cards** (`.panel`, `.tile`): `--surface`, 1px `--border`, `--radius-lg 20px`, `--elev-raised`; course cards show progress bar + completion state.
- **Inputs/quiz** (`.field`): `--surface`, 1px `--border`, `--radius-sm 8px`, focus `--focus-ring 0 0 0 3px rgba(79,70,229,0.4)`; correct/incorrect states use semantic color + icon + text.
- **Progress:** indigo fill on soft track; streak/badge chips for encouragement.

## 6. Depth & Elevation

| Level | Token | Treatment |
|-------|-------|-----------|
| Flat | `--elev-flat` | `none` |
| Ring | `--elev-ring` | `0 0 0 1px var(--border)` |
| Raised | `--elev-raised` | `rgba(79,70,229,.05) 0 1px 2px, rgba(30,27,58,.08) 0 8px 24px -6px` |

Soft, friendly lift on cards; keep it light and approachable.

## 7. Motion & Interaction

- Durations `--motion-fast 150ms` / `--motion-base 220ms`, easing `cubic-bezier(0.2,0,0,1)`.
- Encouraging micro-feedback (checkmarks, progress fills, gentle celebrations on completion). Honor `prefers-reduced-motion`.

## 8. Responsive Behavior

- Collapse course grids to one column on mobile; keep the lesson reading width comfortable.
- Preserve progress visibility and generous line height at every breakpoint; step section padding to `32px`.

## 9. Do's and Don'ts

**Do** — keep reading comfortable (Lexend, 1.62); surface progress and encouragement; use soft rounded shapes; pair correctness with icon+text.

**Don't** — feel childish or condescending; cramp lesson text; punish errors harshly; rely on color alone for right/wrong.

## 10. Agent Prompt Guide

- **Course card:** `--surface`, 1px `--border`, `--radius-lg 20px`, `--elev-raised`; title `Lexend` `--fg`, progress bar `--accent`, "3/8 lessons" `--muted`; CTA `.btn-primary` indigo "Continue".
- **Iteration rules:** (1) reading-optimized Lexend + 1.62 leading; (2) indigo drives progress/actions; (3) soft friendly radii; (4) encouraging, accessible feedback.
