# T28 配对 ACK 与空闲路由修复

用户在 Windows 0.1.2 上向 Mac `192.168.3.17:47833` 配对时，UDP 发现和验证码挑战均成功，但确认阶段报告 `failed to read QUIC stream ack: read error: connection lost`。真实 QUIC 回环测试复现了同一错误：Mac 接收端已接受并持久化配对，却在 `send.finish()` 后立即关闭未认证连接，使 `ok` ACK 可能尚未送达 Windows。

修复 SHA `221fd14a92715c13552eaed8dc5672af2a25e320` 在关闭配对连接前等待对端消费 ACK，并为 ACK 写入/结束失败补充日志。新增回环测试在修复前以相同 `connection lost` 失败，修复后通过。Windows 控制端启动保护 SHA `1d4a8e93fcf6dfa8ea8bbc3ee6bdf1de241a0663` 在安装全局 hook 前强制本地路由并清空剪贴板目标，避免异常退出或运行时重启遗留状态影响 RustDesk 等程序。

0.1.3 构建 SHA `c73efd374a3be124b1e278866cf366b5e911df4d` 的本机检查通过 228 个 Rust 库测试、24 个隔离测试、前端 lint/build、Mac 库检查及核心格式检查。Mac ARM64 app 已生成，DMG 因已有同名挂载卷导致 Tauri 外观脚本失败后，从其已完成且卸载的中间映像确定性转换为 UDZO；`hdiutil verify` 通过。DMG SHA-256 为 `be0c1c93747b68c52b2a61e98e98047da21dfeaf3aa44cee4dc0a64553660c72`。

[GitHub Actions run 34357658307](https://github.com/akitten-cn/mykvm/actions/runs/34357658307) 的 macOS 14 与 Windows Server 2022 job 均通过。CI 生成的 Windows NSIS 已下载并与 CI `SHA256SUMS` 交叉核对，SHA-256 为 `a50fe23e7f14d176633e8b1e468d8e669544844fba020f4b846c4cbd35b3d087`。真实双机验证仍单独记录；安装 0.1.3 前不覆盖当前应用、不修改 TCC，也不结束 RustDesk/Deskflow。
