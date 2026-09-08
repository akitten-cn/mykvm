# T20 自动化回归与主线程审查

实现与修复 SHA：`6effc789ff0ccac45470af0494d353f651dc4bf9`。在 Darwin arm64 上运行 `scripts/check-native.mjs`，8 个步骤全部以退出码 0 完成：225 个 Rust 库测试、22 个隔离测试、前端 lint/build、核心文件格式检查和 Mac `cargo check --locked --lib`。封装日志为本地 `.local-evidence/t20-postcommit-final.log`，分步日志在 `.local-evidence/native/`。

回归发现旧的原生状态仍把停止状态显示为 `stubbed`，已改为 `idle`，不支持的平台改为 `error`；开发环境检查也不再报告已经完成的原生输入为 stub。图片剪贴板的非 Windows fallback 改为非惰性选项组合，端口扫描用 `checked_add` 终止溢出，去掉对 `u16::MAX` 的恒假比较。

资源审查确认：control/input 使用有界 channel；motion 使用每目标单槽 latest-wins；stream 有并发 semaphore、帧上限、5 秒发送等待和 128 MiB 全局 bulk 预算；接收 stream 也有并发上限及读取前预算。QUIC 顶层命令 channel 仍为进程寿命级无界 channel，但高频 motion 每槽最多排一个 flush，bulk 字节和并发另有限制。它只剩低频控制命令入口，作为后续可进一步收紧的非阻塞风险记录。

授权审查确认所有入站 input/control/motion/clipboard 均由实际 TLS 连接证书映射到持久信任记录，并再次校验角色、peer、会话与代次；发现广播不能刷新信任。清理审查确认 End、断流、租约、错误和紧急返回均走按键账本释放；runtime stop 设置 discovery/input/clipboard 停止标志并关闭 QUIC，显式退出释放单实例 socket。配置迁移保留旧布局读取，只移除已知 demo 设备；IPC 在进入网络或系统 API 前执行大小、枚举和字段校验。

仓库级严格 `cargo fmt --all -- --check` 仍以退出码 1 失败，严格 `cargo clippy --locked --lib -- -D warnings` 仍以退出码 101 失败（99 项）；日志分别为 `.local-evidence/t20-full-fmt.log` 和 `.local-evidence/t20-strict-clippy.log`。其类型与 T01 已记录的基线债务一致，主要为未使用的跨平台路径、参数数量、旧 C 字符串字面量和超大枚举；本轮新增的可行动诊断已修复，适用回归通过。Windows target 标准库/runner 不可用，Windows build 为 `pending_environment`；Mac 应用、Windows 实机和 LOL 实机均未运行。
