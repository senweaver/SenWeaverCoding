# SenWeaverCoding

[English](./README.md) · [简体中文](./README.zh-CN.md)

> **AI Code Editor & Autonomous Agent Runtime · Rust back-end + Tauri/React desktop**

SenWeaverCoding is a desktop AI code editor that ships the entire agent
runtime as a single installable application. The back-end is a Rust
library (`src/`) embedded directly into a Tauri 2 shell; the front-end
is a React + Vite UI living in `desktop/`. There is no sidecar process,
no remote service to talk to, and no separate CLI install — one
installer gives you the full IDE **and** a terminal-callable `sen`
command.

---

## Highlights

| Area | What you get |
| --- | --- |
| **Desktop-first** | Tauri 2 shell with native menus, multi-tab sessions, in-app PTY terminal, file browser, and an embedded browser dock for live web/UI work. |
| **Embedded Rust runtime** | The same crate that runs headless is loaded in-process via `crate-type = ["cdylib", "staticlib", "rlib"]`. The UI talks to it over a loopback WebSocket/HTTP gateway — no serialization-heavy IPC, no external daemon. |
| **Intent-routed coding modes** | `Auto` routes each turn to the best fit; or pin any of the 14 modes explicitly — **Agent** (default, full autonomy), **Vibe**, **Spec**, **Plan** (read-only plan authoring), **Ask** (read-only Q&A with citations), **TDD**, **Debug** (4-stage root-cause + in-app browser QA), **Architect**, **Pair**, **Context**, **MVAI**, **Harness**, **Curator** (research → DOCX / implementation blueprint), **Designer** (ten UI/design surfaces). Each mode rewires the system prompt, tool allowlist, approval policy, and auto-verify behavior. |
| **130+ tools** | File ops, PTY-mirrored shell, Git, ripgrep/content search, glob/multi-edit/patch-apply, **code intelligence** (tree-sitter outline + symbol graph for callers/implementors/uses + heuristic lexical code index), multi-engine web search/fetch, headless & embedded browser, SQLite + vector memory, todo/plan tracking, image generation, office docs (xlsx/pdf/docx), MCP tools, Skills, and Subagent delegation. |
| **Multi-provider** | OpenAI-compatible (incl. DeepSeek / Gemini-compatible endpoints), Anthropic, OpenRouter, GitHub Copilot, Claude Code, Ollama, Azure OpenAI, AWS Bedrock, Telnyx, and local CLI bridges. Provider keys and per-session model routing are configured in-app. |
| **Persistent memory & checkpoints** | SQLite + Markdown backends with vector embeddings, per-session work-dir isolation, and rewind/restore checkpoints. Interrupted turns are repaired and resumed against the *most recent* task on "continue". |
| **Automations & extensibility** | Cron-driven automations, Hooks, user Rules, Skills, MCP servers, multi-channel adapters (Slack / Telegram / Discord / Lark / … plus feature-gated Matrix), and first-party TypeScript & Python SDKs over the JSON-RPC 2.0 server. |
| **Performance-tuned** | Virtualized message list, rAF-coalesced streaming flushes, prioritized WebSocket heartbeats, content-aware context compaction, and `spawn_blocking` on IO hot paths. Hard constraints live in `AGENTS.md`. |

---

## Install

