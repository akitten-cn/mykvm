# 首批实现测试报告

日期：2026-09-08。受测代码 `83604777ef3ff49ab95821f9b0b03b74dbbc8fb3`，平台 darwin/arm64。后续收尾提交仅更新文档及任务状态。

从仓库根目录运行：

```sh
../source/with-rust.sh node scripts/check-native.mjs
```

实际执行记录保存在本地 `.local-evidence/native/results.json` 和同目录各命令日志，未纳入版本控制。最终检查于 00:39:36–00:39:43 UTC 执行，8 个子命令退出码均为 0。

|检查|结果|证明范围|
|---|---|---|
|Rust 库测试|127 passed，0 failed，0 ignored|上游 105 项，加策略 1 项、假平台 5 项、路由 16 项|
|隔离检查|7 passed|配置、调用路径和脚本的静态断言；不代替安装器实测|
|npm lint / build|通过|前端检查和静态资源生成|
|cargo check --locked --lib|通过|Mac 库条件编译，不代表 Windows 编译|
|新增核心 rustfmt|通过|control_ports、routing、fork_policy 三个文件|
|全仓 fmt|失败（基线已有）|未以大范围格式化混入原子实现|
|严格 clippy|失败|基线 53 项诊断，阶段复查 51 项；新核心未产生诊断。最终关闭旧 LAN 后未重跑此全仓检查|
|Windows 原生 CI|pending_environment|工作流存在但未推送、未运行|
|Mac 应用包/运行|not_run|没有 app/dmg 交付|
|Windows / LOL 实机|optional_not_run|不作为其余开发的关卡|

开发过程中路由补充测试暴露了重复 Ready 覆盖会话和部分准备失败清理问题，已修复并通过回归。测试默认使用 FakeCapture/FakeInjector，不调用真实键鼠或剪贴板。

纯核心通过不等于 A01–A04 等端到端用例通过；这些用例保留 not_run 并记录部分证据。认证正反测试、协议回环、真实 Windows 分支编译、可靠释放集成、资源数据均尚未完成。详见 taskboard、testcases 和 SOURCE_AUDIT。

## T05.b 连接授权增量

2026-09-08 在 Mac arm64 上执行 `../source/with-rust.sh python3 ../source/run-check.py t05b-native node scripts/check-native.mjs`，退出码 0。前端 lint/build、Mac cargo check、7 项隔离检查及 135 个 Rust 库测试全部通过。测试数较上一阶段增加 8 项。

其中两项使用真实本机 UDP/QUIC endpoint，但只传测试字节，不调用桌面、输入注入或剪贴板：已配对证书成功产生带连接代次和角色的认证上下文；未知证书连接发送的 datagram 未到达处理回调。其余负例覆盖证书旋转、重复证书、错误角色、配对声明证书与 TLS 证书不一致。A10、A11、A12 标记 pass；A43 和 V2 会话相关用例仍为 not_run。

提交后以相同检查再次复核，证据包装器记录 SHA `f51b6fa8b693615d7e1a3a782c2c2146c7b5d775`，退出码 0，日志为本地 `.local-evidence/t05b-postcommit.log`。工作树保持干净。

## T06.a 协议增量

2026-09-08 执行 `t06a-native` 检查，退出码 0：142 个 Rust 库测试、7 项隔离检查、前端 lint/build、Mac cargo check 和新增核心格式检查通过。新增 7 项协议测试覆盖错误版本/角色/能力、分配前长度上限、逐字节与粘包解析、截断、旧 boot 和结束会话。A13–A15 标记 pass；A16/A17 等待真实输入会话接入，仍为 not_run 并记录部分证据。该阶段没有打开网络服务或操作桌面。

提交后复核记录 SHA `ddfc343529812a810c20db9526040a71a0db3284`，退出码 0，日志为本地 `.local-evidence/t06a-postcommit.log`。

## T06.b control stream 增量

定向测试在 Mac 本机真实 QUIC 回环上通过：已认证 control stream 在不关闭写端的情况下连续交换 Hello、Prepare/Ready 和 Commit/CommitAck；另一测试验证 64 帧发送队列及 128 帧/秒速率上限会失败关闭。原有 bulk stream 使用同一 accept loop，完整回归将继续验证其 ACK 路径。`AppRuntime` 尚未接处理器，未启动真实服务或输入。

## T06.c reliable input stream 增量

