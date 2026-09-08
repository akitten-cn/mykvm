# T06.c · 认证 QUIC reliable input 持久流

状态：done。传输和会话门控已实现；`AppRuntime` 到路由及输入账本的接线属于 T04.b/T07。

新增 `MKI2` stream 用途前缀和独立关键输入帧。按键、按钮和滚轮使用可靠有序流；按钮与滚轮携带自己的坐标和 motion sequence，避免点击依赖可能延迟的移动 datagram。帧包含完整 V2 `SessionId` 和严格递增序列，使用 4 字节大端长度前缀，单帧最多 4 KiB。解析器逐块输出完整帧，不等待流 EOF；零序列、空键码、无效 motion sequence、零滚轮量、截断和超限长度都会失败关闭。

input stream 只接受 T05 产生且角色为 Controller 的 `AuthenticatedPeer`。每个 QUIC 连接最多一条，全进程最多 8 条；出站目标只能从持久信任表按 receiver 身份构造。发送端同时限制 256 帧和 256 KiB，在任一预算耗尽时明确返回错误，已排队缓冲在发送或丢弃后归还字节预算。

`InputSessionGate` 在关键帧进入注入层前核对本进程 receiver boot、完整 session 和下一序列。结束的 session 不能重新激活；旧 boot、乱序、重复帧和序列空间耗尽均失败关闭。真实 Mac 本机回环测试在同一未关闭的 input stream 上依次发送并接收两帧，同时核对 TLS 身份。另有单元测试覆盖增量分帧、队列双重边界、旧 boot 和终止后输入。

本阶段不调用真实桌面、剪贴板或系统注入。`AppRuntime` 暂时使用拒绝所有帧的空 handler，因此当前分支仍不可作为实际远控工具。T04.b/T07 必须把 control 会话、input gate、FakeInjector 和可靠释放账本接通，之后才能完成 A16/A17/A22 的端到端验收。
