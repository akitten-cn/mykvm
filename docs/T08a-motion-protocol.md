# T08.a · V2 mouse motion datagram 协议

状态：done。latest-wins transport slot 和接收调度属于 T08.b/T08.c。

新增 `MotionFrame`，包含完整 SessionId、从 1 开始的单调 motion sequence、`required_reliable_sequence` 和绝对 x/y。绝对坐标允许调度器安全覆盖尚未发送的旧位置；required reliable floor 防止位置越过必须先应用的键/按钮/滚轮事件。

Motion 使用独立 `MKM2` magic、V2 major 和 1024 byte 硬上限，以单个 QUIC datagram 为边界，不添加 stream 长度前缀。解码在反序列化前检查空包和尺寸，并校验 magic、major、session 及非零序列。

3 项测试覆盖往返、零序列、超限包、错误 magic 和错误 major。没有启动网络或桌面输入。按钮/滚轮原有非零 `motion_sequence` 校验继续保留。