2026-09-08 的定向库回归在 Mac arm64 上通过 151 项测试。新增真实本机 QUIC 回环证明已认证 Controller 的一条持久 input stream 可按顺序交付两个关键帧，且 handler 收到 TLS 绑定的身份；接收端不等待 EOF。4 KiB 单帧上限、256 帧及 256 KiB 发送预算、单连接唯一 input stream和全局 8 条上限均为硬边界，队列满会向调用方返回错误。

协议门控测试拒绝旧 receiver boot、结束后的帧与同会话复活、乱序/重复序列、空键码和序列空间耗尽。当前 `AppRuntime` 仍传入空 input handler，所以这些结果只证明传输及会话边界；没有调用 FakeInjector 或真实系统输入。A16/A17/A22 保留 not_run，并将已有证据记录为 partial，等待 T04.b/T07 贯通后完成端到端断言。

提交后复核记录 SHA `9c887bb5cf4ced1e6ae20e1d676a5ed4ba5a7551`，8 个检查步骤退出码均为 0，日志为本地 `.local-evidence/t06c-postcommit.log`。

## T04.b1 接收会话适配器增量

新增 6 项 FakeInjector 测试，把 TLS 认证产生的完整 `AuthenticatedPeer` 绑定到 control 和 input：不同连接代次不能提交输入；Ready 前检查 injector；Commit 后严格按序提交；Pong 报告最高已提交序列；重复 Commit 不重置序列；End 先关闭 gate 再请求 ReleaseAll。注入失败会使握手和 input gate 一起进入结束态并尽力 ReleaseAll。测试没有读取或操作真实桌面。

## T04.b2 AppRuntime 接线增量

`AppRuntime` 已构造共享接收会话，并把认证 control/input callbacks 接到 QUIC transport；V2-only 启动不再依赖已禁用的旧 UDP discovery。普通用户 `NativeInjector` 在 Ready 和每次提交前只读取当前辅助功能/Secure Input 状态，不主动弹授权。Mac 库回归 157 项通过；应用没有启动，macOS runtime 仍为 not_run。

主进程原有 ReleaseAll 仍为空操作，因此 `V2_NATIVE_RECEIVER_ENABLED` 编译期门禁保持 false。此阶段只能证明生产路径已接线和可编译，不能接收真实输入；T07 完成真实账本和释放后才允许打开。

## T07.a pressed-state 增量

新增 `PressedState`，以物理 scan code/extended 标识输入源，并冻结 key-down 当时的目标 key code。测试证明自动重复不增加所有权、两个物理源映射到同一目标时不会提前 key-up、正常 End 对每个目标只提交一次真实 up、按钮释放使用账本位置。ReceiverSessionRuntime 不再依赖空操作 ReleaseAll，而是逐项经 InjectorPort 提交释放。

故障测试覆盖 key-down/key-up 提交失败：会话立即结束并尽力释放；释放失败时账本恢复，后续重复 End 可重试。A19/A20 标记 pass；A21 只记录注入失败部分证据，因为 decoder/stream 失败仍需 T07.b 的租约通知。断线/健康超时 A18 仍未实现，因此生产接收门禁继续关闭。

## T07.b lease/health 增量

接收会话使用 3000 ms 活动租约，成功 Commit、关键输入和有效 Ping 刷新时间。FakeClock 测试证明截止前不清理、到期时关闭 handshake/input gate 并提交账本中的真实 up；Ping 可延后截止。释放提交失败会保留账本并写入可查询 fault，AppRuntime 将 fault 显示为 inject error。

QUIC input stream 无论正常 EOF、解码失败还是 handler 拒绝都会调用认证的 close handler；真实回环测试确认 handle 丢弃后回调携带原认证 peer。ReceiverSessionRuntime 随即结束匹配会话并释放，避免 control Ping 在坏 input stream 后无限续租。A18/A21 标记 pass。165 项 Rust 库测试通过后，`V2_NATIVE_RECEIVER_ENABLED` 开为 true；旧 LAN 路径仍为 false。

## T04.b3.a controller handshake 增量

控制端握手纯逻辑生成 Hello/Prepare，拒绝错误 request、未就绪 Ready 和错误 CommitAck，并以控制端 boot、接收端 boot 和随机 nonce 构造新 SessionId。只有 Active 会话可发 Ping/End；Pong 必须匹配当前 session、已发送 ping 序列和单调远端进度。2 项定向测试通过。此阶段尚未接 Router、QUIC handle 或 Windows 捕获，不把纯逻辑测试计为实际切换可用。

