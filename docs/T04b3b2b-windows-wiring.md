# T04.b3.b2b · Windows 捕获线程生产接线

状态：实现和主线程审查完成；Windows 原生编译为 pending_environment。运行门禁保持关闭，待 T08/T09/T11。

`AppRuntime::start_input` 已为 server/control 方向提供独立 V2 启动分支。Windows 捕获线程构造 `ControllerClient<QuicControllerTransport>`，热键或贴边只开始认证 Hello/Prepare，准备期不隐藏光标、不吞本地事件。QUIC control 回调由客户端的 64 帧有界队列送回捕获线程轮询，平台状态不会在网络线程中修改。

匹配 ACK 后的生产分支可把 Windows `KBDLLHOOKSTRUCT` 的 VK、scan code、extended 和方向转换为可靠 Key；左/右/中/前进/后退按钮及滚轮转换为带位置和 motion sequence 的可靠帧。输入/control 队列故障、协议错误、安全桌面、显式返回和停止捕获都会先恢复本地光标与捕获状态，再尽力 End 并丢弃 handle。活动会话每秒发送有界 control Ping，避免空闲会话无故越过接收端 3 秒租约。

本阶段刻意保留三道关闭条件：`V2_NATIVE_CONTROLLER_ENABLED=false`；物理释放采样固定返回 false，交给 T09；`FocusPort` 固定 unavailable，交给 T11。V2 活动态若收到 mouse motion 会立即返回本地，绝不降级到 V1，待 T08 提供认证 motion 数据面后替换。由此当前代码不能接管用户 Windows 输入。

Mac 非交互检查通过 178 项 Rust 库测试和 9 项隔离检查；新增纯测试验证 Windows scan/extended 转换、五键映射及 server/control 模式选择。未启动应用或访问桌面。

尝试 `cargo check --locked --lib --target x86_64-pc-windows-msvc`，在项目代码编译前以 E0463 退出，因为本机 Rust sysroot 仅安装 `aarch64-apple-darwin`。没有擅自安装目标或修改全局工具链。此结果不能算 Windows 编译；真实 Windows CI 和运行仍待环境。
