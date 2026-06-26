# SenWeaverCoding

[English](./README.md) · [简体中文](./README.zh-CN.md)

> **AI 代码编辑器与自治 Agent 运行时 · Rust 后端 + Tauri/React 桌面前端**

SenWeaverCoding 是一款桌面端 AI 代码编辑器：整套 Agent 运行时被打包为
**单一安装包**。后端是 `src/` 下的 Rust 库，以 cdylib/staticlib 形式直接
嵌入 Tauri 2 外壳；前端是 `desktop/` 下的 React + Vite 应用。**没有旁挂
进程、没有外部服务依赖、也无需单独安装 CLI** —— 一个安装器即同时给你
完整 IDE 和可在终端直接调用的 `sen` 命令。

---

## 主要特性

| 维度 | 实际能力 |
| --- | --- |
| **桌面优先** | Tauri 2 外壳，原生菜单 / 多 Tab 会话 / 内嵌 PTY 终端 / 文件浏览 / 用于实时 Web·UI 调试的嵌入式浏览器面板。 |
| **进程内 Rust 运行时** | 与无头部署完全相同的 crate，通过 `crate-type = ["cdylib", "staticlib", "rlib"]` 进程内加载；UI 通过本地回环 WebSocket/HTTP 网关与之通信 —— 无重量级 IPC 序列化、无外部守护进程。 |
| **意图路由的 Coding Mode** | `Auto` 按每轮意图自动选择最合适的模式；也可显式锁定：**Agent**（默认，完全自治 + 完整工具集）、**Plan**（只读的计划撰写）、**Ask**（带引用的只读问答）、**Debug**（四阶段根因分析 + 应用内浏览器 QA）、**Curator**（研究 → DOCX/实现蓝图）、**Designer**（十种 UI/设计画布）。每种模式都会重写系统提示、工具白名单、审批策略与自动验证行为。 |
| **130+ 工具** | 文件操作、PTY 镜像 Shell、Git、ripgrep/内容检索、glob/multi-edit/patch-apply、**代码智能**（tree-sitter 大纲 + 符号图 callers/implementors/uses + 可选 Tantivy 全文索引）、多引擎 Web 搜索/获取、无头与嵌入式浏览器、SQLite + 向量记忆、Todo/计划跟踪、图像生成、Office 文档（xlsx/pdf/docx）、MCP、Skill、Subagent 委派。 |
| **多 Provider** | OpenAI 兼容（含 DeepSeek / Gemini 兼容端点）、Anthropic、OpenRouter、GitHub Copilot、Claude Code、Ollama、Azure OpenAI、AWS Bedrock、Telnyx，以及本地 CLI 桥接。Provider 密钥与会话级模型路由均在应用内配置。 |
| **持久化记忆与检查点** | SQLite + Markdown 双后端 + 向量索引，会话级工作目录隔离，rewind/restore 检查点；中断的任务会被修复，输入"继续"时续跑的是**最近一次**被中断的任务。 |
| **自动化与可扩展** | Cron 定时自动化、Hooks、用户 Rules、Skills、MCP 服务器、多通道适配（Slack / Telegram / Discord / Matrix / Lark / …），以及基于 `/v1/agents` REST 接口的官方 TypeScript & Python SDK。 |
| **性能调优** | 虚拟化消息列表、rAF 合帧的流式 flush、优先级 WebSocket 心跳、内容感知的上下文压缩、IO 热点 `spawn_blocking` 落阻塞池。硬性约束见 `AGENTS.md`。 |

---

## 安装

