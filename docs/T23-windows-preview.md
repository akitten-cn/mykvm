# T23 Windows 预览包准备

实现 SHA：`c1ae061e4384392e796378a4b0d3dc931dc09549`。新增 `scripts/build-windows-preview.ps1`，仅允许在原生 Windows runner 运行：执行锁定依赖安装、Tauri `--no-sign` NSIS 构建，要求至少出现一个真实 `.exe`，并在同目录生成 `SHA256SUMS`。脚本不安装产物、不启动应用、不部署 helper、不修改防火墙或系统信任。

`.github/workflows/native-preview.yml` 的 Windows 2022 矩阵在非交互原生检查通过后调用该脚本，并上传 `src-tauri/target/release/bundle/nsis/*.exe` 与校验和文件作为保留 7 天的 artifact。工作流权限为 `contents: read`，不写 Release、不请求发布或 OIDC 权限，也不使用发布秘密。

本机执行 `scripts/check-native.mjs` 的 8 个适用步骤均为退出码 0，证据为 `.local-evidence/t23-postcommit.log`；A40/A42 的隔离检查覆盖新脚本和工作流。Ruby YAML parser 也成功读取工作流。当前 Mac 没有 PowerShell、Windows 标准库或原生 runner，因此脚本没有执行，未产生 Windows EXE；W01 与 `windows_build` 保持 `pending_environment`，Windows/LOL 实机保持 `optional_not_run`。
