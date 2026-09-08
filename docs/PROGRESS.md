# MyKVM Local 执行进度

更新：2026-09-08。唯一可变任务入口是 `docs/handoff/taskboard.json`；工作区外层 `handoff/` 保留交接包原件。任务中的 approved 表示主线程代码审查完成，不表示用户批准安装或发布。

本轮完成用户要求的 T00/T01 实际核验，并推进了首批实现。完整产品尚未完成。

|任务|实现状态|实际范围|
|---|---|---|
|T00/T01|完成|工作区、交接资料、真实源码与工具链核验；基线测试；已有 fmt/clippy 失败留档|
|T02|完成|独立身份；禁用上游更新、特权 helper、安装和运行时防火墙修改路径|
|T03|完成|假平台端口及 5 项安全测试；完整协议回环未完成|
|T04.a|完成|纯路由状态机、取消代次、独立原子紧急返回通道，16 项测试|
|T04.b|进行中|T04.b1 会话适配和 T04.b2 AppRuntime/普通用户原生接收端接线完成；T07 后认证接收门禁已打开；T04.b3 控制端 Router/热键/捕获仍在实现|
|T04.b3.a|完成|控制端 Hello/Prepare、Ready 校验、随机 SessionId、显式 Commit/CommitAck、Ping/Pong 进度和 End 纯逻辑|
|T04.b3.b1|完成|Router 与控制端握手适配；Ready 后仍等待按键释放和焦点门槛，提前/晚 ACK 失败关闭，返回先恢复本地；QUIC/Windows 生产接线仍未完成|
|T05.a|完成|核验信任漏洞，关闭旧 LAN 入口|
|T05.b|完成|逐设备持久证书信任、双方 TLS 证书出示、连接代次与角色绑定；配对声明绑定实际连接证书|
|T06.a|完成|V2 有界帧、版本/能力校验、随机 boot/session 标识及接收握手纯逻辑，7 项测试|
|T06.b|完成|认证 QUIC control 持久流、有界队列、单连接唯一流与速率限制；真实回环不等 EOF|
|T06.c|完成|仅认证控制端可开的 V2 可靠 input 持久流、双重有界队列、严格会话序列门控和真实 QUIC 回环顺序测试；运行时接线属于 T04.b/T07|
|T07.a|完成|按扫描码冻结目标键映射；自动重复去重、多源同目标引用语义、按钮位置账本、正常 End/故障逐项松开及失败重试，FakeInjector 验证|
|T07.b|完成|input stream 关闭立即结束并释放；3 秒活动租约、Ping 刷新、释放失败状态和 AppRuntime 可见错误；V2 原生接收门禁已打开|
|T21|脚本完成|原生 Mac/Windows 检查工作流；本机 Mac 实际运行，Windows runner 尚未运行|

T04.b3.b1 实现 SHA：`47171b1812e1ea9af6ea70896d9ee9ba30437231`。提交后完整检查通过：172 个 Rust 库测试、8 项隔离检查、前端 lint/build、Mac cargo check 和新增核心格式检查；证据日志为本地 `.local-evidence/t04b3b1-postcommit.log`。全仓严格 fmt/clippy 的既有问题详见 TEST_REPORT.md。

平台证据独立记录：Mac 库编译及前端构建通过；Mac 应用打包和运行未执行；Windows 构建 pending_environment；Windows/LOL 实机 optional_not_run。没有安装包、公开 fork、推送或发布。

下一原子任务为 T04.b3.b2，把已审查的适配器接到 QUIC control/input handle，并改造 Windows 捕获发送路径。旧 LAN 开关保持关闭；T08 鼠标 motion、剪贴板、完整中文设置、后台生命周期和预览打包仍需按任务表实现与验收。
