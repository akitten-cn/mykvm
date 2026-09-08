# T04.a · 路由核心与本地应急通道

实现五种状态、指定目标幂等请求、2500ms 准备期限、1500ms 松键期限、Ready/Commit/ACK 校验、焦点失败回退、本地模式切换。返回本机先设置原子覆盖、请求平台恢复，再返回待发送 End/Cancel 事件；路由核心没有网络等待。

LocalOverride 把取消代际和路由位放在同一 AtomicU64。紧急请求不需要 actor 锁；ACK 用 compare_exchange 授权本代际，不能通过普通 store 把紧急标志清掉。代际耗尽保持本地。平台激活过程中收到紧急请求也会拒绝最终激活并请求清理。

16 个路由测试通过，完整库回归 127 passed/0 failed/0 ignored。先写测试观察失败，再实现；审查补充测试还发现并修复两处边界：prepare 部分失败未清理，以及首次观察到松键已超过截止时间仍提交。记录位于 `.local-evidence/T04a-*`。新增路由/端口文件 rustfmt 检查与 git diff --check 通过。

## 未完成的真实接入（T04.b）

此核心当前未连接生产 hotkey、input.rs 钩子或 Tauri IPC；不能据此称“实际热键已可用”。Ready/ACK 必须由 T05/T06 提供验证后的连接与会话事件，当前 u128 是核心不透明会话标识，不是完整网络 SessionId。集成时须与双方 boot ID/随机 nonce 绑定，不能把公开 core 方法当认证。

平台 adapter 需要每次输入先检查相同 LocalOverride；捕获准备/恢复在所属平台线程执行；物理按键变化和 ACK 按序送入 coordinator，ACK 前重新取物理状态；commit_ack 返回错误时必须中止/关闭对应认证会话。注入账本与租约由 T07 实现。

因此 T04 整体仍为 in_progress，只有 T04.a 代码及核心模拟审查完成。Windows 钩子、真实焦点交接、网络认证、可靠释放、Mac 快捷键效果仍未验证。没有据核心单测把 A28/M05 或 Windows/LOL 实机标成 pass。
