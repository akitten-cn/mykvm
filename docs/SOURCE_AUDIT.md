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

## T20 收尾源码审查

在 SHA `6effc789ff0ccac45470af0494d353f651dc4bf9` 复核完整实现。control/input 队列、motion 单槽、stream 并发和 bulk 字节均有上限；顶层 QUIC 命令通道仍为进程寿命级无界通道，但高频 motion 不逐帧排队，bulk 另受 128 MiB 与并发门控。入站注入必须由 TLS 连接证书命中持久信任，再通过角色、peer、session、boot/generation 和序号门控，发现广播不会写入信任。

End、断流、租约、失败和紧急返回均连接到本地优先恢复及 pressed-state 释放；runtime 停止会设置各 worker stop flag、关闭 QUIC，显式退出清理单实例资源。配置迁移保留用户布局，IPC 输入先做总大小和字段校验。适用自动化与 Mac 库检查全通过；严格全仓 fmt/clippy 仍为已记录基线债务，Windows 条件代码没有获得原生编译证据。

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

## T11 Windows 焦点交接增量核验

原生产 `WindowsV2FocusPort` 固定返回 Unavailable。现由 Windows 捕获线程拥有一个屏幕外的原生 STATIC 工具窗口，并在 Router 完成 Ready 和物理键释放门槛后执行一次合法前台切换。只有焦点结果确认为该窗口时才允许发送 Commit；失败写入 AppRuntime 可见中文状态并由既有 Router 返回本地，不循环抢占，也不合成 Alt+Tab。

焦点窗口与低级 hook 生命周期一致，钩子安装失败和正常停止均销毁窗口。会话返回本地时以原子交换取出交接前 HWND，并在仍为有效窗口时单次请求恢复。Mac 接收端不改变目标应用焦点。Windows cfg 尚未真实编译或运行；本轮证据限于 Rust/FakeFocus 测试、生产源码隔离检查、Mac 条件编译和前端构建。

## T08.d 显示布局增量核验

V2 Prepare/Ready 现在回显并校验由显示器标识、逻辑宽高和 scale 生成的稳定布局版本；motion 同时携带显示器和版本。接收端从保存的逻辑布局与只读检测的原生布局构造映射，先检查显示器内坐标，再生成系统全局坐标。可靠按钮/滚轮的位置快照复用相同映射，因此不能绕过 motion 的边界门控直接点击。

接收租约线程以 500 ms 间隔检测本机显示器，只有检测结果变化时才写回运行布局。活动显示器的尺寸、缩放、原点或存在性变化会结束会话、清除待处理位置并调用 pressed-state 真实释放路径。Windows 生产控制目标已计算同一版本，但 Windows cfg 尚未在原生工具链编译；本轮没有实际移动光标或拔插显示器。

## T12 Mac 键位映射增量核验

原前后端默认将 Ctrl 与 Meta 隐式互换，V2 接收端却没有应用这项设置；Mac Caps Lock 还会绕过键位表并固定合成 Ctrl+Space。现将默认统一为字面保留，设置页增加明确的保留/互换预设，V2 接收端在 pressed-state 之前应用同一套保留左右侧信息的映射。key-up 使用账本冻结的目标，不受运行中设置变化影响。

Caps Lock 恢复普通 macOS 键码 57 注入，不再假设系统输入源快捷键。receiver-only 的 V2 分支不会调用本地捕获启动函数；NativeInjector 的辅助功能和 Secure Input 检查仍位于 Ready 与每次提交路径。源码、FakeInjector 和 Mac 条件编译已核验，真实 TCC 撤销、终端中断、Command 快捷键和中文输入法尚未操作。

## T19 安全回环增量核验

原有 QUIC 回环测试分别验证证书身份、control 或 input，但没有把认证 transport 接到 ReceiverSessionRuntime 和 FakeInjector。新增 test-only 模块使用两套独立持久身份目录，运行真实本机 QUIC control/input/datagram，并由同一个接收会话处理器完成握手、可靠按键和 motion。测试后 endpoint 关闭、临时目录删除。

故障序列跨三个 boot generation：旧 motion 不作用；结束会话的关键帧不能进入新会话；handler 拒绝导致的 stream close 释放账本；租约超时也释放账本。模块由 `#[cfg(test)]` 隔离，生产构建不会暴露 fixture 凭据、测试端口或 FakeInjector 入口。

## T13 Windows 剪贴板增量核验

原 Windows 路径与其他平台一样由 120 ms/轮询读取 arboard，无法区分 busy、empty、unsupported 和错误。现有普通用户后台线程新增 message-only window 和剪贴板格式监听；窗口只发送 sequence 通知，内容在工作线程以原生 Unicode handle 读取。三次有界 busy 重试和读取前后 sequence 比较阻止格式切换时采用陈旧文本。

