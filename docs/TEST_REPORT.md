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
