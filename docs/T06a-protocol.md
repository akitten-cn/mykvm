# T06.a · V2 控制协议纯逻辑

状态：done。真实 QUIC 持久流接入属于 T06.b，T06 整体仍为 in_progress。

新增 `protocol_v2.rs`，固定主版本 2 和 `MKV2` 魔数。控制帧使用 4 字节大端长度前缀，单帧上限 16 KiB；解析器逐段读取前缀和载荷，在按网络声明分配前拒绝零长度及超限长度，并支持逐字节输入、粘包和 EOF 截断检测。未知主版本、未知枚举、缺必要能力、空或超长字段明确返回错误，不降级到 V1。

`BootId` 每次进程启动随机生成，`SessionId` 同时包含控制端 boot、接收端 boot 和每次会话的随机 nonce。接收握手只接受已认证控制端身份对应的 Hello；Prepare/Ready/Commit/CommitAck、Ping/Pong 和 EndSession 均校验请求、boot 和 session。结束状态不接受旧 Commit/Ping 复活；同一连接可用新请求建立新会话。

定向 7 项测试通过，覆盖 A13–A17 的纯协议部分。A13–A15 已达到自动化用例范围；A16/A17 仍需 T06.b/T07 接入真实输入会话后证明注入次数为零，因此只记录部分证据。测试不访问桌面、剪贴板或真实网络。

下一步 T06.b：为已认证 QUIC 连接增加独立小型 control 持久流，以 `ControlDecoder` 逐帧处理，不复用读取到 EOF 的 bulk stream；加入并发和频率边界，再接路由状态机。