Drop、WM_CLOSE、WM_NCDESTROY 和异常消息循环退出均有对应清理。源码没有活动用户会话枚举、SYSTEM helper 或跨会话读取。Mac 条件编译和静态 API 核验通过，但本机缺 Windows target 标准库，因此函数签名尚需 W01 原生编译确认；旧 LAN 总门禁未因本任务翻转。

## T14 Mac 剪贴板增量核验

原 Mac 文本路径在每次轮询中执行 pbpaste，写入执行 pbcopy；现统一为已有 arboard 的进程内文本/图片后端。新增 NSPasteboard changeCount 只读适配器，工作线程比较计数后才读取内容；无目标时刷新计数基线，避免重连触发旧内容。

锁文件只增加 mykvm 对已存在 `objc2-app-kit 0.3.2` 包的直接引用，没有引入新版本树。Mac 编译和静态无子进程检查通过；真实 pasteboard 读写因需要保存、修改和恢复用户内容而未执行。

## T15 双向文本剪贴板增量核验

旧 `run_clipboard_sync` 使用包内 cluster secret、从发现数据直接构造 endpoint，并以 1200 ms 时间窗作为图片回声兜底；同时整个入口因旧 LAN 门禁关闭而不可运行。新的 V2 文本路径不启用旧 LAN 数据：出站 endpoint 由 `TrustedPeerRegistry` 按 peer ID 和远端角色选取固定证书，入站先取得 `AuthenticatedPeer`，再核对机器方向和操作来源。TLS pinning 因此与应用层配对身份、角色和入站写入授权同时成立。

同步状态只保留当前赢家与一次待识别的远端写入，不保存无限操作历史。stream handler 在剪贴板专用锁内完成判断、假/真实 writer 和提交，避免并发旧操作覆盖新操作；该锁不进入键鼠数据路径。生产日志不插入文本或摘要。旧 V1 代码仍由 `LEGACY_LAN_DATA_ENABLED=false` 隔离，当前运行时只调用新的无时间窗实现。

Mac 条件编译与前端构建通过，真实剪贴板没有读取或改写。Windows listener 和 V2 接线尚未在 Windows 标准库或原生主机编译，状态保持 `pending_environment`。

## T16 图片与 bulk 预算增量核验

原有通用 stream 只有数量并发限制，载荷进入发送队列和接收 `read_to_end` 前没有跨连接字节预算。现以进程级 128 MiB 原子预算覆盖 bulk 发送载荷、接收读取和文本/图片编码工作区；不足时在复制或解码前拒绝，permit 随成功、错误或超时路径自动释放。接收单项保守预留 124 MiB，因此同一时间只允许一个最大图片解码工作，并给小型文本编码留出余量；control、reliable input 和 motion 不消费 bulk 预算。

图片 V2 操作复用 T15 的来源绑定、Lamport 排序和系统 revision 回声规则。协议先检查非零尺寸、`width * height * 4` 溢出与 32 MiB 原始上限，再核对预期 base64 长度和摘要；系统 writer 在 base64 解码前重复同一尺寸/长度检查。设置默认关闭图片，游戏模式也强制禁止发送和接收图片，文本同步不受影响。

## T17 后台生命周期增量核验

原配置即使 `visible=false` 仍会在自启时构造 WebView，前端还会在客户端首次加载时自动写入自启项；macOS 的单实例函数固定返回 true。现配置不预建窗口，Rust runtime、QUIC、输入和剪贴板先由 App 管理，普通启动或已有实例激活时才创建设置窗口，关闭时销毁 WebView 而不停止后台。

macOS/Unix 在进程入口用用户临时目录中的 0600 Unix socket 取得唯一所有权，重复进程在进入 Tauri/runtime 前退出并发送激活消息。现有 Windows mutex/event 继续保留。自启只经设置页显式调用 Tauri autostart 插件；Mac launcher 是普通用户 LaunchAgent，参数使后台启动不创建设置窗口。源码未添加 SYSTEM 服务、驱动、登录前控制或睡眠抑制。

## T18 IPC 与诊断增量核验

设置保存入口原先只检查快捷键冲突，允许前端提交无界 JSON、无效枚举、非有限缩放和超长网络字段；手工探测/配对主机及验证码也直接进入网络函数。现统一在 IPC 边界拒绝超过 2 MiB 的布局、超过数量/字段上限的设备、异常屏幕尺寸/缩放、无效模式、含控制字符的主机和非六位数字验证码。后端拥有的配对密钥及信任字段仍由 merge 策略保护，不采用前端副本。

诊断报告原先复制本机名、IP、远端设备名/host 以及日志和配置绝对路径。现复制报告只保留平台、角色、状态、端口、计数和匿名 peer 序号；本地 UI 仍可通过专用“打开日志目录”动作访问路径。旧 V1 拒绝日志中的设备 ID、公钥状态以及 Mac 注入失败的具体键码已移除，日志 fixture 扫描覆盖敏感字段名。
