# Repository Guidelines

## 项目结构与模块组织

ideall 是基于 Rust 与 GPUI 的本地优先个人信息终端。生产源码全部位于 `native/`：
`apps/ideall-desktop` 是 GPUI 桌面壳，`apps/ideall-mobile` 是 iOS/Android 壳，
`apps/ideall-cli` 提供检查和迁移工具；`crates/ideall-protocol` 保持纯契约，
`ideall-domain` 负责 Engine/Workspace 规则，`ideall-application` 负责跨 UI 用例，
`ideall-presentation` 收口无 GPUI 类型的导航、文案与设计 Token，其余 storage、sync、
agent、acp、secrets、updater、webmeta 是适配层。Rust 测试位于各 crate 的 `src/` 或 `tests/`，
维护脚本测试位于 `scripts/*.test.mjs`。文档入口是 `docs/README.md`。

旧 Next.js/Tauri V2 只存在于 `legacy/tauri-v2` 分支。`dev` 与 `main` 不得重新引入 WebView
生产壳、前端运行时依赖或旧发布入口。

## 构建、测试与开发命令

- `pnpm app:dev` 或 `pnpm dev`：启动 GPUI 桌面 App。
- `pnpm app:build` 或 `pnpm build`：构建并打包当前桌面平台的 GPUI App。
- `pnpm native:check`、`pnpm native:test`、`pnpm native:lint`：分别运行原生 workspace
  检查、测试和 rustfmt/clippy 门禁。
- `pnpm test:scripts`：运行维护与发布脚本测试。
- `pnpm lint:docs`、`pnpm lint:actions`、`pnpm version:check`：检查文档链接、GitHub
  Actions 与多平台版本一致性。
- `pnpm verify:checks`：运行轻量仓库门禁；`pnpm verify` 继续执行完整原生门禁。

## 代码风格与命名约定

Rust 使用 edition 2024，依赖方向固定为
`protocol → domain → application → adapters → UI → platform app`；共享 crate 的公开 API
不得暴露 GPUI 类型。桌面与移动使用各自经验证的 GPUI revision，不直接共享 UI 类型。
运行 `cargo fmt` 格式化，clippy 警告视为错误。Node 脚本只用于仓库维护、产物验证与发布，
不得承载产品运行时。

## 测试指南

修改 protocol、domain、storage、同步、安全守卫、provider 或发布脚本时，应补充聚焦测试。
Rust 测试使用 `cargo test --locked --workspace`；维护脚本使用 Node 内置 `node:test`，
文件命名遵循 `scripts/*.test.mjs`。

## 提交与 Pull Request 指南

使用 Conventional Commit 前缀，如 `feat:`、`fix:`、`refactor:`、`chore:`、`ci:`。标题保持
简洁、动作明确。日常开发面向 `dev`，`main` 用于稳定发布。PR 应说明行为变化，关联相关
issue，列出已运行的验证命令；涉及 UI 时附截图。

## 安全与配置提示

原生应用不会自动加载 `.env.local`；本地覆盖通过启动进程的环境变量传入。不要提交密钥、
本机数据目录或私有服务端点。凭据只进入平台安全存储，不得写入 SQLite、归档或日志。
