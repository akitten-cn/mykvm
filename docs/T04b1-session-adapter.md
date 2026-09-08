# T04.b1 · 认证接收会话适配器

状态：done。T04.b 整体仍在进行，生产 AppRuntime 和原生端口接线属于 T04.b2。

`ReceiverSessionRuntime` 把 T05 的 `AuthenticatedPeer`、T06 的 `ReceiverHandshake`/`InputSessionGate` 和 T03 的 `InjectorPort` 串在同一接收边界。Hello 声明的 peer id 必须等于 TLS 信任记录中的设备；后续 control 和 input 必须来自同一 peer、角色、信任版本、连接代次及远端地址。新连接不能在旧会话活动时重绑。

Prepare 在返回 Ready 前检查 injector readiness。首次 CommitAck 激活 input gate；重复 Commit 只重发 ACK，不重置输入序列。关键帧通过 gate 后转换为 `InputCommand` 并经 `submit_ready` 提交，Pong 返回最高已成功提交序列。End 关闭 gate 后请求 ReleaseAll；权限或提交失败会中止握手及 gate，并尽力请求 ReleaseAll。

6 项 FakeInjector 测试覆盖正常顺序、连接代次替换、重复 Commit、End 后迟到输入、注入失败和 Ready 前权限失败。测试没有启动 QUIC、捕获键鼠或访问真实桌面。当前释放仍是已有的通用 ReleaseAll；按会话冻结映射和精确按键账本由 T07 实现，所以不能据此宣布可靠释放已经完成。