提交后复核记录 SHA `212d3c7772976af60e58b9c5ea40359b1c249ed0`，167 个 Rust 库测试及其余 7 个检查步骤退出码均为 0，日志为本地 `.local-evidence/t04b3a-postcommit.log`。

## T04.b3.b1 controller runtime 增量

控制端握手不再在 Ready 时自动生成 Commit；`ControllerRuntime` 只有在 Router 确认物理键已释放且 FocusPort 成功后才显式 Commit。5 项 FakeCapture/FakeFocus 测试覆盖正常激活、输入严格序列、ACK 前应急取消、提前 ACK/非法活动帧失败关闭和返回本地的动作顺序。

2026-09-08 在 Mac arm64 上执行 `../source/with-rust.sh python3 ../source/run-check.py t04b3b1-native node scripts/check-native.mjs`，退出码 0。最终复跑为 172 个 Rust 库测试、8 项隔离检查、前端 lint/build、Mac cargo check 和包含新适配器的核心格式检查全部通过。

提交后以实现 SHA `47171b1812e1ea9af6ea70896d9ee9ba30437231` 再次运行同一整套检查，8 个步骤均为退出码 0，日志为本地 `.local-evidence/t04b3b1-postcommit.log`。

这些测试不启动应用、网络或系统输入。Windows hook 和 QUIC control/input handle 尚未接入，Windows 构建仍为 pending_environment，不能据此宣称产品可用。

## T04.b3.b2a controller client 增量

新增 64 帧有界入站 control 队列和生产 `QuicControllerTransport`，将认证 Receiver peer 的 control/input handle 生命周期交给单一 `ControllerClient`。QUIC 回调只做非阻塞投递；溢出、协议错误或任一发送队列失败会先请求本地恢复再断开。

3 项 FakeControllerTransport 测试通过，覆盖门控握手到可靠 input、input 队列失败后的本地恢复，以及重复 begin 不替换握手中连接。提交前整套检查通过 175 个 Rust 库测试及其余 7 个步骤，日志为本地 `.local-evidence/t04b3b2a-native.log`。

提交后以实现 SHA `94af28d3726ed1db02e7b021842e6ad2d5fa214a` 再次运行整套检查，8 个步骤均为退出码 0，日志为本地 `.local-evidence/t04b3b2a-postcommit.log`。Windows 平台接线尚未实现，状态不变。

## T04.b3.b2b Windows capture wiring 增量

Windows 生产路径已实例化 V2 controller client，接入热键/贴边准备、捕获线程 control 轮询、可靠键/按钮/滚轮、每秒 Ping 和先恢复本地的返回路径。2 项跨平台转换测试和 1 项模式测试新增；控制端编译期门禁仍为 false，T08/T09/T11 前不会真实接管输入。

2026-09-08 执行 `t04b3b2b-native`，退出码 0：178 个 Rust 库测试、9 项隔离检查、前端 lint/build、Mac cargo check 和核心格式检查通过。日志为本地 `.local-evidence/t04b3b2b-native.log`。测试未启动应用、网络、键鼠或剪贴板。

Windows 交叉检查实际尝试后退出码 101，失败发生在项目代码前：`x86_64-pc-windows-msvc` 标准库未安装（E0463）。Windows build 保持 pending_environment；该失败不阻塞 T08 等可在 Mac 验证的实现，也不计为产品缺陷通过。

提交后以实现 SHA `409af83e70ca87a594d97d7008218220e601728d` 复跑整套检查，8 个步骤均为退出码 0，日志为本地 `.local-evidence/t04b3b2b-postcommit.log`。

## T08.a motion protocol 增量

新增独立 V2 motion datagram 编解码，1024-byte 分配前上限及 session/sequence/reliable-floor 字段。3 项测试覆盖正常往返、零序列、超限、错误 magic 和 major。`t08a-native` 完整检查通过 181 个 Rust 库测试及其余 7 个步骤，日志为本地 `.local-evidence/t08a-native.log`。latest-wins transport 与接收应用尚未完成，T08 保持 in_progress。

提交后以实现 SHA `d20467520fe33c81c5851f2fa22946cdd60845d3` 复跑整套检查，8 个步骤退出码均为 0，日志为本地 `.local-evidence/t08a-postcommit.log`。

## T08.b latest-wins transport 增量

