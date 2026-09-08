# T09 Windows 指定设备热键协调

实现 SHA `9aed9a81aa928ad67ba2bf298d49b63027b44dd6` 增加“控制 Mac”“返回 Windows”“紧急返回 Windows”三个独立动作，默认分别为 `Ctrl+Alt+F11`、`Ctrl+Alt+F10`、`Ctrl+Alt+Shift+F10`。仅控制端注册；保存时会统一规范化并拒绝与快捷启停、四向切屏或另外两个控制动作重复的组合，错误信息为中文。注册变更采用回滚流程，避免部分成功。

系统全局快捷键和 Windows 低层键盘钩子共享原子去重器。返回动作在排队前先设置共享 `LocalOverride`，Windows 鼠标钩子随即放行本地事件；捕获线程再发送带 User/Emergency 原因的 V2 EndSession。接收端已有的 pressed-state 账本负责释放热键识别前发出的 Ctrl/Alt。控制 Mac 动作选择当前配置设备/显示器，控制端轮询 `GetAsyncKeyState` 的完整虚拟键范围，只有物理键和鼠标键全部释放才继续提交。

提交后 `.local-evidence/t09c-postcommit.log` 记录 198 个 Rust 库测试、9 项隔离检查、前端 lint/build、Mac cargo check、相关 rustfmt 和干净工作树均退出 0。A05、A06 已通过。A28 只有源码审查部分证据：hook 不执行磁盘或 GUI 调用，网络投递为有界非阻塞路径；彻底移除回调中的布局/context mutex 等工作留给 T10，因此尚不标记通过。

本机没有 Windows Rust 标准库，W01 保持 `pending_environment`。T11 的 `WindowsV2FocusPort` 仍故意返回 `Unavailable`，控制端总门禁仍为 false；本提交不能作为可用版。
