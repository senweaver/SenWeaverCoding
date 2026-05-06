# SenWeaverCoding

[English](./README.md) · [简体中文](./README.zh-CN.md)

> **AI 代码编辑器与自治 Agent 运行时 · Rust 后端 + Tauri/React 桌面前端**

SenWeaverCoding 是一款桌面端 AI 代码编辑器：整套 Agent 运行时被打包为
**单一安装包**。后端是 `src/` 下的 Rust 库，被直接以 cdylib/staticlib
形式嵌入到 Tauri 外壳；前端是 `desktop/` 下的 React + Vite 应用。**没有
旁挂进程、没有外部服务依赖、也无需单独安装 CLI** —— 一个安装器就是完整
IDE。

---

## 主要特性

| 维度 | 实际能力 |
| --- | --- |
| **桌面优先** | Tauri 2 外壳，原生菜单 / 多 Tab 会话 / 内嵌终端 / 文件浏览 / 嵌入式浏览器面板。 |
| **Rust Agent 运行时** | 与无头部署完全相同的 crate，通过 `crate-type = ["cdylib", "staticlib", "rlib"]` 直接嵌入，零 IPC 开销。 |
| **12 种 Coding Mode** | Plan / Build / Debug / TDD / Spec / Vibe / Architect / Pair / Ask / ContextEng / MVAI / Harness，会话中可随时切换；每种模式重写系统提示、工具白名单、自动验证策略。 |
| **130+ 工具** | 文件操作、PTY 镜像 Shell、Git、ripgrep 检索、glob/multi-edit、Web 搜索/抓取、无头浏览器、记忆存取、Todo 写入、图像生成、MCP、Skill、Subagent。 |
| **多 Provider** | OpenAI / Anthropic / DeepSeek / Gemini / Copilot / OpenRouter / 任意 OpenAI 兼容端点；Provider 设置完全在应用内完成。 |
| **持久化记忆** | SQLite + Markdown 双后端 + 向量索引，会话级工作目录隔离，rewind/restore 检查点。 |
| **性能调优** | 流式 Markdown 降级、rAF 批量增量 flush、`content-visibility: auto` 离屏跳过、所有 IO 热点 `tokio::task::spawn_blocking` 落到阻塞池。详见 `AGENTS.md`。 |

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

## 从源码构建

### 准备环境

* Rust ≥ 1.87 stable —— 通过 [rustup](https://rustup.rs) 安装。
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
   等任何缓存（`.gitignore` 已经把这些目录全部排除，runner 上重新
   `bun install` + `cargo build` 从零生成）。
2. 先编 `sen` CLI 二进制并放入 `desktop/src-tauri/binaries/sen[.exe]`。
3. 执行 `tauri-action` 构建前端并把桌面应用 + CLI 一起打成各平台安装包。
4. 把所有安装包附加到同一个 GitHub Release。

```bash
git tag v0.1.0
git push origin v0.1.0
```

几分钟后 Releases 页面就会出现 Windows / macOS（universal `.dmg`）
/ Linux（deb + AppImage）的安装包。

### 质量门禁

仓库强制两条零 error 校验（见 `AGENTS.md`）：

```bash
cargo check --lib
cargo check --lib --no-default-features
cd desktop && bunx tsc --noEmit
```

仓库**不包含任何测试代码**，所有验证依赖 `cargo check` / `cargo clippy`
/ 桌面应用人工 smoke。`benches/*.rs` 下保留 Criterion 基准用于性能回归
监测。

---

## 架构

```
SenWeaverCoding/
├── src/                     # Rust Agent 运行时（lib + `sen` 二进制）
│   ├── agent/               # Turn 循环、工具分发、上下文压缩
│   ├── providers/           # OpenAI / Anthropic / DeepSeek / ...
│   ├── tools/               # 130+ 工具（文件/Shell/Git/Web/...）
│   ├── gateway/             # axum HTTP + WebSocket 路由
│   ├── channels/            # Slack / Telegram / Discord / ...
│   ├── memory/              # SQLite + 向量索引后端
│   ├── context/             # 符号图、Outline、RAG
│   ├── apply_model/         # Patch / multi-edit 应用器
│   └── ...
│
├── desktop/                 # Tauri 2 + React + Vite 前端
│   ├── src/                 # React 应用（12 种模式、终端面板、
│   │                        #   PlanCard、ToolResultBlock 等）
│   ├── src-tauri/           # Tauri 外壳 —— 把 `src/` 作为 Rust 库
│   │                        #   直接嵌入，没有旁挂进程
│   └── package.json
│
├── crates/                  # Workspace 子 crate（sen-core / sen-cli /
│                            #   sen-tui / sen-channels）
├── tool_descriptions/       # 工具机读清单
├── benches/                 # Criterion 基准
└── .github/workflows/       # CI / Release 流水线
```

桌面应用在进程内启动 gateway 路由，React 前端通过 `127.0.0.1` 上的
WebSocket / HTTP 与之对话。因为后端以库（`sen_desktop_lib.{cdylib,
staticlib}`）形式加载，UI 与 Agent 运行时之间**无序列化、无 IPC 开销**。

---

## Roadmap

- Windows 代码签名与 macOS 公证。
- 通过 Tauri Updater 提供原生自动升级通道。
- 更多 Provider 适配（Mistral / Qwen / GLM…）。
- VS Code / JetBrains 伴侣插件，把上下文交接到桌面应用。
- 云端协作工作区。

---

## License

[MIT](./LICENSE) © 2025-2026 senweaver