新增每个 motion handle 独立的单帧槽，并以原子 scheduled 位将同一槽的待处理 QUIC 命令限制为一个。生产者连续覆盖绝对位置，worker 每轮最多发送一次后重新检查，关闭时清空未发送位置并拒绝重新调度；没有为鼠标移动逐帧创建 task。

单元测试在消费 transport 命令前连续提交 100 帧，证明队列只有一个 flush 且保留 sequence 100 的最终坐标，同时覆盖关闭清理。`t08b-native` 完整检查退出码 0：182 个 Rust 库测试、9 项隔离检查、前端 lint/build、Mac cargo check 和核心格式检查通过，日志为本地 `.local-evidence/t08b-native.log`。接收端尚未应用 motion，Windows 生产发送也尚未接入，因此 A22–A27/A29 仍只具备部分证据，不标记端到端通过。

提交后以实现 SHA `cfa1b995f823e46f5187060ae45c5fd094b877db` 复跑相同 8 个步骤，全部退出码为 0，日志为本地 `.local-evidence/t08b-postcommit.log`。

## T08.c motion 顺序、发送和调度增量

接收端 3 项新增测试覆盖 future reliable dependency 的 latest-only 缓存、可靠点击位置先于后续拖动、错误连接/End 后 motion 拒绝；pressed-state 测试验证拖动按钮及释放位置随 motion 更新。控制端测试覆盖初始位置、可靠依赖、按钮 sequence 强制重写、首帧前拒绝 pointer critical event，以及 motion 失败恢复本地。

Windows 生产路径在激活后发送首帧，并在原物理 delta 累计路径上发送绝对位置。A25 测试累计 1000 次相对位移，结合 motion-slot 的 100 帧最终位置测试证明中间覆盖不缩短总位移。Windows cfg 仍缺 target 工具链，代码尚未在 Windows 编译。

A26 使用真实本机 QUIC 回环：两个 bulk handler 同时阻塞时，持久可靠 input 仍在断言期限内到达。bulk handler 移到有界 blocking pool，接收端全局最多 8 个活跃 stream task；预算耗尽/恢复、帧尺寸上限和未认证连接拒绝分别由现有测试覆盖。这里不宣称真实网络 QoS。

最终实现 SHA `5d8bdb104139738bb7f7c9cfece3391019fe41d1` 的 `t08-postcommit` 检查退出码 0：191 个 Rust 库测试、9 项隔离检查、前端 lint/build、Mac cargo check 和核心格式检查通过。A22–A27 更新为 pass。A29 仍为 not_run：协议尚未携带/校验 display_id 和 layout_revision，T08 父任务保持 in_progress。

## T09 控制热键增量

A05 以共享原子去重器覆盖系统入口、hook 入口和自动重复；释放前不会二次派发。A06 在接收端先注入 Ctrl/Alt down，再处理 EndSession，断言两个修饰键均按账本逆序提交真实 up。另有纯测试覆盖返回与紧急返回精确修饰键区分、当前选中远端显示器解析、客户端不注册及快捷键冲突拒绝。

实现 SHA `9aed9a81aa928ad67ba2bf298d49b63027b44dd6` 的 `t09c-postcommit` 检查退出码 0：198 个 Rust 库测试、9 项隔离检查、前端 lint/build、Mac cargo check 和相关格式检查通过。A28 仍为 not_run，已有源码审查部分证据；Windows hook 快速路径的锁与 FFI 边界在 T10 完成。W01 保持 pending_environment，未执行 Windows 或 LOL 实机。

设置界面 SHA `d445e9b9757077500f896534b94a1104f3745edd` 的前端 lint/build 退出码 0，三个录入项均使用已有录制组件和后端冲突错误通道；证据为 `.local-evidence/t09d-postcommit.log`。未启动应用或操作系统快捷键。

## T10.a 本地游戏快速路径

A08 运行 50,000 次原子游戏模式判断，重路径调用计数为 0；源码隔离检查确认 Windows 鼠标/键盘 hook 在 context、布局、网络路径前直接放行，并验证 panic 不越过 FFI。实现 SHA `2dfda6453a7dcbf725ddf5584e7d01e462e1e7c7` 的提交后检查退出码 0：200 个 Rust 库测试、10 项隔离检查、前端 lint/build、Mac cargo check 和相关格式检查通过。

