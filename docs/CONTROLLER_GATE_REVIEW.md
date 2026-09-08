# V2 控制端门禁审查

门禁实现 SHA `63154dcbfc4ff68ccb0d1a911c1652b0ba54fde9`。主线程逐项复核 T04、T08、T09、T10、T11 已完成并 approved：控制路径只使用认证 V2 会话；物理键释放和单次焦点门槛位于 Commit 前；关键输入可靠有序并具备释放账本；motion 有界 latest-wins 且绑定显示布局；返回和紧急返回先打开本地门控；游戏模式的 Windows hook 直接放行。

因此 `V2_NATIVE_CONTROLLER_ENABLED` 已设为 true，旧 LAN 数据路径继续永久为 false，普通用户预览仍不启用 helper、驱动、安全桌面控制或更新安装路径。提交前 203 个 Rust 库测试、12 项隔离检查、前端 lint/build 和 Mac cargo check 通过，日志为 `.local-evidence/controller-gate-precommit.log`。

这次开门只表示代码路径可进入后续构建和验收。Windows 标准库在当前 Mac 缺失，W01 仍为 `pending_environment`；Windows 实机、双机输入、LOL 和安装包均未运行，不能据此宣称当前已有可安装成品。
