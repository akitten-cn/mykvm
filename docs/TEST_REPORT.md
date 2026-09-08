# 首批实现测试报告

日期：2026-09-08。受测代码 `83604777ef3ff49ab95821f9b0b03b74dbbc8fb3`，平台 darwin/arm64。后续收尾提交仅更新文档及任务状态。

从仓库根目录运行：

```sh
../source/with-rust.sh node scripts/check-native.mjs
```

实际执行记录保存在本地 `.local-evidence/native/results.json` 和同目录各命令日志，未纳入版本控制。最终检查于 00:39:36–00:39:43 UTC 执行，8 个子命令退出码均为 0。

|检查|结果|证明范围|
|---|---|---|
|Rust 库测试|127 passed，0 failed，0 ignored|上游 105 项，加策略 1 项、假平台 5 项、路由 16 项|
|隔离检查|7 passed|配置、调用路径和脚本的静态断言；不代替安装器实测|
|npm lint / build|通过|前端检查和静态资源生成|
|cargo check --locked --lib|通过|Mac 库条件编译，不代表 Windows 编译|
|新增核心 rustfmt|通过|control_ports、routing、fork_policy 三个文件|
|全仓 fmt|失败（基线已有）|未以大范围格式化混入原子实现|
|严格 clippy|失败|基线 53 项诊断，阶段复查 51 项；新核心未产生诊断。最终关闭旧 LAN 后未重跑此全仓检查|
|Windows 原生 CI|pending_environment|工作流存在但未推送、未运行|
|Mac 应用包/运行|not_run|没有 app/dmg 交付|
|Windows / LOL 实机|optional_not_run|不作为其余开发的关卡|

开发过程中路由补充测试暴露了重复 Ready 覆盖会话和部分准备失败清理问题，已修复并通过回归。测试默认使用 FakeCapture/FakeInjector，不调用真实键鼠或剪贴板。

纯核心通过不等于 A01–A04 等端到端用例通过；这些用例保留 not_run 并记录部分证据。认证正反测试、协议回环、真实 Windows 分支编译、可靠释放集成、资源数据均尚未完成。详见 taskboard、testcases 和 SOURCE_AUDIT。

## T05.b 连接授权增量

2026-09-08 在 Mac arm64 上执行 `../source/with-rust.sh python3 ../source/run-check.py t05b-native node scripts/check-native.mjs`，退出码 0。前端 lint/build、Mac cargo check、7 项隔离检查及 135 个 Rust 库测试全部通过。测试数较上一阶段增加 8 项。

其中两项使用真实本机 UDP/QUIC endpoint，但只传测试字节，不调用桌面、输入注入或剪贴板：已配对证书成功产生带连接代次和角色的认证上下文；未知证书连接发送的 datagram 未到达处理回调。其余负例覆盖证书旋转、重复证书、错误角色、配对声明证书与 TLS 证书不一致。A10、A11、A12 标记 pass；A43 和 V2 会话相关用例仍为 not_run。

本次命令运行时工作树尚未提交，所以证据包装器记录的是父提交 `4e003d2802fcce97fe6b73929292fc5164a38db9`；日志实际编译了当前未提交 diff。提交后将使用同一代码树执行一次定向复核并记录最终 SHA。
