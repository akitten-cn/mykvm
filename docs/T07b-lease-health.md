# T07.b · 断线租约、健康与释放错误

状态：done。自动化与 Mac 库编译通过；Mac/Windows 实机仍未运行。

接收会话在 Commit 后建立 3000 ms 活动租约，成功关键输入和有效 Ping 都刷新时间。监视线程每 100 ms 使用单调时钟检查；它只持有接收器弱引用，transport 生命周期结束后自行退出。超时先结束 handshake 和 input gate，再从账本提交真实 up。进程被强杀时用户态代码无法运行，此边界不宣称可消除。

QUIC input 持久流现在有关闭回调。正常 EOF、截断/解码错误和 handler 拒绝都携带 TLS 认证 peer 通知会话；只有与当前完整连接绑定相等的会话会立即终止并释放。这样 control Ping 无法在 input stream 已损坏时继续维持可能粘键的会话。

会话保存 Active、highest applied sequence 和最后 fault。租约到期、输入拒绝、stream 关闭和释放失败通过 AppRuntime 的 inject status 可见；新 CommitAck 清除旧 fault。FakeClock、FakeInjector 和真实 QUIC loopback 覆盖 A18/A21，未操作桌面。

T07 完成后只打开认证 V2 receiver 编译期门禁；V1 LAN 仍永久关闭。当前还没有完成 Windows 控制端 Router/热键/捕获接线，因此整个产品仍不能投入使用。
