# T16 图片剪贴板与 bulk 预算

实现提交：`f7e5a07f1e584fcc44ec50f1f376f3a29e8ba15d`。

图片同步是独立的显式开关，默认关闭；游戏模式下即使开关已保存也暂停图片发送和接收。入站操作必须来自当前 TLS 认证且角色匹配的 peer，并沿用文本操作的确定性顺序和精确回声识别。

原始 RGBA 限制为 32 MiB。尺寸乘法、预期 base64 长度、MessagePack 48 MiB 帧上限和 SHA-256 摘要都在系统图片解码之前检查。超限或格式不匹配的本机图片不会进入发送队列，界面收到不含图片数据的错误通知。

QUIC bulk 使用 128 MiB 进程级字节预算，覆盖发送载荷、接收读取和生产端编码工作区。接收最大项预留 124 MiB；预算不足立即拒绝，可靠 input、control 和 motion 使用独立调度。预算 guard 在正常、错误和超时路径通过 RAII 释放。发送仍使用既有 5 秒有界超时，尚未验证在途 stream 的瞬时取消。

提交后证据：

- `.local-evidence/t16-postcommit-rust.log`：220 passed，0 failed。
- `.local-evidence/t16-postcommit-isolation.log`：18 passed，0 failed。
- `.local-evidence/t16-postcommit-lint.log`：退出码 0。
- `.local-evidence/t16-postcommit-build.log`：退出码 0。
- `.local-evidence/t16-postcommit-mac-check.log`：退出码 0，保留仓库既有 warning。

Mac 应用和真实剪贴板未启动；Windows 标准库缺失，构建状态为 `pending_environment`；Windows 和 LOL 实机为 `optional_not_run`。
