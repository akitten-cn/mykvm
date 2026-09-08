# T04.b3.a · 控制端 V2 握手与健康纯逻辑

状态：done。Router、QUIC handle 和 Windows 捕获接线属于 T04.b3.b。

`ControllerHandshake` 使用进程级随机 BootId 和本机持久 peer id。每次明确请求先生成 Hello/Prepare；只有 request id 匹配、receiver boot 非零且 input_ready 的 Ready 才生成随机 SessionId 和 Commit。CommitAck 必须完整匹配，错误或拒绝不会进入 Active。

Active 会话才允许 Ping 和 End。Ping 序列使用 checked arithmetic；Pong 必须匹配 session、对应已发送且未处理的序列，并且最高远端提交序列不能倒退。End 固定当前 session 后进入结束态；准备期可由上层 abort，不会伪造远端已激活。

2 项测试覆盖随机会话组成、错误 Ready/ACK、Ping/Pong 进度和 End 范围。测试不启动网络或捕获输入。当前 Windows 控制端仍未接入该核心，因此产品仍不能使用。
