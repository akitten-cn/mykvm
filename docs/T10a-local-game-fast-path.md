# T10.a 本地游戏最短路径

实现 SHA `2dfda6453a7dcbf725ddf5584e7d01e462e1e7c7` 增加持久化手动游戏模式及中英文设置开关。开启时若存在远程或准备中会话，先设置共享本地门控并排队返回动作。Windows 两个低层 hook 在获取 capture context、布局 mutex 或调用发送路径前，只读取一个进程级 `AtomicBool`；游戏模式直接 `CallNextHookEx`，不做边缘判断、布局重算、鼠标归中、网络投递或逐事件日志。控制 Mac 的系统全局快捷键仍由 Tauri 注册机制处理。

正常桌面/远程 hook 路径增加 `catch_unwind` 外壳；panic 不越过 Windows FFI，且先请求原子本地恢复再放行事件。全局 context 读取改为 `try_lock`，锁竞争时失败开放。现有边缘准备仍只在桌面模式运行；CommitAck 前保持 Windows 输入本地。

A08 的纯测试执行 50,000 次本地游戏判断，重路径计数保持 0；隔离检查验证两个 hook 的原子判断位于 context 获取和 panic 包装之前。提交后 `.local-evidence/t10a-postcommit.log` 记录 200 个 Rust 库测试、10 项隔离检查、前端 lint/build、Mac cargo check、相关格式检查和干净工作树均退出 0。

T10.b 实现 SHA `15fd004b5f471f7f83cc3b86fffb1a448e74dfaf` 将桌面/远程重路径迁到捕获线程。两个 inner hook 现在只读取缓存/原子状态、复制 Windows 事件并向 1024 项有界通道 `try_send`；不等待 mutex，不执行布局重算、光标操作、日志或网络发送。控制热键在捕获启动时预解析，配置变化会重启捕获以刷新缓存。队列满、断开或 panic 均先请求本地门控并放行当前物理事件。

新增测试证明满队列立即返回失败，源码隔离检查逐个检查两个 inner hook 的禁用调用表。提交后 `.local-evidence/t10b-postcommit.log` 记录 202 个 Rust 库测试、10 项隔离检查、前端 lint/build、Mac cargo check、Windows 源码完整语法解析和干净工作树均退出 0。A28 标记 pass。Windows 目标标准库仍未安装，Windows 编译和可选性能实机保持 `pending_environment`/`optional_not_run`。
