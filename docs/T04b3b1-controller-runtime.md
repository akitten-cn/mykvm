# T04.b3.b1 · 控制端 Router 与协议适配器

状态：done。QUIC handle 和 Windows 捕获生产接线属于 T04.b3.b2。

`ControllerRuntime` 将纯 `Router` 和 `ControllerHandshake` 组合成单一控制端决策层。开始切换只产生 Hello/Prepare；收到匹配 Ready 后保存 request/session，但必须继续等待物理按键释放与 `FocusPort` 成功。只有 Router 产生 Commit 效果时才把握手推进到 AwaitCommitAck。匹配的 CommitAck 才激活捕获并允许产生严格递增的可靠输入帧。

错误 request/session、提前 CommitAck、晚 ACK 和本地原子应急覆盖都会失败关闭。返回本地先调用 `CapturePort::restore_local`，随后才产生 EndSession/CloseInput 动作；网络发送失败不能挡住本地恢复。EndSession 保留用户、应急、超时、按键未释放、焦点失败、捕获失败、网络失败和队列满原因。

5 项 FakeCapture/FakeFocus 测试覆盖门槛前不 Commit、正确激活与输入序列、ACK 前应急取消、提前 ACK 和非法活动帧失败关闭，以及先恢复本地再交付 End 的顺序。整套原生检查通过 172 项 Rust 库测试。测试未启动应用、网络、真实捕获或剪贴板。

该适配器尚未由 Windows hook/捕获线程持有，也没有创建生产 control/input handle，因此不能据此宣称 Windows 到 Mac 已可用。下一步是 T04.b3.b2，并需与 T08 的鼠标 motion 数据面衔接。
