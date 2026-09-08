# T13 Windows 原生剪贴板通知

类型化读取策略提交 `9987210c8fd61d60915327bf614795239ec4b90e`，Windows 后端提交 `3a5f7dc5f0e010c6881b04b1259986ec2105ac13`。

Windows 用户进程现在创建独立的 message-only window，并调用 `AddClipboardFormatListener` 接收 `WM_CLIPBOARDUPDATE`。通知只携带 `GetClipboardSequenceNumber`，实际读取仍在剪贴板工作线程进行。目标不存在时排空通知，避免稍后连接目标时把历史内容当作新复制；目标存在时不再定时轮询。

读取结果区分 Content、Unchanged、Empty、Busy、Unsupported 和 Error。Unicode 文本通过 `OpenClipboard`/`GetClipboardData(CF_UNICODETEXT)` 在当前用户会话读取，分配前限制 UTF-16 缓冲区；busy 最多尝试三次，每次间隔 4 ms。每次读取前后比较系统 sequence，若期间格式变化便丢弃该结果，不回退到旧文本。同步循环只为 Content 构造远端包，其余状态不产生空写，Error 只记录无原文的原因。

监听器 Drop 发送 `WM_CLOSE`，窗口过程依次调用 `RemoveClipboardFormatListener` 和 `DestroyWindow`；`WM_NCDESTROY` 释放窗口上下文并退出消息循环。消息循环异常退出也检查并销毁残留窗口。没有调用活动会话切换、SYSTEM 服务或跨会话进程 API。

A33 由类型化状态测试、系统写失败测试及生产消费分支共同覆盖。A40 的普通用户静态证据保持 pass。A38 的完整图片 base64/解码预算属于 T16，暂不标记通过。当前 Mac 没有 Windows Rust 标准库，W01 保持 `pending_environment`；本轮没有执行 Windows 原生编译或运行。

提交后 `.local-evidence/t13-postcommit.log` 退出码 0：207 个 Rust 库测试、15 项隔离检查、前端 lint/build、Mac `cargo check` 和 clipboard 格式检查通过。旧 LAN 剪贴板网络门禁仍关闭，T15 才会接入认证 V2 双向操作。
