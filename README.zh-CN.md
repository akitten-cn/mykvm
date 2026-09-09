# MyKVM Local

MyKVM Local 基于开源项目 [XxMinor/mykvm](https://github.com/XxMinor/mykvm) 修改，开发基线是上游提交 [`a2ea4164861de31b562c8417eeb7879dbc8c23cb`](https://github.com/XxMinor/mykvm/commit/a2ea4164861de31b562c8417eeb7879dbc8c23cb)。本 fork 保留上游版权与 MIT 许可证。

本 fork 面向一个明确场景：Windows 主机提供物理键盘和鼠标，Apple Silicon Mac 用于 Codex、终端、IDE 和浏览器。两台机器的显示器各自直连，只传键鼠输入和可选剪贴板，不传画面。

[English](./README.md) · [上游项目](https://github.com/XxMinor/mykvm) · [完整交付状态](./docs/DELIVERY.md)

## 相对上游的主要修改

- 增加明确的“控制 Mac”“返回 Windows”“紧急返回”动作和可配置热键。
- 增加 Windows 本地游戏模式：关闭边缘切换，并在 Windows hook 最前段放行，不进入布局、光标、网络和日志重路径。
- 以有界 V2 QUIC 协议替换旧数据入口，分别承载控制、可靠输入和 latest-wins 鼠标移动。
- 入站数据绑定到当前 TLS 连接实际出示的证书、持久配对设备、角色、会话、进程代次和序号；发现广播不能静默修改信任。
- 按会话记录按键、修饰键和鼠标按钮，在正常返回、End、断流、租约到期、故障和紧急返回时可靠释放。
- 明确 Mac 修饰键规则：Windows Ctrl 默认仍是 Mac Control，Windows 键对应 Command；可选 Ctrl/Command 互换预设。
- 增加双向、版本化文本剪贴板和精确回声抑制；图片同步默认关闭，游戏模式暂停，并受格式校验和全局 bulk 内存预算保护。
- 后台运行由 Rust 持有，不依赖设置 WebView；设置窗口可以销毁和重新创建。
- 增加简体中文设置与菜单栏、普通用户显式自启、单实例唤起、IPC 参数校验和诊断脱敏。
- fork 身份改为 `local.mykvm.gaming`；禁用上游自动更新、特权 helper、SYSTEM 服务、自动防火墙修改和上游发布自动化。
- 增加 FakeCapture/FakeInjector、认证 QUIC 本地回环、Mac/Windows 原生 CI、无签名预览打包、校验和与被动资源采样器。

## 当前状态

|范围|状态|
|---|---|
|自动化测试|225 个 Rust 测试和 23 个隔离测试通过|
|Mac 构建|ARM64 app 与 DMG 已生成并校验|
|Mac 运行|未运行，没有修改辅助功能或 TCC|
|Windows 构建|CI 与 NSIS 脚本已就绪，原生 runner 尚未完成|
|Windows 物理键鼠|未运行|
|LOL|可选，未运行|

Mac 产物是未签名、未公证的开发预览。源码和自动化链路已具备受控试用条件，但目前不能宣称 Windows→Mac 双机物理测试已经通过。

## 安全范围

- 只传输入和可选剪贴板；不包含显示捕获、视频传输、驱动、游戏注入、特权服务、安全桌面 helper 或反作弊规避。
- 旧 LAN 输入、剪贴板和文件入口默认拒绝；只有已配对控制端的认证 QUIC 连接可以进入 V2 输入路径。
- 文本与图片分别限流。原始图片上限 32 MiB、编码 bulk 帧上限 48 MiB、bulk 总工作内存预算 128 MiB。
- 图片剪贴板默认关闭；登录自启也必须由用户显式开启。
- 不承诺绝对零 GPU、所有游戏版本兼容或控制 Windows 安全桌面。

## 构建和测试

环境要求：Node.js 22、记录构建使用的 Rust 1.98.1；Mac 需要 Xcode Command Line Tools，Windows 需要 Visual Studio 2022 C++ Build Tools 和 WebView2。

不启动桌面应用的完整检查：

```bash
npm ci
node scripts/check-native.mjs
```

在 Apple Silicon Mac 构建无签名预览：

```bash
sh scripts/build-mac-arm.sh
shasum -a 256 -c docs/PREVIEW_ARTIFACTS.sha256
```

在原生 Windows 构建无签名 NSIS：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-preview.ps1
```

Windows 产物写入 `src-tauri\target\release\bundle\nsis\`，同目录生成 `SHA256SUMS`。

## CI 产物

`.github/workflows/native-preview.yml` 在 macOS 14 与 Windows Server 2022 上运行非交互检查。Windows job 还会构建无签名 NSIS，并把它以 `windows-preview-<提交 SHA>` 名称保留 7 天。工作流只有仓库只读权限，不创建 GitHub Release。

## 首次受控试用

不要覆盖已有上游安装。先核对校验和，再以独立的 **MyKVM Local** 名称安装。Mac 选择接收角色，Windows 选择控制角色；双方使用六位验证码配对，核对显示器布局和三个控制热键。先在普通桌面验证控制、返回、紧急返回和全部按键释放，再考虑图片剪贴板、自启或游戏场景。

Mac 注入输入需要辅助功能权限。本仓库不会自动关闭 Gatekeeper 或修改 TCC。当前产物路径、未验证项和回滚步骤见 [交付说明](./docs/DELIVERY.md)。

## 文档

- [实施进度](./docs/PROGRESS.md)
- [测试报告](./docs/TEST_REPORT.md)
- [源码与安全核验](./docs/SOURCE_AUDIT.md)
- [任务板](./docs/handoff/taskboard.json)
- [Mac 预览构建证据](./docs/T22-mac-preview.md)
- [Windows 预览流水线](./docs/T23-windows-preview.md)
- [资源验证方法](./docs/T24-resource-validation.md)

## 许可证与署名

本项目保留原项目的版权与署名，派生自 [XxMinor/mykvm](https://github.com/XxMinor/mykvm)，按 [MIT License](./LICENSE) 发布。MyKVM Local 是独立 fork，不代表上游官方版本。
