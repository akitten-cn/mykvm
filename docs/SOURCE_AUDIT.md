# T01 源码基线核验

实际开发 SHA：`a2ea4164861de31b562c8417eeb7879dbc8c23cb`，clone 时上游默认 main。初始工作树干净。

参考 SHA `bb5421fe1d4c0c8c72bb3f6c0c35f0a0f994209b` 在同一 clone 可达，比 main 多 12 个文件、4,112 行新增、114 行删除，主要为跨屏文件拖放。选择已检出的 main，避免把本轮排除的拖放扩展作为隐含依赖；不重置任何用户工作树。执行定位以此 main 为准。

## 核验结果

| 位置 | 实际事实 | 后续任务 |
|---|---|---|
| quic_transport.rs run_transport | 已有非阻塞 datagram、独立 stream task、发送 semaphore；commands 是 UnboundedSender | T08 保留已有能力，补预算 |
| quic_transport.rs client_config/spawn_accept_loop | 服务端证书 pinning；with_no_client_auth；入站回调只携带 payload+SocketAddr，没有连接身份 | T05 绑定实际连接授权 |
| input.rs handle_input_datagram | 带凭据包校验 cluster/pair_secret；后续无凭据包按 SocketAddr+TTL 放行 | T05 必须替换，不能复用地址缓存当连接授权 |
| input.rs packet_authorized_fields | 校验群组 secret 后，以自报 key 或 ID 匹配 controller；还有 legacy local-device 例外 | T05 明确逐设备信任、去掉隐式降级 |
| lib.rs complete_pairing_from_confirm | 人工验证码限制来源、期限和尝试数；接收端保存同一 cluster secret 和 controller 记录 | 有应用层授权，不能称无授权；但群组 secret 不是逐连接身份 |
| lib.rs refresh_paired_controller_keys/update_device_from_peer | 发现消息可刷新持久 controller/device 证书；同群组 hostname 也可匹配 | T05 禁止广播成为新信任根，变更需重新配对 |
| shared_input.rs / input.rs inject_input_command | ReleaseAll 是内部命令，主进程处理分支为空；Windows helper 才有 release_pressed_inputs | T07 必须真实提交释放，不能把枚举存在当实现 |
| input.rs windows_mouse_proc/windows_keyboard_proc | 已有 Windows 真实钩子、方向切屏及 Mac VK 映射 | T09–T12 接入明确动作、快速路径、冻结映射 |
| clipboard.rs | 文本 256KiB、图片 32MiB，已有空文本过滤；Mac 文本通过 pbpaste/pbcopy | T13–T16 原生后端、类型化结果及预算 |
| lib.rs clipboard loop | 1200ms 回声静默期，会屏蔽用户紧接着的新复制 | T15 按操作/版本抑制回声 |
| lib.rs setup/ensure_main_window/destroy_main_window | Rust 启动 runtime；窗口可销毁重建；Mac 单实例函数直接返回 true | T17 复用后台所有权，补无窗口启动及 Mac 单实例 |
| main.rs | release Windows 无控制台属性已存在 | 保留 |
| tauri*.json / lib.rs updater / desktopApi.ts | 上游 identifier、公钥、更新 endpoint；前端启动主动检查且可安装更新 | T02 全链路禁用 |
| scripts / NSIS / helper | Mac 安装脚本会退出/覆盖上游、清 quarantine、运行钥匙串签名；NSIS 管理 SYSTEM helper、杀进程、改防火墙 | 未执行。T02 从本地预览构建移除这些副作用 |
| Cargo.toml | main 声明 Rust 1.77.2，非参考提交的 1.89；锁定依赖实际用 Rust 1.98.1 成功检查 | 不因旧文档盲改依赖或 MSRV |

## 基线命令

所有命令均在原版 main 源码上执行。JSON 与完整日志见 `.local-evidence/baseline-*.json/.log`。

| 命令 | 退出码/结果 |
|---|---|
| npm ci | 0；engines 警告 |
| npm run lint | 0 |
| npm run build | 0；两个动态导入 warning |
| cargo check --locked --lib | 0；8 个原有 warning |
| cargo test --locked --lib | 0；105 passed，0 failed，0 ignored |
| cargo fmt --all -- --check | 1；原有格式差异 |
| cargo clippy --locked --lib -- -D warnings | 101；53 个原有诊断，包括死代码、C 字符串写法、无效 u16 上界比较 |

没有执行 app 或系统输入/剪贴板实验。上述是 Mac 库与前端构建证据，尚非 .app/.dmg 交付，也不覆盖 Windows cfg。

T01 已完成源码审查与基线记录；fmt/clippy 失败保留为已知基线债务，不能称全部检查通过。后续任务独立验证新代码并在 T20 处理其相关警告，不通过删测试/关闭警告伪造成功。
