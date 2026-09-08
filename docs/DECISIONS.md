# 决策记录

## ADR-001：基线与工作区

使用新目录的本地分支 `feat/mac-first-kvm`，上游 `https://github.com/XxMinor/mykvm.git`，完整基线 `a2ea4164861de31b562c8417eeb7879dbc8c23cb`。审查参考 `bb5421fe1d4c0c8c72bb3f6c0c35f0a0f994209b` 可达，但不是强制回退目标；其新增拖放内容不直接并入本轮。没有覆盖已有工作树，也没有 GitHub 写入授权或远程推送。

## ADR-002：安全开发与身份隔离

Rust 安装到外层项目 `.toolchain/`，使用 `source/with-rust.sh`，不修改全局模型、代理配置和 shell profile。保留原 MIT 许可证及已有 QUIC stream 并发修复、Windows release 无控制台属性。应用身份与 helper、更新、安装副作用分别核验，不能只改显示名。审查后补充发现运行时 netsh 路径，已在执行前加策略门禁并纳入检查。

2026-09-09：T08–T11 的实现、自动化和主线程审查完成后开启 `V2_NATIVE_CONTROLLER_ENABLED`。Windows 构建证据仍独立为 pending_environment；开门不把 Mac 编译推导为 Windows 条件编译或实机通过。V1 LAN、特权 helper、驱动和安全桌面功能继续关闭。

## ADR-003：先纯路由核心，再真实接入

T04 拆为 T04.a 与 T04.b。原子紧急返回独立于 actor 锁及网络队列；取消代次避免晚 ACK 恢复远控。T04.a 已测试，T04.b 依赖 T05/T06，不宣称纯状态机已提供实际切换功能。

## ADR-004：旧 LAN 入口关闭

现有服务端 with_no_client_auth、SocketAddr 授权缓存及发现更新信任不构成双向连接授权。按交接要求，旧 discovery/input/clipboard/file 入口在开启网络或操作系统副作用前拒绝。该限制用于未完成的开发分支，会使其暂时无法远控；不是最终产品方案。T05.b 必须实现逐设备持久信任及具体 QUIC 连接绑定，再接 V2，禁止简单翻转旧策略常量。

## ADR-005：验证分层

Windows runner 不可用时保留真实原生检查工作流，状态记 pending_environment，继续 Mac 可执行任务。基线 fmt/clippy 失败单独留档；新核心格式及适用回归通过，不把跳过记成通过。实机、打包和资源数据不能由 Mac 库编译替代。

## ADR-006：逐设备连接授权

每台设备复用自己的持久自签名传输证书作为配对身份，不新增密码学算法。人工配对成功后，双方分别保存对方证书、角色和单调信任版本。TLS 客户端总是出示本机证书；服务端先验证 CertificateVerify 所证明的私钥持有，再以证书完整 DER 精确匹配持久记录，生成包含 peer_id、角色、信任版本、远端地址和本进程连接代次的 `AuthenticatedPeer`。输入 datagram 只为已授权连接启动读取器；普通 stream 也必须已授权并满足方向角色。

未授权连接只允许现有的限时人工配对确认。确认包声明的证书必须等于 TLS 连接实际出示的证书；成功后立即关闭该未授权连接，后续数据必须新建连接并重新完成授权。发现广播仍可提供地址，但不会修改 `trustedPeers`。旧数据发送 API 暂时保留给迁移对照，并受全局旧 LAN 门禁；T06 新路径必须从持久信任构造目标，不能使用发现广播替换的证书。
