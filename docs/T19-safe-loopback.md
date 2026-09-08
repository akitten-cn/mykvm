# T19 Mac 安全回环与故障注入

实现由两个原子提交组成：`856434bc60b6af1c36a1e916215161fa9c0bd71a` 建立双端认证回环，`c184091c6762118c56faf8494cc3ee6215cb0841` 增加乱序、旧会话、断流和租约故障。

回环模块 `safe_loopback.rs` 只由 `#[cfg(test)]` 引入。控制端和接收端分别使用独立临时身份目录，启动真实本机 UDP/QUIC endpoint，再以对端实际证书建立双向持久信任。接收端唯一注入器是 `FakeInjector`；测试源码不引用 NativeInjector、平台捕获启动、CGEvent 或 SendInput。临时 endpoint 在 Drop 时关闭，临时身份目录随后删除。

单条端到端测试依次验证：

1. TLS 认证后的 control stream 完成 Hello、Prepare/Ready 和 Commit/CommitAck。
2. 可靠 key-down、motion datagram、可靠 key-up 按序到达 FakeInjector。
3. sequence 更旧的 motion 返回 `Stale`，不追加注入事件。
4. 第一代 End 后建立第二代会话；第一代旧帧被生产 input callback 以 WrongSession 拒绝，且输入流关闭会释放第二代已按下的键，旧帧中的 B 键从未注入。
5. 第三代会话按下 C 后推进可控时间，租约到期关闭会话并提交 C key-up。

运行方式：

```sh
../source/with-rust.sh cargo test --manifest-path src-tauri/Cargo.toml --lib safe_loopback::m05_authenticated_loopback_applies_reliable_and_motion_only_to_fake_injector -- --nocapture
```

提交后完整检查日志 `.local-evidence/t19-postcommit.log` 退出码 0：206 个 Rust 库测试、14 项隔离检查、前端 lint/build、Mac `cargo check` 和相关格式检查通过。M05 与 A16 更新为 pass；A10、A18、A22、A24、A26 的既有 pass 证据继续有效。测试未启动应用、未读取或控制真实桌面、未触碰真实剪贴板。
