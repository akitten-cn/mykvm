# T14 Mac 进程内剪贴板后端

实现 SHA `e2a3ccdb47ad1680ccef8f71dd182c2706ca614d`。

Mac 文本读写已从 `pbpaste`/`pbcopy` 子进程改为项目现有 arboard 的进程内 API，图片路径继续使用同一后端。新增对锁定版本 `objc2-app-kit 0.3.2` 的直接最小依赖，只启用 `std` 与 `NSPasteboard`，用于读取 general pasteboard 的 `changeCount`。

`MacClipboardWatcher` 启动时记录当前 changeCount，之后每 120 ms 只比较计数；没有变化便不读取剪贴板。远端目标不存在时更新基线，稍后建立目标不会发送旧内容。计数变化后才调用类型化读取，empty/busy/error 不构造远端包。macOS 没有被描述为具备不存在的通用剪贴板事件 API。

A37 的隔离检查确认生产 `clipboard.rs` 中不存在 pbpaste/pbcopy，且运行循环在 `wait_for_change` 后才读取。纯测试覆盖 changeCount 去重以及中文、emoji、CRLF/LF 和长 UTF-8 在内容模型中的逐字节保持。A32 的网络封装与收敛证据留给 T15，暂不标记通过。

提交后 `.local-evidence/t14-postcommit.log` 退出码 0：208 个 Rust 库测试、16 项隔离检查、前端 lint/build、Mac `cargo check` 和 clipboard 格式检查通过。M03 需要用户授权固定文本并在 finally 恢复原剪贴板，本轮保持 not_run；没有读取或写入用户真实剪贴板。
