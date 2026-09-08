# T22 Mac ARM64 本地预览包

构建实现 SHA：`7ac6d87cc26ae17a4a465c05435230e182075fb0`。实际命令为 `sh scripts/build-mac-arm.sh`，证据包装器记录在本地 `.local-evidence/t22-mac-bundle-final.log`，退出码 0。首次 Tauri DMG 尝试因 Finder AppleEvent 超时失败；脚本随后改为先由 Tauri 生成 app，再用无 Finder、无挂载的 `hdiutil create -srcfolder` 生成压缩 DMG，隔离测试继续通过。

真实产物：

- `src-tauri/target/aarch64-apple-darwin/release/bundle/macos/MyKVM Local.app`，约 17 MiB；主程序为 Mach-O 64-bit arm64。
- `src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/MyKVM Local_0.1.0_aarch64.dmg`，约 6.6 MiB；`hdiutil verify` 报告校验有效。

应用版本 `0.1.0`，bundle identifier 为 `local.mykvm.gaming`。DMG SHA-256 为 `9b1aaa057dabd15bc9c6801b8ddade49c110195ba96ff61b53a8996c7be0e310`；主程序 SHA-256 为 `14cad4d22f2be8c481937f4f4a76afeb58334d4b64df99abb5677c691926c53d`。可用 `shasum -a 256 -c docs/PREVIEW_ARTIFACTS.sha256` 复核。

构建使用 `--no-sign`。Mach-O 带链接器 ad hoc 标记，但 app bundle 没有完整资源封装签名，严格 `codesign --verify --deep --strict` 失败；没有 Developer ID、TeamIdentifier 或公证票据。该状态适合本机开发预览记录，不代表可直接分发的已签名应用。

本轮没有挂载 DMG、安装或启动 app，没有写 `/Applications`、钥匙串、系统信任或 TCC。M01 仅因真实 ARM64 app/dmg 构建和只读校验而标记 pass；M02/M03/M04/M06 等运行和权限用例仍为 `not_run`。
