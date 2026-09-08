# T06.b · 认证 QUIC control 持久流

状态：done。T06 整体仍为 in_progress，可靠 input 持久流属于 T06.c。

QUIC 双向 stream 现在先读取 4 字节用途前缀。`MKC2` 进入 V2 control 路径，其余内容按原 bulk 路径读取并保留前 4 字节，因此已有配对、剪贴板和文件消息不会因分流丢数据。control stream 使用 `ControlDecoder` 按 4096 字节小块持续解析，每一帧立即交给 handler，不等待 EOF；半帧 EOF、非法长度、解码错误和超频会终止该 control stream。

control 入口只接受 T05 生成的 `AuthenticatedPeer`。每个 QUIC 连接最多一条 control stream，全进程最多 8 条；每个出站 handle 使用 64 帧有界队列，满时返回错误；入站速率上限为 128 帧/秒。没有使用 0-RTT。出站 `ControlPeer` 只能由持久信任表按 peer_id 和角色构造，证书不从发现广播参数传入。

真实 Mac 本机回环启动两个 QUIC endpoint，在同一未关闭的 control stream 上完成 Hello、Prepare/Ready、Commit/CommitAck。handler 同时核对来自 TLS 连接的设备身份和角色。另有测试验证队列溢出和速率超限会失败。完整库回归仍覆盖原 bulk stream，测试未访问桌面或剪贴板。

当前 `AppRuntime` 传入空 control handler，不会激活输入或路由。下一步 T06.c 实现独立可靠 input 帧和持久流；T04.b 再把 control handler、路由状态机和平台端口接入实际运行时。
