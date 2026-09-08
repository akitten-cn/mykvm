# T21 · 原生平台检查入口

新增 `.github/workflows/native-preview.yml`，矩阵 macos-14/windows-2022；只读 contents 权限、无发布密钥、不发布 release。固定 Rust 1.98.1、Node 22，运行锁文件依赖、前端 lint/build、隔离检查、核心文件格式检查、原生 cargo check/lib tests。执行不启动 MyKVM、不进行键鼠注入或 LOL 测试。

两平台复用 `node scripts/check-native.mjs`。Windows 无 CI 时在已有 Node/Rust/Xcode 对应平台工具齐备的普通终端运行同一入口；Windows npm 调用由 cmd.exe 处理，所有子命令失败立即退出。每项保存真实退出码、时间、SHA 与日志到 `.local-evidence/native/`，工作流归档日志。

本机验证该入口退出码 0：6 项隔离检查，127 个 Rust 单测，lint/web build/lib check 和三个新核心文件的格式检查通过。没有执行 Windows runner，因此 W01 与 windows_build 仍 pending_environment。工作流存在不等于 CI 已运行；没有生成 Windows 二进制。

全仓库 fmt/clippy 仍有上游已知问题。再次严格 Clippy 检查失败 51 项（基线 53 项），新 control_ports/routing/fork_policy 没有诊断；未删除 lint 或全局压制 warning。后续 T20 处理相关基线债务，本任务不把非交互脚本通过包装成严格 Clippy 通过。

安装包生成由 T22/T23 完成，本工作流只做编译检查和测试证据。当前没有已确认可用于本项目的 GitHub 写入账户，未创建远程 fork、推送或实际触发 Actions。
