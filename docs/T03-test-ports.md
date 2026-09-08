# T03 · Mac 可运行的假平台接口

新增 control_ports.rs：单调时钟、Capture/Focus/Injector/Transport 边界；所有 Fake 类型仅在 cfg(test) 构建。FakeInjector 保存 InputCommand，不调用输入系统 API；FakeTransport 只在内存中记录有界消息，不连接网络；假时钟直接推进，不 sleep。

5 个测试通过：权限拒绝/撤销不提交；失败提交不记作已应用；断网不影响本机恢复请求；传输先检查字节预算；时钟和平台失败可控。`submit_ready` 明确区分就绪与提交，尚未接管上游生产接收路径。

验证：`cargo test --locked --lib control_ports::`，退出码 0；`rustfmt --check` 该新文件通过；git diff --check 通过。日志 `.local-evidence/T03-ports.*`。

这是后续路由/协议测试的接口基础，不是 M05 完整 QUIC 回环或 A28 所有 Windows 钩子都已验证。原生 Windows 实现未被删改为假后端。

T04 拆成 T04.a（纯路由状态与原子应急通道）和 T04.b（连接 V2 握手、真实平台线程、热键/IPC/清理）。原因：生产接入需要 T05/T06 的连接与会话语义，不能用假授权先启用真实输入。T04 只有两部分均完成才能记 done；T07 等仍依赖整个 T04。
