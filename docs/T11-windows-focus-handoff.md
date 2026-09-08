# T11 Windows 保守焦点交接

实现 SHA `19191db65aaac1743823538df3d4c4ad066e6c5b` 在 Windows 控制端捕获线程创建一个 1×1、屏幕外、无任务栏入口的原生工具窗口。一次“控制 Mac”请求在物理按键全部释放后只调用一次 `SetForegroundWindow`；成功并收到 CommitAck 后才隐藏本地光标并激活远端输入。Mac 接收端不切换当前应用，也没有 WebView、画面传输、驱动或游戏注入。

Windows 拒绝前台切换、当前不在默认输入桌面或焦点结果不匹配时，请求立即失败关闭并保留本地控制。运行状态显示中文提示，要求用户先按 Alt+Tab 离开游戏后重试；代码中没有重试循环、合成 Alt+Tab 或反作弊规避。返回 Windows 时仅尝试一次恢复交接前的有效窗口，停止捕获及钩子安装失败路径会销毁工具窗口并清理状态。

A09 的 FakeFocus 路由测试证明焦点拒绝只调用一次且不会激活捕获；源码隔离测试确认生产 `FocusPort` 接到原生适配器，焦点函数没有循环、等待或输入合成，并包含 Alt+Tab 失败提示。A03 的路由测试证明取消后的 Ready/CommitAck 不能重新激活。提交后 `.local-evidence/t11-postcommit.log` 记录 202 个 Rust 库测试、11 项隔离检查、前端 lint/build、Mac cargo check 和干净工作树均退出 0。

本机缺少 Windows Rust 标准库，Windows 条件编译仍为 `pending_environment`。Windows 焦点行为、物理键鼠、LOL 客户端和对局均未实测；L01–L03 保持 `optional_not_run`，不作为后续开发或打包的硬门槛。控制端总门禁仍为 false，需完成 T08.d 并整体审查后才可生成可试用版本。
