# MyKVM Local 执行进度

更新：2026-09-08。唯一可变任务入口是 `docs/handoff/taskboard.json`；工作区外层 `handoff/` 保留交接包原件。任务中的 approved 表示主线程代码审查完成，不表示用户批准安装或发布。

本轮完成用户要求的 T00/T01 实际核验，并推进了首批实现。完整产品尚未完成。

|任务|实现状态|实际范围|
|---|---|---|
|T00/T01|完成|工作区、交接资料、真实源码与工具链核验；基线测试；已有 fmt/clippy 失败留档|
|T02|完成|独立身份；禁用上游更新、特权 helper、安装和运行时防火墙修改路径|
|T03|完成|假平台端口及 5 项安全测试；完整协议回环未完成|
|T04.a|完成|纯路由状态机、取消代次、独立原子紧急返回通道，16 项测试|
|T04.b|完成|接收端、控制端 Router、认证 QUIC 客户端及 Windows hook/热键/关键事件生产路径已接线；T08–T11 审查后控制端 V2 门禁已开启|
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
|T08.b|完成|每次 motion handle 单一绝对位置槽、至多一个待刷新命令、关闭取消和 Receiver 角色限制；接收应用继续实现|
|T08.c|完成|接收 reliable floor/latest pending、点击与拖动顺序、认证 datagram、控制端序列、Windows V2 发送、bulk/input 公平性和入站并发预算|
|T08.d|完成|Prepare/Ready 与 motion 绑定显示器和布局版本；逻辑坐标映射到负原点/缩放后的原生范围，500 ms 显示变化检测失败关闭并释放|
|T09|完成|三个控制动作、全局注册/冲突回滚、中英文设置、系统与 hook 去重、共享本地门控、物理键释放判定和热键前缀释放；Windows 编译为独立 pending_environment|
|T10.a|完成|持久化手动游戏模式、中英文开关、hook 首指令原子放行、panic FFI 边界和 context try-lock；桌面/远程重路径迁移留给 T10.b|
|T10.b|完成|1024 项有界 hook 事件队列；回调只做缓存/原子判断、事件复制和 try_send，慢路径移至捕获线程；满队列失败开放|
|T11|完成|Windows 屏幕外原生焦点窗口；每次请求单次前台交接，失败保留本地并给出 Alt+Tab 中文提示，返回时单次恢复原窗口|
|T21|脚本完成|原生 Mac/Windows 检查工作流；本机 Mac 实际运行，Windows runner 尚未运行|

T08.c 最后实现 SHA：`5d8bdb104139738bb7f7c9cfece3391019fe41d1`。提交后完整检查通过：191 个 Rust 库测试、9 项隔离检查、前端 lint/build、Mac cargo check 和核心格式检查，证据日志为本地 `.local-evidence/t08-postcommit.log`。全仓严格 fmt/clippy 的既有问题详见 TEST_REPORT.md。

平台证据独立记录：Mac 库编译及前端构建通过；Mac 应用打包和运行未执行；Windows 构建 pending_environment；Windows/LOL 实机 optional_not_run。没有安装包、公开 fork、推送或发布。

T09 当前实现 SHA：`9aed9a81aa928ad67ba2bf298d49b63027b44dd6`。提交后检查通过：198 个 Rust 库测试、9 项隔离检查、前端 lint/build、Mac cargo check 和相关格式检查，证据日志为本地 `.local-evidence/t09c-postcommit.log`。

设置界面 SHA `d445e9b9757077500f896534b94a1104f3745edd` 已补三个控制热键的中英文录入和说明，前端检查日志为 `.local-evidence/t09d-postcommit.log`。T10.a SHA `2dfda6453a7dcbf725ddf5584e7d01e462e1e7c7` 完成游戏模式最短放行；T10.b SHA `15fd004b5f471f7f83cc3b86fffb1a448e74dfaf` 将其余重路径移出 hook，检查日志为 `.local-evidence/t10b-postcommit.log`。T11 SHA `19191db65aaac1743823538df3d4c4ad066e6c5b` 完成 Windows 单次焦点交接与失败提示，检查日志为 `.local-evidence/t11-postcommit.log`。T08.d SHA `3dfca829cc74e7d4ff4c47ce2fb826ecd10af2ee` 完成显示布局版本和坐标门控。下一原子任务审查控制端总门禁；Windows 构建和双机试用状态仍独立保留。

控制端门禁审查 SHA `63154dcbfc4ff68ccb0d1a911c1652b0ba54fde9` 已开启认证 V2 Windows 控制路径，旧 LAN 和特权路径继续关闭。下一原子任务为 T12 Mac 键位映射完善；Windows 构建和双机试用状态仍独立保留。
