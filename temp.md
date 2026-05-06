提醒一句：今后所有 push 之前，建议先跑这两条本地验收，避免再撞 CI 才发现 bug：

cargo check --lib
cargo check --lib --no-default-features
cargo check --bin sen --release   # ← 关键，能暴露 bin-only 错误
cd desktop && bunx tsc --noEmit; cd ..
第三条 --bin sen --release 是我这次踩坑后加的——本仓库 bin 重新声明了 lib 模块树，只 check lib 抓不到 bin 路径错误。


cd D:\ai\agent_os\SenAgentOS-code\senagentos-cli
# ... 你做完代码改动 ...
git add .
git commit -m "your message"
git push origin main
# 发新版本
git tag v0.1.1
git push origin v0.1.1