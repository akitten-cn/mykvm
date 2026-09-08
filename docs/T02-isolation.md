# T02 · Fork 身份和构建副作用隔离

应用名 `MyKVM Local`，identifier `local.mykvm.gaming`，打包主程序名 `mykvm-local`。Tauri app_config_dir/app_log_dir 与 autostart 使用 fork 自己的身份；Windows 单实例 mutex/event、helper pipe/service namespace、启动参数和前端存储键已隔离。不导入上游信任或配置。

更新同时在四层禁用：Tauri 插件不注册、capability 不授权、前端 API 拒绝、设置卡片/启动检查关闭。保留 MIT 版权与上游源码链接；旧 release 工作流移为 `upstream-release.yml.disabled` 仅供审计，不在 Actions 下运行。updater 依赖暂保留以避免无关锁文件变动，运行时没有入口。

构建只构建：移除 Windows sidecar 自动构建和占位文件生成，NSIS 不再挂接上游 hooks，不操作服务、进程或防火墙。Mac 安装/签名快捷命令删除，旧脚本改成明确拒绝；Mac preview 构建不会复制到 /Applications、清 quarantine 或操作钥匙串。

普通用户限制同时在 Rust IPC、进程参数入口和 Windows helper 路由检查。仍保留上游平台代码供未来审查，但当前不提供提权、SYSTEM 服务、安全桌面或 SAS 操作。前端相应提权/安装按钮不可用。这些是有意禁用的范围外功能，不是 Windows 输入实现的 TODO 替代品。

## 验证

先运行隔离测试：6 项失败，确认能检出上游行为。改造后 6/6 通过；Rust 106/106（新增 1 项策略拒绝测试）；npm lint/build 通过。完整命令/退出码见 `.local-evidence/T02-*`。审查 diff，保留上游 Windows release 无控制台与非阻塞 QUIC 发送；没有改输入协议。

Windows 原生构建与运行尚未执行。静态安装检查不是实际安装验证。Mac app 尚未启动，完整 V2 安全会话尚未实现，当前不是可投入使用的预览版。

## 后续安装和回滚

T20/T22 完成前不部署。将来从真实构建产物确认架构和哈希后，用户明确选择安装到独立位置；系统权限由用户授权。不运行上游 install/sign 脚本，不关闭系统安全保护。

源码回滚使用本任务提交的 revert。应用未安装，因此本轮无需系统级回滚，也没有登录项、钥匙串或 TCC 变更需要撤销。原 MyKVM、Deskflow、RustDesk 保持原状。

补充审查：在进入 T05 时另找到 start_discovery 调用的 ensure_windows_firewall_rule（独立于安装 hooks）。新增拒绝优先检查，即使应用被手动提权启动也不自动改防火墙。隔离测试先复现失败再通过。这是对初次安装路径审查遗漏的修正，未在本机执行任何防火墙命令。
