# T08.d 显示布局与坐标门控

实现 SHA `3dfca829cc74e7d4ff4c47ce2fb826ecd10af2ee` 将目标显示器和非零 `layout_revision` 绑定到 Prepare/Ready 握手，并把 `display_id`、`layout_revision` 放入每个 V2 motion datagram。控制端只接受与请求完全一致的 Ready；接收端只有在显示器标识和布局版本都匹配当前本机布局时才建立会话。

接收端以显示器内逻辑坐标作为线上的统一单位，先检查 `0 <= x < width`、`0 <= y < height`，再按本机原生显示范围映射到包含负原点的系统坐标。motion 和可靠按钮/滚轮快照都在提交 FakeInjector/NativeInjector 之前完成同一映射；非法范围、未知显示器或旧布局版本不会产生鼠标注入，也不会消耗可靠输入序号。

接收端每 500 ms 只读刷新一次本机显示器列表。拔插、缩放、分辨率或原生范围变化会替换广播布局、结束活动会话、清空待处理 motion 并通过 pressed-state 释放已按键和按钮；界面状态显示“显示器布局已变化，远程输入已释放”。空闲控制端不运行该显示器轮询。

A29 的纯测试覆盖 1440×900 到 2560×1600 的缩放、负坐标原点、边界点映射、越界点击不注入、旧版本不注入、无效事件不消耗序号，以及布局变化后会话关闭和输入释放。源码隔离检查确认这些字段进入生产协议、Windows ControllerTarget 和接收映射。提交后 `.local-evidence/t08d-postcommit.log` 记录 203 个 Rust 库测试、12 项隔离检查、前端 lint/build、Mac cargo check 和干净工作树均退出 0。

Mac 应用未启动，Windows Rust 目标仍缺失，因此显示器实际拔插、Windows 条件编译和跨机坐标体验尚未运行。自动化结果完成 A29 的开发门槛，不替代 M01/W01 或实机验收。
