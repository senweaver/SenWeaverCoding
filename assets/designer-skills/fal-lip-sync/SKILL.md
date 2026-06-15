---
name: fal-lip-sync
description: |
  Create talking head videos and lip sync audio to video via fal.ai. Useful for explainer avatars, multilingual dubbing previews, and social cuts.
triggers:
  - "lip sync"
  - "talking head"
  - "audio to video"
  - "avatar video"
  - "fal lipsync"
od:
  mode: video
  category: video-generation
---

# fal-lip-sync

## What it does

Create talking head videos and lip sync audio to video via fal.ai. Useful for explainer avatars, multilingual dubbing previews, and social cuts.

## Metadata

- Category: `video-generation`

## How to use in SenWeaverCoding

This skill is embedded in SenWeaverCoding and injected into the Designer
system prompt when it matches the active sub-mode. Invoke it by name
(`fal-lip-sync`) or via one of its trigger phrases. Its bundled templates,
checklists, and reference files are read on demand with the
`designer_skill_read` tool (`id: fal-lip-sync`, `path: <relative file>`) —
call `designer_skill_read` with `path: SKILL.md` first if you need the
full body, then pull only the side files a given step requires.

Production flow:

1. **Plan** the scene, motion, duration, and aspect ratio.
2. **Generate** through the `media_generate` tool (`surface: video`):
   submit the job, poll until the provider returns the rendered file, then
   write it into the session directory.
3. **Present** the `.mp4`/`.webm` in the canvas with a text description.

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
- **Await the provider render** before finishing; never claim a video is
  ready while the job is still processing.

<!-- swc-strengthened -->
