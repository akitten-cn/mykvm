# T04.b2 · AppRuntime 与原生接收端接线

状态：done。接收端仍由编译期安全门禁停用；T04.b3 控制端路由接线和 T07 可靠释放未完成。

`AppRuntime` 现在为每次 QUIC transport 生命周期生成 receiver boot id，并共享一个 `ReceiverSessionRuntime<NativeInjector>` 给 control/input handler。handler 在角色模式开关关闭时拒绝 control 并关闭 input；成功提交才增加输入计数。旧 LAN discovery/input/clipboard 入口继续禁用，V2 QUIC endpoint 可独立启动，不会广播旧 UDP discovery。

普通用户 `NativeInjector` 在 Ready 和每次事件提交前读取 macOS Accessibility 和 Secure Keyboard Input 状态，不请求授权或修改系统设置。Windows 分支只复用普通用户注入路径，不启用 helper/SYSTEM 服务。此任务仅做库编译和 FakeInjector 回归，没有启动应用、监听真实会话或操作桌面。

源码核验发现主进程现有 ReleaseAll 仍为空操作。为避免开发中间态造成粘键，`fork_policy::V2_NATIVE_RECEIVER_ENABLED` 固定为 false；因此当前构建即使运行也拒绝实际 V2 输入。T07 必须实现按会话记录与真实释放并通过故障测试，审查后才能把该门禁改为 true。
