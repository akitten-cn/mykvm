# T12 Mac 键位映射与权限边界

实现 SHA `7bf95bb7ebf89c93a04dda839e3423e681935c9d`。默认映射保留按键的字面含义：Windows Ctrl 到 Mac Control，Windows 键到 Command，Alt 到 Option，Shift 保持 Shift。设置页提供显式的 `Ctrl ↔ Command` 预设，也保留逐项自定义；默认值在 Rust 与前端配置中一致。

| Windows 来源 | 默认 Mac 语义 | 互换预设 |
|---|---|---|
| 左/右 Ctrl | 左/右 Control | 左/右 Command |
| 左/右 Win | 左/右 Command | 左/右 Control |
| 左/右 Alt | 左/右 Option | 左/右 Option |
| 左/右 Shift | 左/右 Shift | 左/右 Shift |

接收端在 key-down 进入 `PressedState` 前应用映射；账本以物理 scan code 和 extended 位标识来源，并保存当时的目标键。设置在按住期间变化不会改变随后的 key-up 目标。自动重复不会增加所有权计数，两个物理来源映射到同一目标时也不会提前释放。

现有 VK 表明确覆盖左右修饰键、OEM 标点、空格、导航键、数字区、功能键和 Caps Lock。Caps Lock 现在作为普通 `kVK_CapsLock` 注入，不再暗中合成 Ctrl+Space。输入法切换遵循用户在 macOS 中配置的实际快捷键；本轮没有自动改系统输入源或 TCC，也没有执行真实中文输入测试。Fn/Globe 以及亮度、媒体等依赖硬件/consumer HID 的特殊键不属于当前普通 VK 表，不能宣称支持。

receiver-only 启动只检查并报告 V2 injector 就绪状态，不创建本地捕获线程。`NativeInjector` 在会话 Ready 和每次提交前检查辅助功能权限与 Secure Input；权限不存在、撤销或 Secure Input 阻止时拒绝接管并结束会话。M04/M06 需要用户在固定测试应用中授权后执行，本轮保持 `not_run`。

提交后检查日志 `.local-evidence/t12-postcommit.log` 记录退出码 0：205 个 Rust 库测试、13 项隔离检查、前端 lint/build、Mac `cargo check` 和新增核心格式检查通过。A19、A20、A30、A31 为自动化通过；没有启动应用、注入真实键鼠或读写真实剪贴板。Windows 原生编译仍为 `pending_environment`。
