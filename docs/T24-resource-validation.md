# T24 资源诊断与可选运行检查

实现 SHA：`139cf4644a1261896460ac4d86bfe45cf884dfb1`。`scripts/sample-mac-resources.sh` 只接受已经运行的 PID，不负责启动、停止或控制进程。它按指定时长和间隔记录根进程及递归子进程的 CPU、RSS KiB、运行时间和命令，并保留 macOS `top` 的 threads、idle wakeups 与 power 原始字段。1 秒 shell PID smoke test 实际采样 2 秒并成功生成 metadata、process TSV 和 top 日志；这只验证采样器，不是 MyKVM 的 M07 性能数据。

用户明确允许启动预览后，可先取得 `mykvm-local` 的 PID，再运行：

```sh
scripts/sample-mac-resources.sh --pid PID --duration-seconds 600 --interval-seconds 5 --output .local-evidence/resources/idle-10m
```

8 小时检查把时长改为 `28800`，不得把短时结果写成 M07。分别记录后台空闲、设置窗口打开、文本回环和图片回环；报告实际秒数、样本数、根进程与子进程 RSS 总和。macOS `%CPU` 以一个逻辑核心约为 100% 的口径读取，不能与 Windows 任务管理器总 CPU 百分比直接比较。`idlew`/`power` 保留原始值，不推导绝对能耗或零 GPU。

Windows W05 使用鼠标当前已有回报率，不修改厂商驱动或系统设置。依次记录应用退出、后台 LocalGame、远端桌面控制各 60 秒：鼠标型号/标称回报率、实际输入事件计数、transport 包计数、CPU、工作集、是否丢失按键/按钮和紧急返回耗时。没有原生 Windows 运行和真实设备数据时保持 `optional_not_run`。

LOL L04 只在用户自愿的练习模式执行。固定游戏版本、地图/场景、分辨率、窗口模式和画质，分别测应用退出、后台本地游戏模式、远端控制状态；每个状态预热后记录相同长度的三轮数据，报告平均 FPS、1% low、P99 帧时间及离散程度。没有数据不得宣传对 LOL 无影响或兼容所有版本。

运行边界：强杀发送端通常由断流或 3 秒租约触发接收端释放，但强杀接收端本身无法保证已注入状态获得应用级 key-up；同时使用 Mac 本地键盘可能与远端状态叠加；辅助功能权限在会话中失效时，注入与释放都可能失败并进入可见错误状态。这些情况需要受控实机验证，不能由 FakeInjector 证明。

本轮为遵守不自动操作真实桌面、剪贴板和权限的边界，没有启动预览 app，因此 M07 为 `not_run`；W05/L04 为 `optional_not_run`。
