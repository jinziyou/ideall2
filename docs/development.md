# 开发指南

默认分支只包含 Rust + GPUI 产品实现。旧 Next.js/Tauri V2 位于 `legacy/tauri-v2`，不得把
其运行时依赖、构建脚本或 WebView 壳重新合入 `dev`/`main`。

## 目录

```text
native/
├── apps/                     # desktop、mobile、cli
├── crates/                   # protocol、domain、application 与适配器
├── scripts/                  # 原生打包、验证与平台脚本
└── vendor/gpui-mobile/       # 固定 revision 的移动 UI 快照

scripts/                      # 仓库版本、发布和 CI 维护脚本
docs/                         # 架构、设计与操作手册
```

## 环境

- Rust 1.96
- Node.js 22 与 pnpm 9（仅用于统一命令、CI 和发布脚本）
- Linux：Fontconfig、FreeType、XCB、xkbcommon-x11、Vulkan loader 开发包
- Android：SDK、NDK、cargo-ndk、Gradle
- iOS：macOS、Xcode、XcodeGen

原生应用不会自动读取 `.env.local`。诊断覆盖通过启动进程的环境变量传入；密钥不得写入
仓库文件或命令行参数。

## 常用命令

```bash
pnpm install
pnpm app:dev
pnpm app:build
pnpm native:check
pnpm native:test
pnpm native:lint
pnpm verify
```

移动构建：

```bash
pnpm native:android
pnpm native:android:bundle
pnpm native:ios:simulator
pnpm native:ios:device
```

## 依赖与边界

依赖方向固定为：

```text
protocol → domain → application → adapters → UI → platform app
```

- protocol/domain 不依赖 GPUI、SQLite、HTTP 或平台 API。
- presentation 可以提供文案、Token 和纯状态，但不暴露 GPUI 类型。
- desktop 与 mobile 分别使用经过验证的 GPUI revision，不直接共享 UI 类型。
- UI 通过 application service 修改数据，不能直接访问 SQLite。
- Agent 数据访问必须经过 Grant → MCP/FileSystem。

新增依赖时优先放在最窄的 crate；GPUI Git/revision 变更应单独提交，并运行全平台构建矩阵。

## 测试与格式

```bash
cd native
cargo fmt --all
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets --exclude ideall-mobile -- -D warnings
cargo clippy --locked -p ideall-mobile --features mobile-ui-host-check -- -D warnings
```

根目录入口会额外覆盖移动 host-check 和维护脚本：

```bash
pnpm native:lint
pnpm test:scripts
pnpm lint:docs
pnpm lint:actions
pnpm version:check
```

Rust 测试放在 crate 的 `src/` 或 `tests/`；Node 维护脚本测试命名为
`scripts/*.test.mjs`。修改协议、存储、同步、安全边界或发布脚本时补充聚焦测试。

## 分支与提交

日常开发以 `dev` 为默认分支，`main` 只接收稳定发布变更。提交使用 Conventional Commit，
如 `feat:`、`fix:`、`refactor:`、`ci:`。涉及 UI 的 PR 应附截图并列出验证命令。

需要排查旧数据导出时，在另一个工作目录检出 `legacy/tauri-v2`；不要在当前工作树临时恢复
`src/` 或 `src-tauri/`。