预编译安装包发布在
[Releases](https://github.com/senweaver/SenWeaverCoding/releases)
页面。**每个安装包都同时附带桌面应用与 `sen` 命令行**，安装一次即可
同时获得 GUI 和可在终端直接调用的 `sen`。

| 平台 | 安装包 | 安装后获得的内容 |
| --- | --- | --- |
| **Windows x64** | `SenWeaverCoding_<ver>_x64-setup.exe`（NSIS） | 安装时**可选择安装范围（当前用户 / 本机所有用户）并自定义安装目录**；安装目录会被自动写入 `HKCU\Environment\Path`，cmd / PowerShell / Windows Terminal 立即可用 `sen.exe`，卸载时自动移除。 |
| **Windows x64** | `SenWeaverCoding_<ver>_x64_en-US.msi` | 适合域内静默部署 / 组策略推送。 |
| **macOS（通用）** | `SenWeaverCoding_<ver>_universal.dmg` | 一个拖入 Applications 的安装包，即可在 Apple Silicon 与 Intel Mac 上原生运行（内置 `sen` 是 `lipo` 合并后的胖二进制）。CLI 位于 `SenWeaverCoding.app/Contents/Resources/sen`，可执行一次 `ln -sf "/Applications/SenWeaverCoding.app/Contents/Resources/sen" /usr/local/bin/sen` 让 `sen` 全局可用。 |
| **Linux x64** | `SenWeaverCoding_<ver>_amd64.deb` | Debian/Ubuntu 系：`sudo dpkg -i` —— 同时安装桌面快捷方式与 `/usr/bin/sen`。 |
| **Linux x64** | `SenWeaverCoding_<ver>_amd64.AppImage` | 单文件便携桌面版。AppImage 内**不含独立 CLI**，如需系统级 `sen` 命令请改用 `.deb`。 |

安装后从开始菜单 / Applications / 应用启动器打开
**SenWeaverCoding** 即可。首次启动请进入 *设置 → Providers* 添加任意
LLM 提供商的 API Key。终端中执行 `sen --help` 可验证 CLI 安装。

---

## Coding Mode 说明

模式可在输入框处会话中随时切换。**Auto** 是意图路由，按每轮自动选择；
其余模式可显式锁定：

| 模式 | 是否写入 | 用途 |
| --- | --- | --- |
| **Auto** | 视情况 | 按消息意图（调试 / 计划 / 问答 / 通用）路由到最合适的模式。 |
| **Agent**（默认） | 是 | 完全自治的编排器。完整工具集、自动审批工具调用，拆解任务、端到端执行并自我验证。 |
| **Plan** | 否 | 在 `.senweavercoding/plans/` 下撰写/更新 `.plan.md` 供后续执行；不改源码、不跑 Shell。 |
| **Ask** | 否 | 带引用的只读问答；不改文件、不跑 Shell、不写计划。 |
| **Debug** | 是 | 复现 → 假设 → 隔离 → 修复，配合应用内浏览器 QA、LLM 边界 PII 脱敏，输出报告/技术文档。 |
| **Curator** | 仅文档 | 深挖 Web 与本地工作区，撰写专业论文/方案/技术报告并导出 DOCX，完成后停下，交由 Agent 模式实现蓝图。 |
| **Designer** | 是 | 设计工作室，覆盖十种画布（原型、仪表盘、幻灯片、图表、图像、视频等），走探索 → 规划 → 生成 → 评审流水线并实时预览。 |

Agent 模式具备其它所有模式的工具与能力，因此停留在默认模式不会损失任何功能。

---

## 从源码构建

### 准备环境

* Rust ≥ 1.87 stable（edition 2024）—— 通过 [rustup](https://rustup.rs) 安装。
* [Bun](https://bun.sh) ≥ 1.1（用于前端；npm/pnpm 亦可）。
* Tauri 平台依赖：
  * **Windows**：WebView2 运行时 + MSVC 构建工具链。
  * **macOS**：Xcode Command Line Tools。
  * **Linux**：`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf`。
  * 完整清单：<https://v2.tauri.app/start/prerequisites/>。

### 开发模式

```bash
git clone https://github.com/senweaver/SenWeaverCoding.git
cd SenWeaverCoding/desktop

bun install
bun run tauri dev
```

### 本地打包安装器

```bash
cd desktop
bun install
bun run tauri build         # 按 tauri.conf.json 输出全部目标平台安装包
```

打包产物位于 `desktop/src-tauri/target/release/bundle/`：

| 形态 | 路径 |
| --- | --- |
| Windows NSIS / MSI | `bundle/nsis/*.exe`、`bundle/msi/*.msi` |
| macOS dmg / .app | `bundle/dmg/*.dmg`、`bundle/macos/*.app` |
| Linux deb / AppImage | `bundle/deb/*.deb`、`bundle/appimage/*.AppImage` |

### 发版（自动多平台 release）

仓库内 `.github/workflows/release.yml` 是唯一一份发版流水线。**只要
推送一个 `v*` 形式的 tag**（例如 `v0.1.0`），或在 GitHub 网页上手动
`Run workflow` 输入 tag，即可自动触发所有平台的干净构建：

1. 每个平台 job 都跑一次全新 `actions/checkout@v4`，不会复用你本地的
   `node_modules/`、`target/`、`.senweavercoding/`、`dist/`、`.vite/`
   等任何缓存（`.gitignore` 已排除，runner 上从零 `bun install` +
   `cargo build`）。
2. 先编 `sen` CLI 二进制并放入 `desktop/src-tauri/binaries/sen[.exe]`。
3. 执行 `tauri-action` 构建前端并把桌面应用 + CLI 一起打成各平台安装包。
4. 把所有安装包附加到同一个 GitHub Release。

```bash
git tag v0.1.0
git push origin v0.1.0
```

---

## Feature 开关

crate 全面按 feature 门控，无头与桌面构建只编译各自需要的部分。

| 开关组 | 示例 | 说明 |
| --- | --- | --- |
| **default** | `observability-prometheus`、`skill-creation`、`fs-watch`、`sandbox`、`lsp-push-diagnostics`、`tool-image`、`tool-utility-misc`、`tool-search-broad`、`tool-workspace-deep`、`tool-curator`、`lan-comms`、`office-docs` | 常规构建默认启用项。 |
| **code-intel** | tree-sitter 语法（rust/js/ts/python/go/java/c/cpp/…） | 启用 AST 大纲 + 符号图；关闭时回退启发式。 |
| **code-search** | `tantivy` | 增量全文代码索引。 |
| **extras** | `tool-cron`、`tool-sop`、`tool-team`、`tool-reports`、`tool-cloud-ops`、`computer-use`… | 可选工具族（桌面端也启用这些）。 |
| **channels** | `channel-slack`、`channel-telegram`、`channel-matrix`、`channel-lark`… | 多通道适配。 |

### 质量门禁

仓库**不包含任何测试代码**，所有验证依赖 `cargo check` / `cargo clippy`
与桌面应用人工 smoke；`benches/*.rs` 下保留 Criterion 基准用于性能回归
监测。强制的零 error 校验（见 `AGENTS.md`）：

```bash
cargo check --lib
cargo check --lib --no-default-features
cargo check --bin sen
cargo check --bin sen --features crdt-coordination
cargo check --lib --features extras
cd desktop && bunx tsc --noEmit
```

---

## 架构

```
SenWeaverCoding/
├── src/                     # Rust Agent 运行时（lib + `sen` 二进制）
│   ├── agent/               # Turn 循环、工具分发、模式、上下文压缩
│   ├── providers/           # OpenAI 兼容 / Anthropic / OpenRouter / Copilot / …
│   ├── tools/               # 130+ 工具（文件/Shell/Git/Web/代码智能/…）
│   ├── code_intel/          # tree-sitter 大纲、符号图、Tantivy 检索
│   ├── context/ · rag/      # 上下文组装、检索、RAG
│   ├── gateway/             # axum HTTP + WebSocket 路由（本地回环）
│   ├── memory/              # SQLite + Markdown + 向量索引后端
│   ├── channels/            # Slack / Telegram / Discord / Matrix / Lark / …
│   ├── skills/ · workflows/ # Skill、Subagent、多步工作流
│   ├── cron/ · hooks/       # 自动化、生命周期 Hook、用户 Rules
│   ├── apply_model/         # Patch / multi-edit 应用器
│   ├── security/ · guardrails/  # 沙箱、权限、PII 脱敏
│   └── observability/ · evolution/ · lsp/ · …
│
├── desktop/                 # Tauri 2 + React + Vite 前端
│   ├── src/                 # React 应用（模式、终端面板、Plan/Tool 卡片、
│   │                        #   嵌入式浏览器面板、设置 …）
│   ├── src-tauri/           # Tauri 外壳 —— 把 `src/` 作为 Rust 库直接嵌入
│   └── package.json
│
├── sdk/                     # 官方 TypeScript & Python SDK
├── tool_descriptions/       # 本地化工具清单（en / zh-CN）
├── benches/                 # Criterion 基准
└── .github/workflows/       # CI / Release 流水线
```

桌面应用在进程内启动 gateway 路由，React 前端通过 `127.0.0.1` 的
WebSocket / HTTP 回环与之对话。因为后端以库（`sen_desktop_lib.{cdylib,
staticlib}`）形式加载，UI 与 Agent 运行时之间**无 IPC 序列化开销**。
同一网关还对外暴露 `/v1/agents` REST 接口，供 `sdk/` 下的 TypeScript 与
Python SDK 调用。

---

## Roadmap

- Windows 代码签名与 macOS 公证。
- 通过 Tauri Updater 提供原生自动升级通道。
- 更多一方 Provider 适配。
- 面向大型仓库、由代码图谱驱动的更深检索。
- 云端协作工作区。

---

## License

[MIT](./LICENSE) © 2025-2026 SenWeaverCoding
