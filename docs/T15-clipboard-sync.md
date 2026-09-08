# T15 双向文本剪贴板证据

实现由两个原子提交组成：同步核心 `73f4868d489bd2ae8e9bc151fcdd9b46aa7ff0c9`，认证生产接线和设置 `112539cd9a0e20851fc55c03df93d0b9c89ded78`。

V2 文本操作携带 boot ID、本地序号、来源 peer、发送端系统版本、Lamport 计数和 SHA-256 摘要。接收端按 `(lamport, origin_peer, boot_id, local_sequence)` 选择稳定赢家；重复和旧操作幂等确认，不再次写入。远端成功写入后记录本机系统版本与摘要，只抑制该次系统回声，不使用固定静默时间，因此紧接着复制不同文本仍产生新操作。启动和重连只建立系统版本基线，不发送已有剪贴板；用户可以在设置中显式重新发送。

发送端必须用持久信任注册表中的 peer ID、角色和证书构造 bulk endpoint。入站操作只在 QUIC 连接已认证、角色符合本机方向、操作来源等于 TLS peer ID、剪贴板开关已启用时处理。操作决定、系统写入和提交由剪贴板专用锁串行化，避免并发 stream 的旧写入晚于新写入完成。日志和界面提示只包含错误、字节数和阈值，不记录正文或摘要。

文本阈值默认 1 MiB，界面可选 256 KiB、512 KiB 或 1 MiB；后端将配置限制在 64 KiB 至 1.5 MiB，V2 bulk 编码有 2 MiB 硬上限。超限返回独立结果并显示提示，不截断正文。图片在 T16 前不进入新的生产同步路径。

提交后证据全部退出码 0：216 个 Rust 库测试、17 项隔离检查、前端 lint/build 和 Mac `cargo check --lib`。日志为 `.local-evidence/t15-postcommit-rust.log`、`t15-postcommit-isolation.log`、`t15-postcommit-lint.log`、`t15-postcommit-build.log`、`t15-postcommit-mac-check.log`。

测试只使用内存文本、Fake writer 和本机临时 QUIC loopback，没有启动应用或读取、修改真实剪贴板。M03 保持 `not_run`；Windows 标准库缺失，Windows 构建为 `pending_environment`，Windows/LOL 实机为 `optional_not_run`。
