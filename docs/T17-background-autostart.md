# T17 后台、菜单栏与普通用户自启

实现提交：`e838ec27a5c863ad234b136da186ba2cdc1e1130`。

应用配置不再预建窗口。Rust `AppRuntime`、网络、输入和剪贴板先初始化；普通启动或菜单栏/第二实例请求时才创建设置 WebView。关闭设置销毁 WebView，隐式最后窗口退出被阻止，后台继续运行。前端事件监听和定时器在窗口销毁时清理。

macOS/Unix 使用用户临时目录中的 0600 Unix socket 作为单实例所有权与激活通道。第二进程在进入 Tauri 和捕获路径之前退出，只通知第一实例打开设置。Windows 保留已有的独立命名 mutex/event 与 release 无控制台属性。

登录自启默认不自动打开，也不再由客户端首次加载隐式启用。用户在设置中明确开启后，macOS 使用普通用户 LaunchAgent，并携带 `--mykvm-local-autostart` 让启动保持无窗口。实现没有安装 SYSTEM 服务、登录前输入、驱动或睡眠抑制。

提交后证据：

- `.local-evidence/t17-postcommit-rust.log`：222 passed，0 failed。
- `.local-evidence/t17-postcommit-isolation.log`：19 passed，0 failed。
- `.local-evidence/t17-postcommit-lint.log`：退出码 0。
- `.local-evidence/t17-postcommit-build.log`：退出码 0。
- `.local-evidence/t17-postcommit-mac-check.log`：退出码 0，保留仓库既有 warning。

M02 需要用户允许后运行正式 `.app` 并操作菜单栏/登录项，本轮未执行。Windows 构建仍为 `pending_environment`，W02 为 `optional_not_run`。
