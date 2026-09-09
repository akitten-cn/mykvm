# T27 QUIC 预热、macOS 权限显示与 RustDesk 共存修复

用户在已安装的 0.1.1 Mac 客户端看到 `V2 motion decode rejected: InvalidLength`，并报告辅助功能已开启但仍提示，以及 Windows 启用 MyKVM 后 RustDesk 无法输入和粘贴。

源码确认 `warm_quic_peer` 使用认证空 datagram 预热 QUIC。接收闭包此前无条件把每个 datagram 交给 motion 解码，空包因此形成 `InvalidLength` 并持久覆盖注入权限状态。修复 `084a2b3e82f1b386c8bcd646d9a4361a31c6582e` 将认证空 datagram 作为 transport warmup 静默消费，同时继续拒绝任何非空损坏 motion。测试先因缺少该分类失败，修复后通过。

Windows V2 `request_local_restore` 此前调用 `restore_windows_capture_state(..., false)`，导致返回本地后仍保留 MyKVM 剪贴板目标。修复 `73990b1c2f2edf00e9ab71b1dc753eaf999d83f0` 改为清除剪贴板所有权；隔离测试先在旧实现上失败，修改后通过。MyKVM 远端会话激活期间仍会独占 Windows 物理输入，这是产品控制语义；需要使用 RustDesk 时应先“返回 Windows”、紧急返回或暂停 MyKVM 后台。游戏模式可避免贴边误切换。

本机核验显示 `/Applications/MyKVM Local.app` 是 0.1.1，正在监听 UDP 47833/47834，并保存了一个已配对控制端。已安装 0.1.1 与新构建 0.1.2 都是 linker ad-hoc 签名，designated requirement 是各自不同的 CDHash。因此升级后 macOS 辅助功能列表可能保留一个视觉上开启、实际不匹配新二进制的旧记录。安装 0.1.2 后需要由用户删除旧条目、重新添加 `/Applications/MyKVM Local.app` 并重启应用；本轮没有自动修改 TCC、钥匙串或系统信任。

0.1.2 完整本机检查通过 227 个 Rust 库测试、24 个隔离测试、前端 lint/build 和 Mac cargo check。Mac ARM64 app/DMG 已生成，`hdiutil verify` 通过。GitHub Actions [run 34353184370](https://github.com/akitten-cn/mykvm/actions/runs/34353184370) 的 macOS 14 与 Windows Server 2022 job 均通过，并生成已核验的 Windows NSIS。真实双机输入、剪贴板和 RustDesk 切换仍需用户安装 0.1.2 后复测。
