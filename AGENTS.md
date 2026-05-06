# AGENTS.md — 本仓库对 AI 协作代理的硬性约定

## 测试代码（硬约束）

本仓库**不含任何测试代码**，且**永久禁止**引入测试代码。具体约束如下：

- **绝对禁止**新增以下任何内容：
  - `tests/*.rs` 集成测试文件（`cargo test --test <name>` 场景）。
  - 源码中的 `#[cfg(test)] mod tests { ... }` 块、`#[test]` 函数、`#[tokio::test]` 函数。
  - `test-utils` feature 或 `install_test_runtime` 等仅用于测试的辅助。
  - 任何测试专用的 helper、mock、fixture 代码。
- `benches/*.rs` **性能基准**文件保留，不受上述约束，`criterion` dev-dependency 也为此保留。
- **验收标准只有两条**，缺一不可：
  1. `cargo check --lib` 通过，零 error。
  2. `cargo check --lib --no-default-features` 通过，零 error。
- 若改动确实需要验证，使用 `cargo check` / `cargo clippy` / 人工 smoke 即可；**不要**以任何理由在项目中写测试。
- 所有新提交代码必须遵守此约束，无例外。

## 模型协议：OpenAI 与 Anthropic 双格式兼容（硬约束）

后续**所有**涉及「多轮对话、持久化 transcript、hydration、压缩/裁剪、工具调用往返、网关与 Agent 之间传消息」的功能与修复，**必须同时兼容**下列两类模型侧协议，不得只做其中一种：

- **OpenAI / OpenAI 兼容 Chat Completions**：`role` 为 `system` | `user` | `assistant` | `tool`；`assistant` 使用 `tool_calls`（**禁止**发出空的 `tool_calls` 数组）；`tool` 必须能合法挂接在带 `tool_calls` 的 `assistant` 之后（或按该 API 允许的链式规则）；`reasoning_content` 等扩展字段在需要回放的模型上必须可 round-trip。
- **Anthropic Messages 形态**（本仓库通过转换或块结构接触时）：`user` / `assistant` 内容块中的 `tool_use` 与 `tool_result` **成对**、顺序与 ID 一致；恢复会话、合并消息、截断历史时不得留下「孤块」导致上游 400。

实现上要求：新增或修改相关逻辑时，必须在**统一归一化/修复层**（例如发送 API 前、写入 session 前）保证两种形态在各自路径下都合法；**禁止**为某一供应商写死一种 wire 形状却破坏另一种；若只能二选一，必须先扩展抽象或分支，而不是牺牲未覆盖的一种。

## 其它

- 修复和优化必须是真实实现，不使用占位代码或 `todo!()` 逃逸。
- 所有新增或者优化以及修复的功能和模块都必须真实的接入我们的项目，不能出现断连。
- 改代码前先读文件；不要凭记忆或搜索片段就下手改。
- 文档（`docs/**`、`*.md`）仅在用户明确要求时才新增；不要为了"更完整"自造文档。
- 我们项目的核心是AI编程所以所有修改和优化都选择最优最彻底最完整的方式
- 文件名命名不要使用阶段命名，而是直接使用功能命名
- 代码注释中不要把阶段名添加进去例如A1.1、B2.1、M1.1、phase0等
- 最后总结的时候请使用中文回复我，如果需要提问那么也请使用中文
- 不要在代码中添加代码注释和说明，每个文件和代码块除了添加版权的注释以外不要添加其他任何的注释信息和说明
- 不要提交仓库代码，我自己手动提交git代码