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

## T05.b 增量核验

原 `with_no_client_auth()` 已替换为客户端出示本机持久证书的 TLS 配置；服务端仍允许没有既有信任的连接完成限时人工配对，但不会给它数据权限。应用层从 TLS peer identity 读取实际证书，精确匹配后端维护的 `trustedPeers`，再生成连接代次和方向角色。发现广播更新仍会影响在线地址和旧设备字段，但不会写入这一信任表；T06 新出站路径必须只从持久信任选证书。

入站 datagram reader 只为已认证连接创建。stream 入口先将配对声明证书与 TLS 出示证书绑定，普通数据则要求认证上下文和正确角色。旧数据包内部的组 secret 检查暂时保留为附加校验，但不再承担连接身份职责。真实 LAN 总门禁仍关闭，直到 V2 帧和会话层完成。

## T04.b2 运行时接线增量核验

`AppRuntime::start_quic_transport` 原先为 V2 control/input 传入空 handler；现已共享一个 `ReceiverSessionRuntime<NativeInjector>`，control 与 input 都检查 `input_receive_enabled` 后才进入会话适配器。不同 TLS 连接代次不能共享会话。旧 `start_discovery` 的总门禁分支现在只启动认证 QUIC endpoint，不绑定或广播旧 UDP discovery。

`input.rs` 的普通用户 NativeInjector 复用现有平台注入函数，并在 Ready/每次提交前读取权限状态。源码复核同时确认主进程 `inject_input_command` 对 ReleaseAll 仍为空分支，不能满足 T07；因此新增独立 `V2_NATIVE_RECEIVER_ENABLED = false` 编译期门禁。应用即使启动也不会接受真实 V2 输入，直到按会话释放账本实现并经 FakeInjector 验证后再审查开门。

## T04.b3 Windows 控制端接线核验

Windows 原有低级键鼠 hook 和热键/贴边入口已接到独立 V2 controller client；网络回调只写有界队列，捕获线程拥有 Router 和平台状态。准备期保持本地，匹配 CommitAck 后才允许可靠关键事件，任何 V2 motion 在 T08 前失败关闭且不会调用旧 V1 datagram。

本机 `rustup target list --installed` 仅返回 `aarch64-apple-darwin`。实际执行 Windows MSVC target 的 `cargo check` 后，依赖下载完成，但 rustc 在项目代码前报 E0463 `can't find crate for core`，退出码 101。没有把该结果标记为 Windows 条件编译通过，也没有修改全局 rustup 安装。

## T08.b motion 调度核验

原 QUIC transport 已有非阻塞 datagram 发送和连接预热，但通用命令入口是无界通道。motion 现不把每次移动直接加入该通道：每个 `MotionHandle` 只有一个可覆盖 payload 槽和一个 scheduled 位，因此积压量与鼠标事件频率无关。worker 每次 flush 后回到 transport loop，再按需重排；关闭标记阻止排队的旧会话位置继续发送。可靠 input 的有界 stream 队列和 bulk stream 并发限制保持不变。

## T08.c 顺序和预算增量核验

生产 ReceiverSessionRuntime 已直接消费 TLS 认证连接上的 MKM2 datagram，绑定完整 AuthenticatedPeer 和活动 SessionId。可靠按钮/滚轮携带的位置先应用并推进 motion floor，依赖未来 reliable sequence 的 datagram 只保留最新一帧。Windows 端每个物理 delta 先累计进远端绝对坐标，再交给单槽调度；控制端 runtime 统一写入序列依赖。

复核接收路径发现 generic bulk handler 会同步占用仅两个 QUIC async worker，且跨连接没有全局 stream task 上限。现由 8 个全局入站 permit 限制活跃 stream，bulk handler 转入 blocking pool；真实回环用两个阻塞 bulk handler 验证 input stream 仍推进。协议仍缺详细设计要求的 display_id/layout_revision，因此 A29 和 T08 父任务没有标记完成。

## T09 控制热键增量核验

原 Windows hook 只有方向切屏返回，系统全局快捷键也只处理按下事件。现新增三个持久化控制动作并同时接收按下/释放，两个入口共享同一去重状态。返回和紧急返回在捕获线程协调锁之前设置 Router 使用的原子本地门控，随后用不同 ReturnReason 结束会话；接收端 pressed-state 会释放已转发的热键修饰键。

控制端原来向 Router 永久传入 `keys_released=false`，现由 Windows 捕获线程轮询完整虚拟键范围后传入实际结果。`WindowsV2FocusPort` 仍返回 Unavailable，编译期控制端门禁保持关闭。hook 仍读取共享 context/layout 并执行少量 Windows FFI；A28 的严格快速路径审查必须在 T10 完成，不能将本次纯逻辑与 Mac 编译结果视为 Windows hook 已通过。

## T10.a 本地游戏路径核验

手动游戏模式由保存布局写入进程级 AtomicBool。Windows hook 的第一个业务判断只读该原子值，命中后直接交给系统；不读取 capture context，也不进入旧的边缘/发送路径。普通路径的 context 获取从阻塞 mutex 改为 try-lock，FFI 外壳捕获 panic 并请求本地门控。桌面/远程 inner hook 仍执行光标和发送协调，A28 尚未闭环。

T10.b 将 inner hook 缩减为缓存热键匹配和有界 try-send。Windows 原始事件在捕获线程消息循环中处理，原有绝对位移累计、latest-wins motion、可靠关键事件及光标状态代码继续复用。队列容量固定 1024，溢出不阻塞 hook，并通过共享 LocalOverride 和返回动作失败开放。Windows cfg 尚未真实编译，此结论限于源码审查、语法解析和跨平台纯测试。
