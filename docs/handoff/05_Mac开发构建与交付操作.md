# 在 M4 Mac mini 上开发、构建与交付 MyKVM Fork

版本：2.0 · 本文件是给执行者的操作说明，不表示命令已经运行。

## 1. 放置交接包

新建独立工作区，不覆盖现有项目。

```shell
mkdir -p "$HOME/Projects/mykvm-workspace"
cd "$HOME/Projects/mykvm-workspace"
```

将 ZIP 解压得到的 `handoff/` 放进这里，在 Codex 打开本目录。

```text
mykvm-workspace/
├── handoff/          本交接包
└── mykvm/            Codex 随后创建/使用的代码仓库
```

已有源码仓库时使用原仓库，并将资料放 `docs/handoff/`。不要在已有 Git 仓库里再随意嵌套 clone；先由 Codex 检查目录与 `git status`。

## 2. 检查 Mac 环境

先检测，不直接重装。

```shell
uname -m
sw_vers
xcode-select -p
xcrun --find clang

git --version
node --version
npm --version
rustc -vV
cargo --version
rustup show active-toolchain
```

M4 原生目标应当是 `aarch64-apple-darwin`；若终端/工具实际通过 Rosetta 运行，记录并调整为原生路径，不把 x86 构建误当 ARM64。

上游审查快照声明 Rust 最低版本 1.89，但实际锁定依赖可能有更高要求；以真实构建结果为准。Node 版本读取当前 `package-lock.json` 和依赖 `engines`，不要盲目套用旧教程的 Node 版本。[S07][S09]

缺 Xcode Command Line Tools 时，由用户允许后执行系统安装：

```shell
xcode-select --install
```

其余工具优先复用已安装版本。若必须安装或升级，先说明具体依赖和影响；不默认 `sudo`，不覆盖全局 conda、Node 或 Rust 配置。Tauri 的各平台依赖以官方文档为准。[S17]

## 3. 获取 fork 或先本地开发

### 3.1 已有 GitHub 授权

查看认证和本地状态，输出中不要展示 token。

```shell
gh auth status
git status --short
git remote -v
```

确认当前目录不是用户的其他代码仓库后，创建/复用 fork：

```shell
# 在上一级工作区执行；已有仓库时由 Codex 调整，不重复克隆。
gh repo fork XxMinor/mykvm --clone --remote
```

GitHub CLI 的 remote 调整行为需随后检查；不得假设 `origin` 一定已经指向自己的 fork。[S20]

```shell
cd mykvm
git remote -v
git status --short
```

只在账户、目标和可见性明确时写远端。代码仓库是公开的，不意味着可以把真实设备 IP、token、密钥、私人日志和用户整个工作区一起上传。

### 3.2 暂时没有 GitHub 授权

不需要先停下来等待。可以先本地开发：

```shell
git clone https://github.com/XxMinor/mykvm.git mykvm
cd mykvm
# 新 clone 下将只读上游明确命名，避免误向上游推送。
git remote rename origin upstream
git switch -c feat/mac-first-kvm
```

后续有授权再创建 fork，核验后添加自己的 `origin`。不用为了 fork 把全局 Git/GitHub 设置改掉。

## 4. 记录并固定实际源码版本

```shell
git status --short
git rev-parse HEAD
git show -s --format='%H%n%ci%n%s' HEAD
git remote -v
```

本包审查过的提交：

```text
bb5421fe1d4c0c8c72bb3f6c0c35f0a0f994209b
```

只把它当可核验的参考基线，不宣称是最新稳定版。检查该对象是否存在：

```shell
AUDITED_SHA=bb5421fe1d4c0c8c72bb3f6c0c35f0a0f994209b
git cat-file -e "${AUDITED_SHA}^{commit}"
```

对象不存在时先核验正确 remote 后 fetch；实际选择最新源码或该审查点，都必须记录完整 SHA、差异理由和符号映射。**已有用户改动时不要为了匹配文档执行 reset、checkout 覆盖或强行降级。**

环境/源码记录使用 `templates/ENVIRONMENT.md`、`templates/SOURCE_AUDIT.md`。涉及真实机器信息的记录默认留本地，不自动公开提交。

## 5. 合并项目级执行规则

读取 `templates/AGENTS.project.md`，与现有项目 `AGENTS.md` 合并项目专属规则，不直接覆盖。只有实际安装了 `$sol-multi-agent-development` 才引用它；没有则单主线程顺序执行。

不修改 `~/.codex/AGENTS.md`、全局模型名称或用户级代理配置。项目已有审批规则继续保留，但移除本项目旧文档中的 LOL 硬关卡要求。[S18][S19]

## 6. 基线构建：先看脚本再运行

先读 `package.json`、`scripts/build-tauri-assets.mjs`、Tauri 配置及安装 hooks，确认构建不是偷偷安装服务或运行签名/覆盖应用脚本。

```shell
npm ci
npm run lint
npm run build

cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo check --manifest-path src-tauri/Cargo.toml --locked --lib
cargo test --manifest-path src-tauri/Cargo.toml --locked --lib
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --lib -- -D warnings
```

这些是根据审查快照的入口整理的基线命令，Codex 必须按实际仓库脚本和依赖补齐前置条件，不能因为命令写在文档里就声称一定可成功。

已有 warning/失败先记录。不能通过删除测试、全局允许 warning 或无差别升级锁文件来伪造成功。必要的依赖升级单独任务、单独说明。

### 保留日志及真实退出码

```shell
mkdir -p .local-evidence
# 建议将 .local-evidence 放入本地 exclude，避免公开提交私人日志。
# 示例用 bash，确保管道保留 cargo 的退出状态。
bash -o pipefail -c \
  'cargo test --manifest-path src-tauri/Cargo.toml --locked --lib 2>&1 | tee .local-evidence/mac-lib-tests.log'
```

