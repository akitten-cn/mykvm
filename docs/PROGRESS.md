# MyKVM Local 执行进度

更新：2026-09-08。唯一可变任务入口是 `docs/handoff/taskboard.json`；工作区外层 `handoff/` 保留交接包原件。任务中的 approved 表示主线程代码审查完成，不表示用户批准安装或发布。

本轮完成用户要求的 T00/T01 实际核验，并推进了首批实现。完整产品尚未完成。

|任务|实现状态|实际范围|
|---|---|---|
|T00/T01|完成|工作区、交接资料、真实源码与工具链核验；基线测试；已有 fmt/clippy 失败留档|
|T02|完成|独立身份；禁用上游更新、特权 helper、安装和运行时防火墙修改路径|
|T03|完成|假平台端口及 5 项安全测试；完整协议回环未完成|
|T04.a|完成|纯路由状态机、取消代次、独立原子紧急返回通道，16 项测试|
|T04.b|完成|接收端、控制端 Router、认证 QUIC 客户端及 Windows hook/热键/关键事件生产路径已接线；控制端总门禁等待 T08/T09/T11 后开启|
|T04.b3.a|完成|控制端 Hello/Prepare、Ready 校验、随机 SessionId、显式 Commit/CommitAck、Ping/Pong 进度和 End 纯逻辑|
|T04.b3.b1|完成|Router 与控制端握手适配；Ready 后仍等待按键释放和焦点门槛，提前/晚 ACK 失败关闭，返回先恢复本地；QUIC/Windows 生产接线仍未完成|
|T04.b3.b2a|完成|有界控制端连接客户端及真实 QUIC transport adapter；入站回调只投递队列，发送/溢出故障先恢复本地；尚未由 Windows 捕获线程实例化|
|T04.b3.b2b|完成|Windows 捕获线程实例化 V2 客户端；热键/贴边准备、可靠键/按钮/滚轮、Ping 与返回已接线；Windows 构建缺环境，控制端门禁仍关闭|
|T05.a|完成|核验信任漏洞，关闭旧 LAN 入口|
|T05.b|完成|逐设备持久证书信任、双方 TLS 证书出示、连接代次与角色绑定；配对声明绑定实际连接证书|
|T06.a|完成|V2 有界帧、版本/能力校验、随机 boot/session 标识及接收握手纯逻辑，7 项测试|
|T06.b|完成|认证 QUIC control 持久流、有界队列、单连接唯一流与速率限制；真实回环不等 EOF|
|T06.c|完成|仅认证控制端可开的 V2 可靠 input 持久流、双重有界队列、严格会话序列门控和真实 QUIC 回环顺序测试；运行时接线属于 T04.b/T07|
|T07.a|完成|按扫描码冻结目标键映射；自动重复去重、多源同目标引用语义、按钮位置账本、正常 End/故障逐项松开及失败重试，FakeInjector 验证|
|T07.b|完成|input stream 关闭立即结束并释放；3 秒活动租约、Ping 刷新、释放失败状态和 AppRuntime 可见错误；V2 原生接收门禁已打开|
|T08.a|完成|独立 MKM2 motion datagram、1024-byte 上限、会话/单调序列和 required reliable floor；latest-wins 调度与接收应用继续实现|
|T21|脚本完成|原生 Mac/Windows 检查工作流；本机 Mac 实际运行，Windows runner 尚未运行|

当前 T08.a 工作树基于 SHA `f0ba967efda2280fee135ddad10cffc0c2fb80cf`；提交前完整检查通过：181 个 Rust 库测试、9 项隔离检查、前端 lint/build、Mac cargo check 和核心格式检查，证据日志为本地 `.local-evidence/t08a-native.log`。提交后仍需以新 SHA 复核。全仓严格 fmt/clippy 的既有问题详见 TEST_REPORT.md。

平台证据独立记录：Mac 库编译及前端构建通过；Mac 应用打包和运行未执行；Windows 构建 pending_environment；Windows/LOL 实机 optional_not_run。没有安装包、公开 fork、推送或发布。

下一原子任务为 T08.b，实现每会话单槽 latest-wins motion 调度及关闭取消；随后 T08.c 接收端应用顺序和 Windows 发送接线。完成 T08 后继续 T09–T11，再审查打开控制端总门禁。旧 LAN 开关保持关闭；剪贴板、完整中文设置、后台生命周期和预览打包仍需按任务表实现与验收。
