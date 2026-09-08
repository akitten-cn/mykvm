# T08.b · latest-wins motion transport slot

状态：done。接收端顺序门控和 Windows motion 生产接线属于 T08.c。

`MotionHandle` 为每次打开的 Receiver 会话持有一个绝对位置槽。生产者先完成 `MotionFrame` 的有界编码，再覆盖槽中的旧位置；`scheduled` 原子位保证 QUIC worker 命令队列中同一槽至多有一个 `FlushMotion`。worker 每轮只取一帧并立即返回主循环，如果取帧期间出现更新则重新排一次，因此不会由单个高频鼠标来源独占 transport loop，也不会积压相对位移。

句柄只能为持久信任的 Receiver 角色创建。句柄丢弃时先标记关闭并清空尚未发送的帧；已经排队的 flush 会观察关闭状态并退出。实际 datagram 继续复用已有非阻塞 QUIC 连接和健康状态，不为每次移动创建 task。

单元测试连续写入 100 帧，在不消费 worker 命令的情况下验证只产生一个 flush、槽内可解码帧为 sequence 100 的最终绝对坐标，并验证关闭清理和拒绝重新调度。Mac arm64 完整检查通过 182 个 Rust 库测试、9 项隔离检查、前端 lint/build、Mac cargo check 和核心格式检查；没有启动应用、桌面输入或剪贴板。
