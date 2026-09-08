# MyKVM Local 交付说明

本地分支为 `feat/mac-first-kvm`，上游基线为 `a2ea4164861de31b562c8417eeb7879dbc8c23cb`。代码、自动化、Mac ARM64 打包、Windows 构建脚本和资源采样准备已经完成；Mac/Windows/LOL 真实运行尚未完成。因此当前交付是可安装验证的 Mac 开发预览及完整源码，不是已经通过双机实测的稳定版。

## 已实现

- 产品方向固定为 Windows 物理键鼠控制 Mac，不传画面；旧 LAN 数据入口、上游更新、特权 helper、SYSTEM 服务和运行时防火墙修改均关闭。
- V2 QUIC 使用逐设备持久证书信任、实际 TLS 连接身份、角色/会话/代次/序号门控；控制、可靠输入和 latest-wins motion 分离。
- “控制 Mac”“返回 Windows”“紧急返回”有独立状态和热键；紧急路径先恢复本地门控。游戏模式在 Windows hook 首段直接放行，关闭边缘切换。
- 接收端按会话记录按键、修饰键和按钮，End、断流、3 秒租约、故障和返回路径执行释放。Mac 修饰键默认保留 Ctrl/Command/Option 语义，支持显式互换。
- 双向文本剪贴板使用版本化操作和精确回声抑制；图片默认关闭，游戏模式暂停，文本/图片/QUIC bulk 均有独立大小或全局内存上限。
- Rust 后台独立于设置 WebView；中文菜单栏、设置、诊断脱敏、单实例和显式普通用户自启已实现。

## 验证结果

- 225 个 Rust 库测试通过，0 failed/ignored。
- 23 个隔离与安全脚本测试通过，前端 lint/build、核心文件格式检查和 Mac `cargo check --locked --lib` 通过。
- 46 个定义用例为 `pass`；Mac 运行 5 项为 `not_run`，Windows build 1 项为 `pending_environment`，Windows/LOL 实机 9 项为 `optional_not_run`。
- 严格全仓 fmt 仍因已有格式差异失败；严格 Clippy 仍有 99 项跨平台 dead-code、旧 C 字符串、参数数量等基线诊断。详情见 `docs/T20-regression-review.md`。

测试默认使用 FakeCapture/FakeInjector。真实 QUIC 回环使用两个临时身份和 FakeInjector，不读取或注入桌面输入，也不操作系统剪贴板。

## Mac ARM64 产物

- App：`src-tauri/target/aarch64-apple-darwin/release/bundle/macos/MyKVM Local.app`
- DMG：`src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/MyKVM Local_0.1.0_aarch64.dmg`
- DMG SHA-256：`9b1aaa057dabd15bc9c6801b8ddade49c110195ba96ff61b53a8996c7be0e310`
- 主程序 SHA-256：`14cad4d22f2be8c481937f4f4a76afeb58334d4b64df99abb5677c691926c53d`
- 架构/版本/标识：Mach-O arm64，`0.1.0`，`local.mykvm.gaming`

`hdiutil verify` 已确认 DMG 有效。构建使用 `--no-sign`；主程序只有链接器 ad hoc 标记，app bundle 没有完整 Developer ID 签名或公证，严格 codesign 校验失败。校验命令：

```sh
shasum -a 256 -c docs/PREVIEW_ARTIFACTS.sha256
```

## 安装与首次受控验证

安装、第一次启动和辅助功能授权会改变本机应用/TCC 状态，需要用户明确允许后执行。建议先复制到独立的 `~/Applications/MyKVM Local.app`，不要覆盖现有上游应用，也不要关闭 Gatekeeper/SIP。若系统拦截未公证预览，应由用户在系统设置中针对该应用作决定，不运行上游签名脚本或修改整机信任。

Mac 端选择接收角色并只授予所需辅助功能权限；Windows 端需要在原生 Windows 上运行 `scripts/build-windows-preview.ps1` 得到真实 NSIS 产物，然后选择控制角色。双方通过六位验证码配对，核对显示器布局和三个控制热键后，先在普通桌面验证控制、返回、紧急返回和全部按键释放，再决定是否测试剪贴板、自启和游戏场景。图片同步保持关闭，直到文本和返回路径实测稳定。

## 未验证和已知限制

- 没有 Windows 原生编译/安装包证据，也没有物理键鼠、双机网络、锁屏/UAC、睡眠恢复或无控制台实测。
- Mac app 未启动；菜单栏关闭/重开、真实剪贴板、辅助功能拒绝/撤销、IME 和长时资源趋势没有数据。
- LOL 未运行，不承诺反作弊兼容、零游戏影响、绝对零 GPU 或所有窗口模式都能自动交接焦点。
- 强杀接收端可能无法为已经注入到目标应用的状态补发 key-up；同时使用本地 Mac 键盘可能与远端状态叠加。辅助功能权限在会话中撤销时，释放也可能失败并显示错误。
- 顶层 QUIC 内部命令通道仍为进程寿命级无界通道；高频 motion、input、control 和 bulk 已分别通过单槽、有界队列、并发限制和 128 MiB 预算抑制增长。

## 回滚

本轮未安装应用、未启用登录项、未修改钥匙串/TCC/信任，也未停止 Deskflow 或 RustDesk。若之后完成受控安装，先在 MyKVM Local 设置中关闭登录自启，从菜单栏退出，再删除独立安装的 app。需要清除本 fork 的配对和设置时，先备份后删除 `local.mykvm.gaming` 对应的用户配置目录；不要删除上游 MyKVM 或其他远控工具的数据。

源码回滚使用 `git revert <commit>`，按依赖逆序撤销并复跑 `../source/with-rust.sh node scripts/check-native.mjs`。需要查看原版时，在工作树干净后创建独立 worktree：

```sh
git worktree add --detach ../mykvm-baseline a2ea4164861de31b562c8417eeb7879dbc8c23cb
```

不要用强制 reset 覆盖未提交改动。当前没有 GitHub fork、远程推送、公开 Release 或 Windows artifact。
