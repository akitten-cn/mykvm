# T04.b3.b2a · 有界控制端连接客户端

状态：done。Windows hook/捕获线程持有该客户端属于 T04.b3.b2b。

`ControllerClient` 在 Router/握手适配器外管理连接生命周期。生产 `QuicControllerTransport` 只接受信任表中角色为 Receiver 的 peer，复用同一 `ControlPeer` 打开 control 和 input stream。control 回调只向 64 帧有界同步队列投递，不在 QUIC 线程中调用平台捕获；队列断开或溢出视为故障。

客户端依次发送 Hello/Prepare，轮询入站 Ready/CommitAck，再执行 Router 动作。input 只能在匹配 ACK 后打开。control/input 队列失败、入站溢出和协议错误都会请求 Router 返回本地并丢弃两个 handle；EndSession 是尽力发送，本地恢复不等待网络。

3 项 FakeControllerTransport 测试覆盖 Ready 后仍等待按键释放、CommitAck 后才开 input、可靠帧序列、input 队列失败立即恢复本地和断开，以及重复 begin 不替换正在握手的认证连接。测试未创建网络连接或调用桌面。

这一层使用了真实 QUIC handle 类型，但尚未被 Windows 平台代码实例化，因此仍不能使用。Windows 条件编译和实机状态保持 pending_environment/optional_not_run。
