# T08.c · motion 顺序与生产接线

状态：done。T08 父任务继续等待 T08.d 的 display/layout revision 门控。

接收会话现在维护 `highest_applied_sequence`、`highest_motion_sequence` 和单个 `pending_motion`。依赖尚未应用可靠事件的位置只覆盖缓存；按钮/滚轮的可靠位置快照先建立 motion floor，事件成功后才刷新缓存位置。旧连接、旧 session、零序列和不高于 floor 的迟到 datagram 不会注入。motion 注入失败会结束会话并尝试释放账本；End、租约到期和 input stream 关闭都会清空 motion 状态。

控制端统一生成可靠序列与 motion 序列。每帧 motion 带当前可靠序列依赖；按钮/滚轮的调用方不能伪造 motion sequence，由 `ControllerRuntime` 写入最后成功排队的位置序列。QUIC adapter 同时持有可靠 input handle 和 latest-wins motion handle，任一发送失败都先请求 Windows 恢复本地再断开。

Windows hook 在 CommitAck 激活后先发送初始绝对位置，之后先累计每个物理相对 delta，再按既有节流只覆盖最新绝对位置。返回边界走 V2 EndSession/本地恢复，不降级到永久关闭的 V1 数据路径。此代码尚未在 Windows target 编译或实机运行，控制端总门禁保持 false。

调度补充了 8 个全局入站 stream 槽；同步 bulk handler 在有界 `spawn_blocking` 任务中执行，避免占住两个 QUIC async worker。真实本机 QUIC 回环同时阻塞两个 bulk handler 时，可靠 input 仍在 2 秒断言窗口内到达。另一测试证明槽位耗尽明确拒绝并在 permit 释放后恢复。

实现提交：

- `ca0729e0d2a583fd3981f874238fae3fff0064ce`：接收顺序与 pressed-state drag 账本
- `a6f30ef90cf8dc0d9cb2374b7fc4b7e1c5430b51`：认证 datagram 接入生产 ReceiverSessionRuntime
- `11cd70eb75e725736612bb19faf87a32611de33e`：控制端序列与 motion handle 生命周期
- `990773c3f04174fdba377e34b162f912d798fcf5`：Windows V2 motion 生产路径
- `6d7b7ab6239209ad3526b3fc6cd491a04921ff78`：入站预算和 bulk/input 公平性
- `5d8bdb104139738bb7f7c9cfece3391019fe41d1`：相对位移累计证据

最终提交后检查通过 191 个 Rust 库测试、9 项隔离检查、前端 lint/build、Mac cargo check 和核心格式检查；本地证据为 `.local-evidence/t08-postcommit.log`。没有启动桌面输入或剪贴板。
