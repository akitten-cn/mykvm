# T07.a · 按会话输入账本与真实释放命令

状态：done。断线租约和健康超时属于 T07.b，生产 V2 输入门禁仍关闭。

`PressedState` 以 Windows scan code 加 extended 位识别物理键；scan code 缺失时才退回 virtual key。key-down 冻结当时的目标 key code，后续配置变化不会改变对应 key-up。自动重复不重复提交 down；多个物理源映射到同一目标时只提交一次 down，并在最后一个所有者释放时提交一次 up。

按钮账本记录自己的可靠位置快照。正常 End 和注入故障不再发送主进程中为空操作的 ReleaseAll，而是从账本生成逐键及逐按钮 up，并逐项调用 InjectorPort。若任何释放提交失败，账本恢复以允许后续 End/健康任务重试；已成功的 up 可能安全地重复提交。

PressedState 的 3 项测试完成 A19/A20；ReceiverSessionRuntime 的注入故障测试为 A21 提供部分证据，均使用 FakeInjector，不访问真实桌面。decoder/stream 失败通知、断线后自动触发、租约超时和错误状态可见性仍由 T07.b 实现。进程强杀无法运行用户态清理；在这些工作完成前 `V2_NATIVE_RECEIVER_ENABLED` 保持 false。
