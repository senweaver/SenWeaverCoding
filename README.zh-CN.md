# SenWeaverCoding

[English](README.md) | [简体中文](README.zh-CN.md)


<p align="center">
  <strong>自主 AI Agent 运行时与 CLI 代码编辑器 · Rust 原生构建</strong>
</p>

<p align="center">  
  <a href="https://github.com/senweaver/SenWeaverCoding/blob/master/LICENSE">
    <img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License" />
  </a>
</p>

---

## 什么是 SenWeaverCoding？

**SenWeaverCoding（`sen`）** 是一个基于 **SenAgentOS** 构建的 Rust 原生自主 AI Agent 运行时与 CLI 代码编辑器，专为追求性能、稳定性、安全性和可扩展性的专业开发者设计。

sen 在 SenAgentOS（Rust 原生 Agent 操作系统）的基础上，针对代码工程工作流进行了深度定制和优化。它将 Agent 能力引入开发闭环：自主代码探索、生成、重构、测试与调试 —— 全部由 LLM 驱动，通过可插拔工具系统进行编排。

> 如果你熟悉 Claude Code、Cursor CLI 或 Aider，sen 是下一代 Rust 驱动、更深架构、更强工程纪律的替代选择。

---

## 核心特性

| 特性 | 说明 |
|------|------|
| **Rust 原生** | 二进制分发，无需运行时，极低内存占用 |
| **十二种编程模式** | 从自由探索（Vibe）到工程级工作流（Harness），详见 [docs/coding-modes.md](docs/coding-modes.md) |
| **130+ 工具** | 文件操作、Git、Shell、搜索、Web、浏览器、记忆等开箱即用 |
| **模组化架构** | 通过 trait 系统扩展：Provider、Channel、Tool、Memory、Observer、RuntimeAdapter |
| **持久记忆** | Markdown + SQLite 双后端 + 向量嵌入，跨会话积累知识 |
| **自更新** | 一条命令升级到最新版本 |

---

## 安装

**Linux / macOS：**

```bash
curl -fsSL https://raw.githubusercontent.com/senweaver/SenWeaverCoding/master/install.sh | bash
```

**Windows（PowerShell）：**

```powershell
irm https://raw.githubusercontent.com/senweaver/SenWeaverCoding/master/install.ps1 | iex
```

**其他安装方式**（Homebrew、Scoop、Deb/RPM、从源码编译）请参阅 [docs/setup-guides/installation.md](docs/setup-guides/installation.md)。

---

## 快速开始

```bash
# 首次配置
sen onboard --quick --api-key sk-xxx --provider openrouter

# 交互式 REPL
sen

# 单次查询
sen -m "解释这个项目的架构"

# 从 stdin 传入
cat error.log | sen -m -
```

---

## 十二种编程模式

sen 内置 12 种可配置的编程模式，每种模式对应不同的行为配置：系统提示词注入、工具白名单、审批策略和自动验证行为。可在会话中随时使用 `/m <name>` 切换。

完整模式说明：[docs/coding-modes.md](docs/coding-modes.md)（[中文版](docs/coding-modes.zh-CN.md)）

```
sen > /m tdd      # 测试驱动开发
sen > /m debug    # 系统化调试协议
sen > /m harness  # 工程级工作流
```

---

## 工具一览

开箱即用 130+ 工具，覆盖日常开发全流程：

| 工具 | 描述 |
|------|------|
| `shell` | 执行 Shell 命令（流式输出、超时控制、输出上限） |
| `file_read` / `file_write` / `file_edit` | 文件读写与精确搜索替换 |
| `multi_edit` | 原子性多文件编辑（失败回滚） |
| `glob_search` / `content_search` | Glob 和 Ripgrep 搜索 |
| `git_operations` | Git add / commit / diff / log / branch |
| `diagnostics` | 运行 cargo / tsc / go vet |
| `web_search` / `web_fetch` | 网页搜索与内容抓取 |
| `browser` | 无头浏览器自动化 |
| `memory_store` / `memory_recall` | 持久记忆存取 |
| `todo_write` | 任务列表管理 |
| `image_gen` | 图像生成 |
| `http_request` | HTTP 请求 |

---

## 配置

配置文件位于 `~/.senweavercoding/config.toml`（或 `~/.config/senweavercoding/config.toml`），也可通过环境变量覆盖：

```bash
export SEN_API_KEY="sk-xxx"
export SEN_PROVIDER="openrouter"
export SEN_MODEL="anthropic/claude-sonnet-4-20250514"
export SEN_THEME="concise"   # concise | code-only | formal
```

查看完整配置 schema：

```bash
sen config schema | jq .
```

---

## 从源码构建

```bash
git clone https://github.com/senweaver/SenWeaverCoding.git
cd SenWeaverCoding

cargo build --release
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
./dev/ci.sh all
```

构建含 GUI / TUI 的版本：

```bash
cargo build --release --features gui   # egui 桌面应用
cargo build --release --features tui  # 终端 TUI 仪表板
```

---

## 架构

sen 采用 trait 驱动的模组化架构，核心扩展点：

```
src/providers/traits.rs    →  Provider（模型供应商）
src/channels/traits.rs     →  Channel（Telegram/Discord/Slack 等）
src/tools/traits.rs         →  Tool（工具扩展）
src/memory/traits.rs        →  Memory（记忆后端）
src/observability/traits.rs →  Observer（可观测性）
src/runtime/traits.rs       →  RuntimeAdapter（运行时适配）
```

新增 Provider、Channel 或 Tool：只需实现对应 trait 并注册到工厂模块，无需修改核心代码。

---

## 路线图

- [ ] 文档完善与国际化（i18n）
- [ ] MCP（Model Context Protocol）服务器支持
- [ ] Web UI / 云端协作
- [ ] VS Code / JetBrains IDE 插件
- [ ] 更多模型 Provider（Claude、Google AI 等）
- [ ] 与 SenAgentOS 子系统深度集成

---

## 贡献指南

请阅读 [docs/contributing/](docs/contributing/) 下的指南：

- [贡献流程](docs/contributing/change-playbooks.md) — 如何添加 Provider/Channel/Tool
- [PR 规范](docs/contributing/pr-discipline.md) — 提交规范、隐私规则
- [文档系统](docs/contributing/docs-contract.md) — 文档结构与国际化

安全漏洞请通过私密渠道报告，不要在公开 Issue 中提及。

---

## 许可证

本项目基于 **MIT 许可证** 开源，详见 [LICENSE](LICENSE) 文件。

---

## 社区与支持

- 📖 [文档](docs/)
- 🐛 [Issue Tracker](https://github.com/senweaver/SenWeaverCoding/issues)
- 💬 [Discussions](https://github.com/senweaver/SenWeaverCoding/discussions)

如果觉得 sen 有用，欢迎给项目点个 ⭐！