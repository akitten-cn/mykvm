# T21 · 原生平台检查入口

新增 `.github/workflows/native-preview.yml`，矩阵 macos-14/windows-2022；只读 contents 权限、无发布密钥、不发布 release。固定 Rust 1.98.1、Node 22，运行锁文件依赖、前端 lint/build、隔离检查、核心文件格式检查、原生 cargo check/lib tests。执行不启动 MyKVM、不进行键鼠注入或 LOL 测试。

两平台复用 `node scripts/check-native.mjs`。Windows 无 CI 时在已有 Node/Rust/Xcode 对应平台工具齐备的普通终端运行同一入口；Windows npm 调用由 cmd.exe 处理，所有子命令失败立即退出。每项保存真实退出码、时间、SHA 与日志到 `.local-evidence/native/`，工作流归档日志。

2026-09-09 在 fork `akitten-cn/mykvm` 的 GitHub Actions 上实际运行原生矩阵。提交 `94ff6ed173ba70e4ebab48e20a80209e1693d665` 的 macOS 14 与 Windows Server 2022 job 均通过；Windows job 完成原生 cfg 编译及 216 个库测试。运行证据：[Actions run 34344520603](https://github.com/akitten-cn/mykvm/actions/runs/34344520603)。W01 与 `windows_build` 已更新为 `pass`，该结果不代表 Windows 物理输入或双机运行通过。

全仓库 fmt/clippy 仍有上游已知问题。再次严格 Clippy 检查失败 51 项（基线 53 项），新 control_ports/routing/fork_policy 没有诊断；未删除 lint 或全局压制 warning。后续 T20 处理相关基线债务，本任务不把非交互脚本通过包装成严格 Clippy 通过。

安装包生成由 T23 的同一 Windows job 完成。fork 已推送到 [akitten-cn/mykvm](https://github.com/akitten-cn/mykvm)；未创建 GitHub Release。
