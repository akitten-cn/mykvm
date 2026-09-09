# T23 Windows 预览包准备

实现 SHA：`c1ae061e4384392e796378a4b0d3dc931dc09549`。新增 `scripts/build-windows-preview.ps1`，仅允许在原生 Windows runner 运行：执行锁定依赖安装、Tauri `--no-sign` NSIS 构建，要求至少出现一个真实 `.exe`，并在同目录生成 `SHA256SUMS`。脚本不安装产物、不启动应用、不部署 helper、不修改防火墙或系统信任。

`.github/workflows/native-preview.yml` 的 Windows 2022 矩阵在非交互原生检查通过后调用该脚本，并上传 `src-tauri/target/release/bundle/nsis/*.exe` 与校验和文件作为保留 7 天的 artifact。工作流权限为 `contents: read`，不写 Release、不请求发布或 OIDC 权限，也不使用发布秘密。

2026-09-09 在 Windows Server 2022 GitHub runner 上实际执行该脚本。首次运行暴露 Unix RSS 解析测试在 Windows cfg 下误判单位的问题；提交 `94ff6ed173ba70e4ebab48e20a80209e1693d665` 修复后，Windows 原生检查、216 个库测试、release 构建、NSIS 打包和 artifact 上传全部通过。macOS 原生矩阵与独立 Linux CI 也通过。

- CI：[Actions run 34344520603](https://github.com/akitten-cn/mykvm/actions/runs/34344520603)
- Artifact：`windows-preview-94ff6ed173ba70e4ebab48e20a80209e1693d665`，保留 7 天
- 安装包：`MyKVM Local_0.1.0_x64-setup.exe`，3,728,677 bytes
- SHA-256：`9db3d1e04510e8fe7d179bb529f6604f73ab1bdbe50c111ddbe4f473dfd9fbc0`
- 本地下载：`artifacts/windows/94ff6ed173ba70e4ebab48e20a80209e1693d665/MyKVM Local_0.1.0_x64-setup.exe`

`file` 将安装器识别为 Windows PE/Nullsoft Installer 自解压包，下载后的 SHA-256 与 runner 生成值一致。安装器没有代码签名；没有在 Windows 桌面安装或启动，因此 W02、物理键鼠、双机网络与 LOL 实机仍为 `optional_not_run`。
