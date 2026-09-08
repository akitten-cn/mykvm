# 本地开发环境

2026-09-08，T00。原生 arm64；macOS 26.7 (25G224)；Xcode Command Line Tools 与 clang 可用。Git 2.50.1；Node 23.11.0 / npm 10.9.2。

原 PATH 中没有 Rust/Cargo/rustup/GitHub CLI。在工作区 `.toolchain/` 安装官方 Rust stable 1.98.1 (aarch64-apple-darwin)、rustfmt、clippy，使用 `../source/with-rust.sh` 显式设置进程级环境；没有修改 shell profile、全局工具链、Codex 设置或系统权限。

Node 23 对部分依赖产生 engines 警告，但当前 npm ci、lint、web build 实际成功。后续 CI 固定受支持的 Node 22；不升级用户全局 Node。

无现有 MyKVM 工作树，无项目祖先 AGENTS；未发现 sol-multi-agent-development skill，采用主线程顺序执行。项目 AGENTS 从交接模板复制，未改变用户全局规则。

源码本地 clone 自 https://github.com/XxMinor/mykvm.git，remote 命名 upstream，只读使用；分支 feat/mac-first-kvm。尚未创建 GitHub fork、推送、触发 CI 或发布。没有确认用于此项目的 GitHub 写入账户，先按本地开发路径继续。

Windows 原生构建：pending_environment；Windows/LOL 实机：optional_not_run。不自动启动应用、捕获输入、改剪贴板或关闭 Deskflow/RustDesk。

本地证据放 `.local-evidence/`，已通过 Git local exclude 排除。交接包 19 条 SHA256 校验全部匹配。可维护任务入口为 `docs/handoff/taskboard.json`，外层 handoff 保留原始包。
