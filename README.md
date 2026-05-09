# SenWeaverCoding

[English](./README.md) · [简体中文](./README.zh-CN.md)

> **AI Code Editor & Autonomous Agent Runtime · Rust back-end + Tauri/React desktop**

SenWeaverCoding is a desktop AI code editor that ships the entire agent
runtime as a single installable application. The back-end is a Rust
library (`src/`) embedded directly into a Tauri shell; the front-end is
a React + Vite UI living in `desktop/`. There is no sidecar process,
no remote service to talk to, and no separate CLI install — one
installer gives you the full IDE.

---

## Highlights

| Area | What you get |
| --- | --- |
| **Desktop-first** | Tauri 2 shell with native menus, multi-tab sessions, in-app terminal, file browser, embedded browser. |
| **Rust agent runtime** | Same crate that ships in headless deployments, embedded via `crate-type = ["cdylib", "staticlib", "rlib"]`. Zero IPC overhead. |
| **5 coding modes** | Agent, Plan, Ask, Debug, Harness — switch mid-session; each mode rewires system prompt + tool allowlist + auto-verify policy. Agent is the default and exposes the full tool surface. |
| **130+ tools** | File ops, shell with PTY mirroring, Git, ripgrep search, glob/multi-edit, web search/fetch, headless browser, memory store/recall, todo write, image gen, MCPs, Skills, Subagents. |
| **Multi-provider** | OpenAI / Anthropic / DeepSeek / Gemini / Copilot / OpenRouter / any OpenAI-compatible. Provider settings live inside the app. |
| **Persistent memory** | SQLite + Markdown backends with vector embeddings, per-session work-dir isolation, rewind/restore checkpoints. |
| **Performance-tuned** | Streaming Markdown downgrade, rAF batched delta flushes, `content-visibility: auto` virtualization, `tokio::task::spawn_blocking` on every IO hot path. See `AGENTS.md` for the hard constraints. |

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

## Build from source

### Prerequisites

* Rust ≥ 1.87 (stable) — install via [rustup](https://rustup.rs).
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

1. Each platform job does a fresh `actions/checkout@v4` (no local
   `node_modules/`, `target/`, `.senweavercoding/`, `dist/` or
   `.vite/` cache from your dev machine ever leaks in — `.gitignore`
   already excludes them, and the workflow re-installs from scratch).
2. Builds the `sen` CLI into `desktop/src-tauri/binaries/sen[.exe]`.
3. Runs `tauri-action` which builds the front-end and bundles the
   desktop application + CLI into the platform installers.
4. Uploads every installer to the same GitHub Release.

```bash
git tag v0.1.0
git push origin v0.1.0
```

After a few minutes the Release page lists installers for Windows /
macOS (one universal `.dmg`) / Linux (deb + AppImage).

### Quality gates

The repo enforces two zero-error checks (see `AGENTS.md`):

```bash
cargo check --lib
cargo check --lib --no-default-features
cd desktop && bunx tsc --noEmit
```

There are deliberately **no test files** in this repository — all
verification is performed via `cargo check`, `cargo clippy`, and manual
smoke tests of the desktop app. Benches under `benches/*.rs` are kept
for performance regression tracking.

---

## Architecture

```
SenWeaverCoding/
├── src/                     # Rust agent runtime (lib + `sen` bin)
│   ├── agent/               # Turn loop, tool dispatch, compression
│   ├── providers/           # OpenAI / Anthropic / DeepSeek / ...
│   ├── tools/               # 130+ tools (file/shell/git/web/...)
│   ├── gateway/             # axum HTTP + WebSocket router
│   ├── channels/            # Slack / Telegram / Discord / ...
│   ├── memory/              # SQLite + vector index backends
│   ├── context/             # Symbol graph, outline, RAG
│   ├── apply_model/         # Patch / multi-edit application
│   └── ...
│
├── desktop/                 # Tauri 2 + React + Vite front-end
│   ├── src/                 # React app (5 modes, terminal panel,
│   │                        #   plan card, tool result blocks, …)
│   ├── src-tauri/           # Tauri shell — embeds `src/` as a Rust
│   │                        #   library, no sidecar process
│   └── package.json
│
├── crates/                  # Workspace sub-crates (sen-core / sen-cli /
│                            #   sen-tui / sen-channels)
├── tool_descriptions/       # Machine-readable tool manifests
├── benches/                 # Criterion benchmarks
└── .github/workflows/       # CI / release pipelines
```

The desktop app boots the gateway router in-process and connects the
React front-end to it via `127.0.0.1` WebSocket / HTTP. Because the
back-end is loaded as a library (`sen_desktop_lib.{cdylib,staticlib}`)
there is zero serialization or IPC overhead between the UI and the
agent runtime.

---

## Roadmap

- Code-signed installers for Windows and macOS notarization.
- Native auto-update channel via Tauri Updater.
- More provider adapters (Mistral, Qwen, GLM …).
- VS Code / JetBrains companion extensions for hand-off into the desktop app.
- Cloud-collaborative workspaces.

---

## License

[MIT](./LICENSE) © 2025-2026 senweaver