A08 标记 pass。T10.b 以 1024 项有界 try-send 队列将桌面/远程状态、光标和发送协调移至捕获线程；满队列测试立即失败，源码白名单确认 inner hook 无锁等待、布局、光标、日志或网络调用。实现 SHA `15fd004b5f471f7f83cc3b86fffb1a448e74dfaf` 的提交后检查退出码 0：202 个 Rust 库测试、10 项隔离检查、前端 lint/build、Mac cargo check及完整 input.rs 语法解析通过。A28 标记 pass。未测 Windows 物理回报率，也未运行 LOL。

## T11 Windows 焦点交接

A09 的路由测试使用拒绝型 FakeFocus，证明单次尝试后恢复本地且不会激活；新增隔离检查核对生产 FocusPort 调用 Windows 原生适配器，适配器只有一次前台交接调用，没有循环、等待、键盘合成或游戏注入，并提供中文 Alt+Tab 回退提示。A03 的既有路由测试证明取消后的 Ready 和 CommitAck 均不能激活旧请求。两项更新为 pass。

实现 SHA `19191db65aaac1743823538df3d4c4ad066e6c5b` 的提交后检查退出码 0：202 个 Rust 库测试、11 项隔离检查、前端 lint/build、Mac cargo check 和干净工作树通过，证据为 `.local-evidence/t11-postcommit.log`。Mac 应用未启动，Windows 编译缺环境，Windows 焦点/物理输入未实测；L01–L03 保持 optional_not_run。

## T08.d 显示布局门控

A29 自动化覆盖逻辑坐标到负原点/缩放原生范围的映射、边界检查、旧 layout_revision 和未知范围拒绝、可靠点击拒绝前不消耗序号，以及布局改变后的会话关闭与按键释放。生产路径每 500 ms 只读刷新显示器；变化后更新广播布局并失败关闭，motion 与按钮/滚轮均在注入前映射。

实现 SHA `3dfca829cc74e7d4ff4c47ce2fb826ecd10af2ee` 的提交后检查退出码 0：203 个 Rust 库测试、12 项隔离检查、前端 lint/build、Mac cargo check 和干净工作树通过，证据为 `.local-evidence/t08d-postcommit.log`。A29 标记 pass，T08 自动化闭环。Mac 应用/显示器拔插、Windows 编译和双机坐标体验未运行。

## T12 Mac 键位映射

A30 的表驱动测试覆盖左右 Ctrl/Win/Alt 的默认保留与显式 Ctrl/Command 互换，并复用既有测试核对左右 Shift、OEM 标点、数字区、功能键和 Caps Lock。A31 的接收会话测试分别提交 Ctrl+C 与 Win+C/V，证明默认不会把终端中断键暗中改成 Command，同时 Windows 键序列保持 Mac Command 语义。

接收端在 pressed-state 登记前映射，测试在右 Ctrl 按下后改变配置，仍按 key-down 时冻结的右 Command 目标释放。Caps Lock 的旧 Ctrl+Space 合成路径已移除，生产隔离检查确认 receiver-only 分支只报告 injector 状态且不创建本地 capture。权限拒绝逻辑已由 FakeInjector 和生产路径静态检查覆盖，但 M04/M06 需要真实辅助功能授权和固定测试应用，本轮未运行。

实现 SHA `7bf95bb7ebf89c93a04dda839e3423e681935c9d` 的 `.local-evidence/t12-postcommit.log` 退出码 0：205 个 Rust 库测试、13 项隔离检查、前端 lint/build、Mac cargo check 和核心格式检查通过。A19、A20、A30、A31 为 pass；Mac 应用/真实按键/IME 为 not_run，Windows 原生编译为 pending_environment。

## T19 Mac 安全回环

M05 在本机创建两个独立临时证书目录和真实 QUIC endpoint，经逐设备证书信任完成 V2 control、input 与 motion。接收端由 `FakeInjector` 记录事件；静态隔离测试确认整个模块只在 test 配置编译，且不引用原生捕获或注入 API。

同一回环把后发的较小 motion sequence 判为 stale；第一代会话结束后，生产 input handler 拒绝其旧帧并通过断流回调释放第二代按住的键，由此完成 A16 的剩余生产接线证据。第三代会话以可控时间触发 3 秒租约并验证 key-up。既有 A10、A18、A22、A24、A26 继续由各自定向测试覆盖。

最终实现 SHA `c184091c6762118c56faf8494cc3ee6215cb0841` 的 `.local-evidence/t19-postcommit.log` 退出码 0：206 个 Rust 库测试、14 项隔离检查、前端 lint/build、Mac cargo check 和相关格式检查通过。M05、A16 更新为 pass；没有启动应用或触碰真实桌面/剪贴板。