Pre-built installers are published on the
[Releases](https://github.com/senweaver/SenWeaverCoding/releases) page.
**Every installer ships both the desktop app and the `sen` CLI** so a
single install gives you the GUI and a terminal-callable `sen` command.

| Platform | Asset | What lands on your system |
| --- | --- | --- |
| **Windows x64** | `SenWeaverCoding_<ver>_x64-setup.exe` (NSIS) | Lets you pick install scope (per-user / per-machine) **and the install directory**. Adds the install dir to `HKCU\Environment\Path` so `sen.exe` is callable from any cmd / PowerShell / Windows Terminal session immediately after install. Removed cleanly on uninstall. |
| **Windows x64** | `SenWeaverCoding_<ver>_x64_en-US.msi` | For silent / group-policy deployment. |
| **macOS (universal)** | `SenWeaverCoding_<ver>_universal.dmg` | One drag-to-Applications package that runs natively on both Apple Silicon and Intel Macs (the bundled `sen` is a `lipo`-merged fat binary). CLI lives at `SenWeaverCoding.app/Contents/Resources/sen`; symlink it once with `ln -sf "/Applications/SenWeaverCoding.app/Contents/Resources/sen" /usr/local/bin/sen` to call it from any shell. |
| **Linux x64** | `SenWeaverCoding_<ver>_amd64.deb` | `sudo dpkg -i` on Debian/Ubuntu — installs the desktop launcher **and** `/usr/bin/sen` automatically. |
| **Linux x64** | `SenWeaverCoding_<ver>_amd64.AppImage` | Portable single-file desktop app. The CLI is **not** carried by the AppImage — install the `.deb` instead if you want a system-wide `sen` command. |

After installation launch **SenWeaverCoding** from the start menu /
Applications / app launcher. On first run open *Settings → Providers*
and add an API key for any provider you want to use. From a terminal
you can verify the CLI with `sen --help`.

---

## Coding modes

Modes are switchable mid-session from the composer. **Auto** is an
intent router that picks a fit per turn; the rest can be pinned:

**Auto** is not a mode itself — it is an intent router that picks one of the
14 pinnable modes below per turn (debug / plan / Q&A / general fit).

| Mode | Writes? | Purpose |
| --- | --- | --- |
| **Agent** *(default)* | yes | Fully autonomous orchestrator. Full tool surface, auto-approved tool calls, decomposes the task, executes end-to-end, then self-verifies. |
| **Vibe** | yes | Full tool access with minimal prompting — fast prototyping and free-form coding when you trust the agent to move quickly. |
| **Spec** | yes | Specification-driven — generates `SPEC.md`, follows a tracked plan step-by-step, and verifies each step with build/test commands. |
| **Plan** | no | Drafts/updates a `.plan.md` under `.senweavercoding/plans/` for later execution. No source edits, no shell. |
| **Ask** | no | Read-only Q&A with citations. No edits, no shell, no plan writes. |
| **TDD** | yes | Strict Red → Green → Refactor: failing test first, minimum implementation, then refactor; auto-verifies after every edit. |
| **Debug** | yes | Reproduce → Hypothesize → Isolate → Fix, with in-app browser QA, PII redaction at the LLM boundary, and report/tech-doc output. |
| **Architect** | yes | Architecture-focused — broad reads for high-level design review, then targeted cross-module edits backed by spec analysis. |
| **Pair** | yes | Collaborative pair-programming — one step at a time, pausing at every checkpoint for your confirmation. |
| **Context** | yes | Context engineering for large codebases — Explore → Map → Plan → Strike, with impact analysis after each batch of edits. |
| **MVAI** | yes | Model-View-Agent-Interface architecture — enforces interface-first contracts that are observable, testable, and clearly layered. |
| **Harness** | yes | Engineering-grade harness — spec generation, skill orchestration, session checkpoints, and multi-agent delegation with auto-verify. |
| **Curator** | docs only | Mines the web + local workspace, authors a professional paper / solution / report with DOCX export, then stops so Agent mode can implement the blueprint. |
| **Designer** | yes | Design studio across ten surfaces (prototype, dashboard, slide deck, diagram, image, video, HyperFrames, audio, from Figma, from template) with a discovery → plan → generate → critique pipeline and a live preview panel. |

Agent mode includes the tools and capabilities of every other mode, so
you never lose functionality by staying in the default.

---

## Build from source

### Prerequisites

* Rust ≥ 1.87 (stable, edition 2024) — install via [rustup](https://rustup.rs).
* [Bun](https://bun.sh) ≥ 1.1 (used for the front-end; npm/pnpm also work).
* Platform-specific Tauri prerequisites:
  * **Windows**: WebView2 runtime + MSVC build tools.
  * **macOS**: Xcode Command Line Tools.
  * **Linux**: `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf`.
  * Full list: <https://v2.tauri.app/start/prerequisites/>.

### Develop

```bash
git clone https://github.com/senweaver/SenWeaverCoding.git
cd SenWeaverCoding/desktop

bun install
bun run tauri dev
```

### Produce installers locally

```bash
cd desktop
bun install
bun run tauri build         # all bundlers configured in tauri.conf.json
```

The artifacts land in `desktop/src-tauri/target/release/bundle/`:

| Bundle | Path |
| --- | --- |
| Windows NSIS / MSI | `bundle/nsis/*.exe`, `bundle/msi/*.msi` |
| macOS dmg / .app | `bundle/dmg/*.dmg`, `bundle/macos/*.app` |
| Linux deb / AppImage | `bundle/deb/*.deb`, `bundle/appimage/*.AppImage` |

### Cutting a release

This repo ships a single GitHub Actions workflow at
`.github/workflows/release.yml`. Pushing a tag matching `v*`
(e.g. `v0.1.0`) — or running the workflow manually with
`gh workflow run release.yml -f tag=v0.1.0` — triggers a clean
multi-platform build:

1. Each platform job does a fresh `actions/checkout@v4`, so no local
   `node_modules/`, `target/`, `.senweavercoding/`, `dist/` or `.vite/`
   cache from your dev machine ever leaks in (`.gitignore` excludes
   them and the workflow reinstalls from scratch).
2. Builds the `sen` CLI into `desktop/src-tauri/binaries/sen[.exe]`.
3. Runs `tauri-action`, which builds the front-end and bundles the
   desktop application + CLI into the platform installers.
4. Uploads every installer to the same GitHub Release.

```bash
git tag v0.1.0
git push origin v0.1.0
```

---

## Feature flags

The crate is feature-gated so headless and desktop builds only compile
what they need.

| Flag group | Examples | Notes |
| --- | --- | --- |
| **default** | `observability-prometheus`, `skill-creation`, `fs-watch`, `sandbox`, `lsp-push-diagnostics`, `tool-image`, `tool-utility-misc`, `tool-search-broad`, `tool-workspace-deep`, `tool-curator`, `lan-comms`, `office-docs` | What a normal build ships. |
| **code-intel** | tree-sitter grammars (rust/js/ts/python/go/java/c/cpp/…) | Enables AST outline + symbol graph; falls back to heuristics when off. |
| **extras** | `tool-cron`, `tool-sop`, `tool-team`, `tool-reports`, `tool-cloud-ops`, `computer-use`, … | Optional tool families (also what desktop enables). |
| **channels** | `channel-slack`, `channel-telegram`, `channel-matrix`, `channel-lark`, … | Multi-channel adapters. |

### Quality gates

There are deliberately **no test files** in this repository — all
verification is performed via `cargo check`, `cargo clippy`, and manual
smoke tests of the desktop app. Benches under `benches/*.rs` are kept
for performance regression tracking. The enforced zero-error checks
(see `AGENTS.md`):

```bash
cargo check --lib
cargo check --lib --no-default-features
cargo check --bin sen
cargo check --bin sen --features crdt-coordination
cargo check --lib --features extras
cd desktop && bunx tsc --noEmit
```

---

## Architecture

```
SenWeaverCoding/
├── src/                     # Rust agent runtime (lib + `sen` bin)
│   ├── agent/               # Turn loop, tool dispatch, modes, context compaction
│   ├── providers/           # OpenAI-compat / Anthropic / OpenRouter / Copilot / …
│   ├── tools/               # 130+ tools (file/shell/git/web/code-intel/…)
│   ├── code_intel/          # tree-sitter outline, symbol graph, heuristic code search
│   ├── context/ · rag/      # Context assembly, retrieval, RAG
│   ├── gateway/             # axum HTTP + WebSocket router (loopback)
│   ├── memory/              # SQLite + Markdown + vector index backends
│   ├── channels/            # Slack / Telegram / Discord / Matrix / Lark / …
│   ├── skills/ · workflows/ # Skills, subagents, multi-step workflows
│   ├── cron/ · hooks/       # Automations, lifecycle hooks, user rules
│   ├── apply_model/         # Patch / multi-edit application
│   ├── tool_descriptions/   # Localized tool manifests (en / zh-CN)
│   ├── security/ · guardrails/  # Sandbox, permissions, PII redaction
│   └── observability/ · evolution/ · lsp/ · …
│
├── desktop/                 # Tauri 2 + React + Vite front-end
│   ├── src/                 # React app (modes, terminal panel, plan/tool cards,
│   │                        #   embedded browser dock, settings, …)
│   ├── src-tauri/           # Tauri shell — embeds `src/` as a Rust library
│   └── package.json
│
├── sdk/                     # First-party TypeScript & Python SDKs
├── benches/                 # Criterion benchmarks
└── .github/workflows/       # CI / release pipeline
```

The desktop app boots the gateway router in-process and connects the
React front-end to it via a `127.0.0.1` WebSocket / HTTP loopback.
Because the back-end is loaded as a library
(`sen_desktop_lib.{cdylib,staticlib}`) there is no IPC serialization
overhead between the UI and the agent runtime. Programmatic access goes
through the JSON-RPC 2.0 server (`sen rpc`, HTTP `/rpc` or stdio/UDS)
consumed by the TypeScript and Python SDKs under `sdk/`.

---

## Roadmap

- Code-signed installers for Windows and macOS notarization.
- Native auto-update channel via Tauri Updater.
- More first-class provider adapters.
- Deeper code-graph driven retrieval across large repositories.
- Cloud-collaborative workspaces.

---

## License

[MIT](./LICENSE) © 2025-2026 SenWeaverCoding
