# ideall

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

ideall 是一个开源、本地优先的个人信息终端。它把笔记、文件、书签、关注源和 AI
协作收进同一个原生工作台；无账号、无网络时，本地核心仍然可用。

当前 App 已以 Rust + GPUI 为唯一生产实现：

- macOS、Windows、Linux：GPUI + gpui-component；
- iOS、Android：GPUI + 仓库固定版本的 gpui-mobile；
- 五个平台共享 protocol、domain、application、SQLite、同步、Agent 与安全存储边界；
- Next.js/Tauri 代码仅保留为旧数据的 V2 归档导出兼容层，不再产生生产 App。

## 产品结构

```text
Storage → FileSystem → FileRef → Engine → Display
```

- `FileRef` 是内容的稳定身份，路径只是投影。
- 同一文件可由笔记、预览、代码、浏览器等不同 Engine 打开。
- 桌面使用“导航轨道 → 资源上下文 → 多标签工作台”，移动端使用导航栈和底部栏。
- Agent 只能经 Grant → MCP/FileSystem 能力链读写，并为写操作留下脱敏审计。
- 笔记、书签、关注和同步数据默认位于本机 SQLite；同步只上传端到端加密密文。
- 模型 Key、同步码和登录凭据只进入平台安全存储，不写入 SQLite、归档或日志。

## 仓库结构

```text
native/
├── apps/
│   ├── ideall-desktop/       # GPUI 桌面 App
│   ├── ideall-mobile/        # GPUI iOS / Android App
│   └── ideall-cli/           # 数据检查、归档迁移与服务探测
├── crates/
│   ├── ideall-protocol/
│   ├── ideall-domain/
│   ├── ideall-application/
│   ├── ideall-presentation/  # 无 GPUI 类型的导航、文案与设计 Token
│   ├── ideall-storage/
│   ├── ideall-sync/
│   ├── ideall-agent/
│   ├── ideall-acp/
│   ├── ideall-secrets/
│   └── ideall-updater/
└── vendor/gpui-mobile/

src/ + src-tauri/              # 旧版 V2 归档导出兼容层，不进入原生发布
```

依赖方向固定为：

```text
protocol → domain → application → adapters
                    presentation → UI → platform app
```

共享核心不暴露 GPUI、SQLite、HTTP 或平台系统类型。

## 快速开始

需要 Rust 1.96。Linux 还需要 Fontconfig、FreeType、XCB、xkbcommon-x11 和 Vulkan loader
开发包，完整说明见 [native/README.md](native/README.md)。

```bash
git clone git@github.com:jinziyou/ideall.git
cd ideall

pnpm app:dev          # 启动 GPUI 桌面 App
pnpm app:build        # 构建并打包当前桌面平台
pnpm native:check     # 检查整个原生 workspace
pnpm native:test      # 运行共享核心与原生应用测试
pnpm native:lint      # rustfmt + clippy -D warnings
```

也可以直接使用 Cargo：

```bash
cd native
cargo run --locked -p ideall-desktop
cargo test --locked --workspace
cargo run --locked -p ideall-cli -- doctor --database /path/to/ideall.db
```

移动端入口：

```bash
pnpm native:android
pnpm native:android:bundle
pnpm native:ios:simulator
pnpm native:ios:device
```

Android 需要 SDK、NDK、cargo-ndk 与 Gradle；iOS 需要 macOS、Xcode 与 XcodeGen。

## 旧版数据迁移

GPUI App 不直接读取旧 WebView 的 IndexedDB。迁移流程保持可恢复：

1. 用旧兼容层导出明文或加密 V2 工作区归档；
2. 原生 importer 校验 envelope、密码派生、AES-GCM、CRC32、引用与体积预算；
3. 在临时数据库中完成单事务导入；
4. 成功后原子替换目标数据库并保留导入前备份；
5. API Key、同步码和登录 token 不进入归档。

仅在需要旧数据导出或 wire 回归时安装 Node.js 22 + pnpm 9：

```bash
pnpm install
pnpm legacy:web:export
pnpm verify:smoke:static
```

旧 Tauri 壳不再发布，也不得与 GPUI App 共用数据目录。

## 验证与发布

```bash
pnpm verify            # TypeScript 迁移兼容门 + 原生 fmt/clippy/test
pnpm version:check     # 原生 Cargo、桌面包与移动包版本一致性
pnpm gen:api:check     # 后端 OpenAPI wire 漂移检查
```

- `.github/workflows/rust.yml` 验证原生共享核心、三桌面平台、Android 模拟器与 iOS Simulator。
- `.github/workflows/native-release.yml` 生成并验证桌面安装包、AAB、iOS archive 和签名更新清单。
- `.github/workflows/smoke.yml` 只验证旧版数据导出兼容性，不产生发布 App。

真实发布仍需各平台签名身份，以及中文 IME、候选窗、选区、VoiceOver/TalkBack 和安装升级
人工矩阵。证据与剩余人工门禁见
[GPUI 迁移记录](docs/design/gpui-migration.md)和
[原生发布手册](native/RELEASE.md)。

## 后端与隐私

本地核心不依赖后端。wonita 是资讯、社区与加密同步的默认服务实现，但客户端通过自有
契约消费服务，不绑定单一提供方。启用同步、模型、发布或外部 Agent 时，界面会按各能力
边界产生明确出站；详见[安全策略](.github/SECURITY.md)。

OpenAPI schema 权威源为 `openapi/server.json`：

```bash
pnpm gen:api
SERVER_LOCAL=/abs/path/to/openapi.json pnpm sync:api
```

## 文档

- [原生开发与平台说明](native/README.md)
- [原生发布与签名](native/RELEASE.md)
- [架构说明](docs/architecture.md)
- [FileSystem 与 Engine 架构](docs/file-system-engine-architecture.md)
- [GPUI 迁移决策与证据](docs/design/gpui-migration.md)
- [文档导航](docs/README.md)
- [安全策略](.github/SECURITY.md)

## 许可与商标

- 源码：[Apache License 2.0](LICENSE)
- 原生第三方依赖许可：[native/THIRD_PARTY_LICENSES.md](native/THIRD_PARTY_LICENSES.md)
- Wonita 名称与官方标识不随 Apache 2.0 许可转让