## T13 Windows 原生剪贴板

A33 新增纯策略测试区分 unchanged/empty/busy/unsupported/error，并证明 busy 重试有硬上限、格式 sequence 变化时旧文本被丢弃。生产同步循环只接受 Content；既有接收测试证明系统写失败不会确认或清空旧内容。隔离检查核对 `AddClipboardFormatListener`、`WM_CLIPBOARDUPDATE`、sequence 读取、Remove/Destroy/上下文释放和消息循环退出路径，同时禁止 SYSTEM/跨会话读取。

实现 SHA `3a5f7dc5f0e010c6881b04b1259986ec2105ac13` 的 `.local-evidence/t13-postcommit.log` 退出码 0：207 个 Rust 库测试、15 项隔离检查、前端 lint/build、Mac cargo check 和 clipboard 格式检查通过。A33 更新为 pass；A38 留待 T16，W01 为 pending_environment。Windows 条件代码尚未在 Windows 标准库或原生主机编译，未宣称运行通过。

## T14 Mac 进程内剪贴板

Mac 文本读写已改为 arboard，生产源码不再包含 pbpaste/pbcopy。`MacClipboardWatcher` 先比较 NSPasteboard changeCount，目标不存在时只更新基线，有变化才读取。测试覆盖相同计数去重、计数推进以及中文/emoji/换行/长 UTF-8 的内容模型完整性；隔离检查核对 watcher 位于读取之前。

实现 SHA `e2a3ccdb47ad1680ccef8f71dd182c2706ca614d` 的 `.local-evidence/t14-postcommit.log` 退出码 0：208 个 Rust 库测试、16 项隔离检查、前端 lint/build、Mac cargo check 和格式检查通过。A37 更新为 pass；A32 等待 T15 网络操作测试，M03 真实剪贴板保持 not_run。

## T15 双向文本剪贴板

A32 覆盖中文、emoji、CRLF/LF 和长 UTF-8 的 V2 MessagePack 往返，以及超限返回字节数且不截断。A34 用系统版本与摘要证明远端写回只抑制自身回声，随后不同文本立即形成新操作。A35 在双端并发、乱序和重复输入下按 Lamport 与稳定 peer/boot/sequence 排序收敛。A36 证明新引擎只记录启动基线，除非出现新系统版本或用户点击手动重发，否则不发送旧内容。A33 继续覆盖 busy/error/empty/unsupported，并新增 V2 系统写失败不提交、可重试的断言。

生产接线要求出站目标存在于信任表且角色匹配；入站要求 TLS 认证 peer、方向角色和操作来源一致。真实 QUIC loopback 改用 `trusted_bulk_peer`，错误角色无法构造 endpoint。设置页提供文本阈值和手动重发，超限事件不含正文或摘要。

实现 SHA `112539cd9a0e20851fc55c03df93d0b9c89ded78` 的提交后检查退出码 0：216 个 Rust 库测试、17 项隔离检查、前端 lint/build 和 Mac cargo check 通过。M03 未运行；Windows 构建 `pending_environment`；Windows/LOL 实机 `optional_not_run`。图片仍关闭在 V2 文本路径之外，资源预算由 T16 处理。

## T16 图片与 bulk 资源预算

A38 覆盖图片尺寸乘法、RGBA 长度、base64 编码长度、摘要、默认关闭、游戏模式暂停和认证接收写入门控。图片原始 RGBA 上限为 32 MiB，MessagePack bulk 帧上限为 48 MiB；本机超限或格式不符时发送不发生，并发出不含内容的可见提示。文本仍受独立的 1.5 MiB 后端上限约束。

QUIC 通用 bulk 发送按实际载荷预留字节，接收在读取完整 stream 前预留 124 MiB；生产端编码也从同一个 128 MiB 全局预算预留工作内存。RAII 测试连续 100 次取得和释放近上限预算并回到零，真实回环继续证明 bulk 阻塞时可靠 input 可推进。停止或网络失败由既有 5 秒发送超时有界结束；本轮没有宣称在途 stream 可瞬时取消。

实现 SHA `f7e5a07f1e584fcc44ec50f1f376f3a29e8ba15d` 的提交后检查退出码 0：220 个 Rust 库测试、18 项隔离检查、前端 lint/build 和 Mac cargo check 通过。Mac 应用和真实剪贴板未运行；Windows 构建为 `pending_environment`，Windows/LOL 实机为 `optional_not_run`。
