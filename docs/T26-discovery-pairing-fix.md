# T26 发现与配对响应修复

用户在 Windows 服务端手动添加 Mac 客户端 `192.168.3.17:47833` 时收到“没有收到 MyKVM 响应”。源码核验发现 `AppRuntime::start_discovery` 在 `LEGACY_LAN_DATA_ENABLED=false` 时启动 QUIC 后直接返回，连带跳过安全发现与配对所需的 UDP socket 绑定。因此 0.1.0 Mac 客户端不会监听 47833。

修复提交 `08171ae78c7901ad98bcbb9d1e0885ff6cdef12d` 删除该提前返回，让接收端继续绑定 UDP 发现端口并启动认证 QUIC；`start_input`、`start_clipboard`、旧剪贴板/文件处理和发送入口仍受禁用策略约束。隔离测试先在旧代码上复现失败，再验证禁用旧数据通道时发现初始化仍会执行。新增本机 UDP 回环测试向显式 `127.0.0.1:<port>` 发送 probe 并收到有效 peer announce。

本机完整检查通过：226 个 Rust 库测试、23 个隔离测试、前端 lint/build、Mac cargo check。0.1.1 ARM64 app/DMG 原生构建成功，`hdiutil verify` 有效。GitHub Actions [run 34349252256](https://github.com/akitten-cn/mykvm/actions/runs/34349252256) 的 macOS 14 与 Windows Server 2022 原生 job 均通过；Windows job生成 0.1.1 NSIS。独立 Linux [CI run 34349252180](https://github.com/akitten-cn/mykvm/actions/runs/34349252180) 也通过。

这些证据验证代码路径、UDP 回环和平台构建。尚未在 `192.168.3.17` 与用户的 Windows 机器之间重跑真实配对，因此双机运行状态仍为 `optional_not_run`，需两端安装/运行 0.1.1 后验证。
