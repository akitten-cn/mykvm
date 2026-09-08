# T18 中文设置、菜单栏与诊断安全

实现提交：`7b8e36a88f9de3ebc342566d452ffe5c204d6ae4`、`b8e7919699075551cebecbceb461a4e51280c379`、`7d0e5631bb6be5da1e07f6691cd2d831948ca843`。

设置页覆盖设备角色、控制/返回/紧急热键、游戏模式、Mac 修饰键策略、文本和图片剪贴板、后台自启。菜单栏提供紧急返回和暂停/恢复后台服务；紧急动作先设置独立本地门控，再进入控制动作队列。

主要可变 IPC 在进入磁盘、网络或系统剪贴板前校验：布局总量不超过 2 MiB，设备和屏幕有数量/字段/尺寸边界，枚举值受限，手工主机拒绝空值、超长和控制字符，配对码必须为六位数字，手工剪贴板文本不超过 1.5 MiB。

复制诊断只含匿名 peer 序号、角色、状态、端口和计数；本机身份、设备名、IP/host 与绝对路径均不进入报告。日志静态扫描禁止正文、配对密钥、公钥和具体键码字段。中英文资源通过 AST 比较完整键集合。

提交后证据：

- `.local-evidence/t18-postcommit-rust.log`：225 passed，0 failed。
- `.local-evidence/t18-postcommit-isolation.log`：22 passed，0 failed。
- `.local-evidence/t18-postcommit-lint.log`：退出码 0。
- `.local-evidence/t18-postcommit-build.log`：退出码 0。
- `.local-evidence/t18-postcommit-mac-check.log`：退出码 0，保留仓库既有 warning。

未启动应用、修改真实剪贴板或安装登录项。Windows 构建状态保持 `pending_environment`。
