# ideall

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

ideall 是一个开源、本地优先的个人信息终端。它把笔记、文件、书签、关注源和 AI
协作收进同一个原生工作台；无账号、无网络时，本地核心仍然可用。

当前 `dev` 与 `main` 只包含 Rust + GPUI 实现：

- macOS、Windows、Linux：GPUI + gpui-component；
- iOS、Android：GPUI + 仓库固定版本的 gpui-mobile；
- 五个平台共享 protocol、domain、application、SQLite、同步、Agent 与安全存储边界。

停止维护的 Next.js/Tauri V2 已完整冻结在 `legacy/tauri-v2` 分支，不参与默认分支的构建、
依赖、测试或发布。需要导出旧数据时，请在独立目录检出该分支；不要让两个版本共用数据目录。

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
│   ├── ideall-sync-http/
│   ├── ideall-agent/
│   ├── ideall-acp/
│   ├── ideall-secrets/
│   ├── ideall-updater/
│   └── ideall-webmeta/       # 书签页面的有界 HTML 元数据抓取
└── vendor/gpui-mobile/
```

依赖方向固定为：

```text
protocol → domain → application → adapters → UI → platform app
```

共享核心不暴露 GPUI、SQLite、HTTP 或平台系统类型。

## 快速开始

需要 Rust 1.96。Linux 还需要 Fontconfig、FreeType、XCB、xkbcommon-x11 和 Vulkan loader
开发包，完整说明见 [native/README.md](native/README.md)。

```bash
pnpm install
pnpm app:dev
pnpm app:build
pnpm native:check
pnpm native:test
pnpm native:lint
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

## 旧数据迁移

GPUI App 不直接读取旧 WebView 的 IndexedDB。迁移通过 V2 工作区归档完成：

1. 在 `legacy/tauri-v2` 的独立检出中导出归档；
2. 原生 importer 校验 envelope、密码派生、AES-GCM、CRC32、引用与体积预算；
3. 在临时数据库中完成单事务导入；
4. 成功后原子替换目标数据库并保留导入前备份；
5. API Key、同步码和登录 token 不进入归档。

## 验证与发布

```bash
pnpm verify            # 仓库维护门禁 + 原生 fmt/clippy/test
pnpm version:check     # Cargo、桌面包与移动包版本一致性
pnpm test:scripts      # 发布与仓库维护脚本测试
```

- `.github/workflows/rust.yml` 验证共享核心、三桌面平台、Android 模拟器与 iOS Simulator。
- `.github/workflows/native-release.yml` 生成并验证桌面安装包、AAB、iOS archive 和签名更新清单。
- `.github/workflows/ci.yml` 验证文档、Actions、版本与维护脚本。

真实发布仍需各平台签名身份，以及中文 IME、候选窗、选区、VoiceOver/TalkBack 和安装升级
人工矩阵。证据与剩余门禁见 [平台支持状态](docs/platform-support.md)和
[原生发布手册](native/RELEASE.md)。

## 文档

- [原生开发与平台说明](native/README.md)
- [原生发布与签名](native/RELEASE.md)
- [架构说明](docs/architecture.md)
- [FileSystem 与 Engine 架构](docs/file-system-engine-architecture.md)
- [桌面 UI 规范](docs/design/ui-style.md)
- [文档导航](docs/README.md)
- [安全策略](.github/SECURITY.md)

## 许可与商标

- 源码：[Apache License 2.0](LICENSE)
- 原生第三方依赖许可：[native/THIRD_PARTY_LICENSES.md](native/THIRD_PARTY_LICENSES.md)
- Wonita 名称与官方标识不随 Apache 2.0 许可转让
