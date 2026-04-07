# SenWeaverCoding

[English](README.md) | [简体中文](README.zh-CN.md)

<p align="center">
  <!-- logo placeholder -->
  <img src="docs/images/logo.svg" width="200" alt="SenWeaverCoding Logo" />
</p>

<p align="center">
  <strong>Autonomous AI Agent Runtime & CLI Code Editor · Built in Rust</strong>
</p>

<p align="center">
  <a href="https://github.com/senweaver/SenWeaverCoding/actions/workflows/ci.yml">
    <img src="https://github.com/senweaver/SenWeaverCoding/actions/workflows/ci.yml/badge.svg" alt="CI Status" />
  </a>
  <a href="https://crates.io/crates/senweavercoding">
    <img src="https://img.shields.io/crates/v/senweavercoding" alt="Crate Version" />
  </a>
  <a href="https://crates.io/crates/senweavercoding">
    <img src="https://img.shields.io/crates/d/senweavercoding" alt="Crate Downloads" />
  </a>
  <a href="https://github.com/senweaver/SenWeaverCoding/blob/master/LICENSE">
    <img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License" />
  </a>
</p>

---

## What is SenWeaverCoding?

**SenWeaverCoding (`sen`)** is a Rust-first autonomous AI agent runtime and CLI code editor, engineered for professional developers who demand performance, stability, security, and extensibility.

sen is built on top of **SenAgentOS** — a Rust-native agent operating system — and extends it specifically for code engineering workflows. It brings agentic capabilities into the development loop: autonomous code exploration, generation, refactoring, testing, and debugging — all driven by LLMs and orchestrated through a pluggable tool system.

> If you know Claude Code, Cursor CLI, or Aider, sen is the next-generation, Rust-powered alternative with deeper architecture and more engineering discipline built in.

---

## Key Features

| Feature | Description |
|---------|-------------|
| **Rust-first** | Binary distribution, zero runtime, minimal memory footprint |
| **12 Switchable Modes** | From free-form exploration (Vibe) to engineering-grade workflows (Harness) — see [docs/coding-modes.md](docs/coding-modes.md) |
| **130+ Tools** | File ops, Git, Shell, search, web, browser, memory, and more |
| **Modular Architecture** | Extend via trait system: Provider, Channel, Tool, Memory, Observer, RuntimeAdapter |
| **Persistent Memory** | Markdown + SQLite backends with vector embeddings for cross-session knowledge |
| **Self-updating** | One command to upgrade to the latest version |

---

## Installation

**Linux / macOS:**

```bash
curl -fsSL https://raw.githubusercontent.com/senweaver/SenWeaverCoding/master/install.sh | bash
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/senweaver/SenWeaverCoding/master/install.ps1 | iex
```

**Other methods** (Homebrew, Scoop, Deb/RPM, from source) are documented in [docs/setup-guides/installation.md](docs/setup-guides/installation.md).

---

## Quick Start

```bash
# First-time setup
sen onboard --quick --api-key sk-xxx --provider openrouter

# Interactive REPL
sen 

# Single-shot query
sen -m "Explain the architecture of this project"

# Pipe from stdin
cat error.log | sen -m -
```

---

## Twelve Coding Modes

sen ships with 12 configurable coding modes, each with a distinct behavioral profile: system prompt injection, tool allowlists, approval policies, and auto-verify behavior. Switch modes mid-session with `/m <name>`.

Full mode reference: [docs/coding-modes.md](docs/coding-modes.md)

```
sen > /m tdd      # Test-driven development
sen > /m debug    # Systematic debugging protocol
sen > /m harness  # Engineering-grade workflow
```

---

## Tools

130+ tools available out of the box:

| Tool | Description |
|------|-------------|
| `shell` | Execute commands with streaming, timeout, and output cap |
| `file_read` / `file_write` / `file_edit` | File I/O and precise search-and-replace |
| `multi_edit` | Atomic multi-file editing with rollback |
| `glob_search` / `content_search` | Glob and Ripgrep-powered search |
| `git_operations` | Git add / commit / diff / log / branch |
| `diagnostics` | Run cargo / tsc / go vet |
| `web_search` / `web_fetch` | Web search and content fetching |
| `browser` | Headless browser automation |
| `memory_store` / `memory_recall` | Persistent memory read/write |
| `todo_write` | Task list management |
| `image_gen` | Image generation |
| `http_request` | HTTP requests |

---

## Configuration

Config file: `~/.senweavercoding/config.toml` (or `$SEN_CONFIG`)

```bash
export SEN_API_KEY="sk-..."
export SEN_PROVIDER="openrouter"
export SEN_MODEL="anthropic/claude-sonnet-4-20250514"
export SEN_THEME="concise"   # concise | code-only | formal
```

View the full config schema:

```bash
sen config schema | jq .
```

---

## Build from Source

```bash
git clone https://github.com/senweaver/SenWeaverCoding.git
cd SenWeaverCoding

cargo build --release
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
./dev/ci.sh all
```

With GUI / TUI:

```bash
cargo build --release --features gui   # egui desktop app
cargo build --release --features tui    # terminal dashboard
```

---

## Architecture

sen uses a trait-driven modular architecture. Core extension points:

```
src/providers/traits.rs    →  Provider (model providers)
src/channels/traits.rs     →  Channel (Telegram/Discord/Slack etc.)
src/tools/traits.rs         →  Tool (tool extensions)
src/memory/traits.rs        →  Memory (memory backends)
src/observability/traits.rs →  Observer (observability)
src/runtime/traits.rs       →  RuntimeAdapter (runtime adapters)
```

To add a new Provider, Channel, or Tool: implement the trait and register it in the factory module. No core code changes required.

---

## Roadmap

- [ ] Documentation and internationalization (i18n)
- [ ] MCP (Model Context Protocol) server support
- [ ] Web UI / cloud collaboration
- [ ] VS Code / JetBrains IDE plugins
- [ ] More model providers (Claude, Google AI, etc.)
- [ ] Deeper integration with SenAgentOS subsystems

---

## Contributing

Read the guides in [docs/contributing/](docs/contributing/):

- [Change Playbooks](docs/contributing/change-playbooks.md) — how to add providers/channels/tools
- [PR Discipline](docs/contributing/pr-discipline.md) — commit conventions, privacy rules
- [Docs Contract](docs/contributing/docs-contract.md) — doc system and i18n rules

Security vulnerabilities should be reported privately — do not discuss them in public issues.

---

## License

MIT License. See [LICENSE](LICENSE).

---

## Community

- 📖 [Documentation](docs/)
- 🐛 [Issue Tracker](https://github.com/senweaver/SenWeaverCoding/issues)
- 💬 [Discussions](https://github.com/senweaver/SenWeaverCoding/discussions)

If sen is useful to you, please give it a ⭐!