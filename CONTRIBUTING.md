# Contributing to SenWeaverCoding

感谢您对 SenWeaverCoding 的贡献！以下是参与项目开发的一些指南。

## 开发环境设置

1. 安装 Rust 工具链 (建议使用 rustup)
2. 克隆项目仓库
3. 运行 `cargo build` 构建项目
4. 运行 `cargo test` 执行测试

## 代码规范

- 遵循 Rust 官方代码规范
- 使用 `cargo fmt` 格式化代码
- 使用 `cargo clippy` 进行代码检查
- 编写单元测试以覆盖新功能

## 提交规范

- 使用语义化提交信息 (Conventional Commits)
- 提交前运行完整测试套件
- PR 描述应清晰说明变更内容和目的

## 验证命令

```bash
# 格式化检查
cargo fmt --all -- --check

# 代码检查
cargo clippy --all-targets -- -D warnings

# 运行测试
cargo test

# 完整验证 (推荐提交前运行)
./dev/ci.sh all
```

## 安全注意事项

- 不要在代码中硬编码敏感信息
- 所有文件操作必须经过安全验证
- 遵循最小权限原则

## 提问和反馈

如有问题或建议，请通过 GitHub Issues 与我们联系。
