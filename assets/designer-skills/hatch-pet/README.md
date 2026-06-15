# hatch-pet

This directory is a self-contained copy of the `hatch-pet` skill. It is
checked into the SenWeaverCoding repo so that:

- Any SenWeaverCoding agent can run the skill end-to-end without a network
  fetch, an extra install step, or an out-of-tree clone.
- The packaged desktop build can ship the skill as inert static assets
  alongside the rest of `skills/`.
- Reviews of changes that touch pet generation can see the skill source in
  the same diff as the daemon / web wiring that consumes it.

## Where outputs land

The skill packages each pet under
`${CODEX_HOME:-$HOME/.codex}/pets/<pet-id>/` with `pet.json` and
`spritesheet.{webp,png,gif}`. The daemon scans that directory in
`apps/daemon/src/codex-pets.ts`; the web pet settings list and one-click
adopt pets from there. See `docs/codex-pets.md` for the end-user setup
flow (including how SenWeaverCoding behaves when Codex is not installed).
