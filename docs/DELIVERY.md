# 首批实现交付（非完整产品验收）

已实际完成 T00/T01，并提交身份隔离、假平台接口、路由核心、安全入口限制及原生 CI 脚本。分支为 `feat/mac-first-kvm`，完整受测代码 SHA 为 `83604777ef3ff49ab95821f9b0b03b74dbbc8fb3`。本报告之后的记录提交不修改代码。

提交历史可用 `git log --oneline a2ea4164861de31b562c8417eeb7879dbc8c23cb..HEAD` 查看。当前状态见 PROGRESS.md，测试与局限见 TEST_REPORT.md，逐任务依据见 docs/handoff。

## 开发使用

在仓库根目录执行 `../source/with-rust.sh node scripts/check-native.mjs`。依赖已在当前项目安装，工具链留在外层 `.toolchain/`。不要通过旧 install/sign 脚本安装；这些路径已禁用。当前开发版旧 LAN 功能关闭，尚不可替代现有远控工具。

## 产物与未验证项

没有生成 app、dmg、exe 或安装包，因此没有安装包路径或校验和。前端静态构建成功不等于应用交付。没有真实权限授权、安装覆盖、钥匙串操作或公开发布，未停止 Deskflow/RustDesk。

T05.b 连接授权、T06 协议、T04.b 真实接入及其余任务仍待完成。Windows 编译和实机、LOL 实机、Mac 运行、性能与完整集成均未验证，不保证游戏兼容或绝对零 GPU。

## 回滚

所有改动在新项目的本地分支中，原有应用和系统安装未被替换。保留该目录即可保留全部证据。需要查看原版时，在确认工作树干净后创建独立 worktree：`git worktree add --detach ../mykvm-baseline a2ea4164861de31b562c8417eeb7879dbc8c23cb`。若要撤销单项实现，先检查 `git status` 和对应 diff，再使用 `git revert <commit>`；存在依赖时按逆序撤销并重新运行检查，不使用强制 reset 或覆盖未提交改动。