记录执行命令、退出码和源码 SHA；路径存在不等于测试成功。不要执行要求真实键鼠注入的测试作为默认 cargo test 子集。

## 7. 开发与原生 Mac 预览

设置界面开发入口：

```shell
npm run tauri:dev
```

开发命令本来会显示终端，不应据此认定 release 软件“常驻 CMD”。正式 Windows 无控制台属性上游已经存在，另检查新增辅助进程。[S05]

在 fork updater、标识和脚本副作用隔离完成后，构建 Mac ARM64 测试包：

```shell
npm run tauri:build:mac-arm
```

审查快照中该脚本调用 ARM64 目标的 app/dmg 构建并带 `--no-sign`；实际使用前再次核验脚本。[S09]

从真实输出中找产物，不猜文件名：

```shell
find src-tauri/target -type d -name '*.app' -print
find src-tauri/target -type f -name '*.dmg' -print
```

检查可执行文件架构、文件大小与签名情况：

```shell
# 将 APP_PATH 替换成上一步真实找到的 fork .app 目录。
APP_PATH='/实际路径/MyKVM Local.app'
ls "$APP_PATH/Contents/MacOS"
file "$APP_PATH"/Contents/MacOS/*
codesign -dv --verbose=4 "$APP_PATH" 2>&1
```

`codesign` 报告未签名时如实记录，不为了“通过”自动运行上游的本地签名脚本。该脚本会调整钥匙串与信任设置，并硬编码上游 identifier。[S08]

本地自用可以按当前系统规则使用未公证/ad hoc 测试产物，不要求购买开发者证书。没有 Developer ID 公证不能写成已经公证。不要关闭 Gatekeeper/SIP、不要自动修改整机信任。

## 8. Mac 权限与当前控制工具

真实 Mac 注入/剪贴板测试由用户选择在专用测试场景执行。辅助功能等权限通过系统界面授权，不使用修改 TCC 数据库或关闭 SIP 的方式。

**如果 Deskflow/RustDesk 正在维持你对 Mac 的键鼠控制，不要由 Codex 自动退出或杀掉它。** 默认 fake/loopback 测试不会争夺真实输入，可以继续写代码。只有用户准备好真实测试与本地恢复方式后，才人为切换当前控制工具。

不要在真实终端、IDE、Codex 输入框自动注入测试命令。使用本程序专用测试窗口或用户明确指定的空白文本场景。测试剪贴板前说明会暂时修改内容；只能尽力恢复支持的格式，不能承诺所有应用私有格式完全还原。

## 9. Windows 在哪里构建

首选可用的 Windows 原生 CI/runner，而不是把 M4 上 `cargo build --target x86_64-pc-windows-msvc` 当作万能跨平台打包命令。Tauri 的 Windows 原生构建涉及平台依赖，完整安装器以实际原生构建结果为准。[S21]

工作流最小职责：

```text
检出固定提交
→ 设置并记录 Rust / Node 版本
→ npm ci
→ npm run lint / build
→ cargo check / test（Windows非交互lib部分）
→ unsigned preview EXE / installer
→ 上传构建产物与日志
```

可用原生 Windows 上的命令入口示例：

```powershell
npm ci
npm run lint
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --locked --lib
npm run tauri:build
# 只有 fork 安装 hooks 已审查且签名/更新依赖已隔离后才打安装器。
npm run tauri -- build --bundles nsis
```

CI 配置要求：最小 `contents: read` 权限；明确的手动/代码触发条件；不执行上游公开 release 工作流；不读取发布密钥；不使用需要额外付费的 runner。源码来自 PR 时不得授予写仓库/发布秘密权限。

GitHub Actions 是否可用取决于当前账户、仓库和配额。没有可用免费 CI 时保留原生构建脚本并记录待执行，**不要求买 Windows 虚拟机、Parallels 或付费 runner，也不阻塞 Mac 后续开发。**

Windows CI 不执行 LOL，不需要交互桌面。编译通过不能当成钩子、鼠标捕获或游戏兼容性实测通过。

## 10. 配置、自启与更新

调试阶段默认不自动启用登录项。稳定后由用户明确启用普通用户登录自启，不是系统开机登录前服务。

升级前备份 fork 自己的配置和信任资料；秘密只留本地。不要复制上游 updater 的 pubkey/endpoint 到 fork 后继续自动升级。上游 NSIS、安全桌面 helper、命名管道和发现标识也要核对是否需要隔离。

修改版与上游并存时必须使用不同标识和数据路径；真实输入接管避免两套程序同时启用。

## 11. 最终交付与校验

必须列出：完整 commit、改动摘要、运行的测试、尚未运行项目、实际构建产物、签名/公证状态、安装步骤、已知风险和回滚步骤。

对实际生成的归档/安装包计算校验和：

```shell
shasum -a 256 '/实际路径/安装包.dmg'
```

校验和帮助检查文件完整性，不等同于代码签名或可信发行认证。

交付报告中允许出现：

```text
Mac build: pass
Windows build: pending_environment
Windows runtime: optional_not_run
LOL runtime: optional_not_run
```

这比编造“所有平台通过”正确。没有实际 Windows 产物就不要提供虚构的下载链接。

## 12. 回滚

用户需要回退时：正常结束当前远程会话 → 退出 fork → 禁用 fork 自己的登录项 → 恢复先前保存的 fork 版本/配置。不要删除全局 `.ssh`、Codex、上游 MyKVM、Deskflow 或 Home Assistant 资料。

源码回退使用独立分支或任务提交的 revert；保护工作树中用户改动。真实设备未验证之前保留原可用工具，但不要由 Codex 自行切断用户当前的控制通道。
