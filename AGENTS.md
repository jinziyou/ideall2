# Repository Guidelines

## 项目结构与模块组织

ideall 是基于 Rust 与 GPUI 的本地优先个人信息终端。生产源码位于 `native/`：`apps/ideall-desktop` 是 GPUI 桌面壳，`apps/ideall-mobile` 是 iOS/Android 壳，`apps/ideall-cli` 提供检查和迁移工具；`crates/ideall-protocol` 保持纯契约，`ideall-domain` 负责 Engine/Workspace 规则，`ideall-application` 负责跨 UI 用例，`ideall-presentation` 收口无 GPUI 类型的导航、文案与设计 Token，storage/sync/agent/acp/secrets/updater 是适配层。`src/` 与 `src-tauri/` 只保留旧版 V2 归档导出兼容性，不进入生产发布。Rust 测试位于各 crate 的 `src/` 或 `tests/`，迁移兼容测试以 `*.test.ts` 就近放在 `src/`，维护脚本测试位于 `scripts/*.test.mjs`。文档入口是 `docs/README.md`，API schema 源文件是 `openapi/server.json`。

## 构建、测试与开发命令

- `pnpm app:dev` 或 `pnpm dev`：启动 GPUI 桌面 App。
- `pnpm app:build` 或 `pnpm build`：构建并打包当前桌面平台的 GPUI App。
- `pnpm native:check`、`pnpm native:test`、`pnpm native:lint`：分别运行原生 workspace 检查、测试和 rustfmt/clippy 门禁。
- `pnpm legacy:web:export`：仅为旧数据迁移生成 Next 静态兼容层；它不是生产 App。
- `pnpm lint`、`pnpm lint:deps`、`pnpm lint:dead-code`、`pnpm lint:docs`、`pnpm typecheck`、`pnpm test`：分别运行架构边界、依赖使用、孤立文件、文档链接、TypeScript 与业务测试。
- `pnpm test:coverage`：为选定的核心源码生成 c8 text/lcov 报告，并执行仓库基线覆盖率门槛。
- `pnpm verify:checks`：运行不含生产构建的 CI 质量门禁，包含依赖与孤立文件 lint；`pnpm version:check` 校验四处项目版本一致。
- `pnpm verify:bundle`：检查已有 `out/` 中 JavaScript chunk 的 raw/gzip bundle 预算，应在生产构建后运行。
- `pnpm verify`：等同 `pnpm verify:base`，依次运行 `verify:checks`、生产构建和 `verify:bundle`。

## 代码风格与命名约定

Rust 使用 edition 2024，依赖方向固定为 `protocol → domain → application → adapters → UI → platform app`；共享 crate 的公开 API 不得暴露 GPUI 类型。桌面与移动使用各自经验证的 GPUI revision，不直接共享 UI 类型。运行 `cargo fmt` 格式化，clippy 警告视为错误。旧兼容层继续使用 TypeScript strict、`@/*` 与 `@protocol/*` 路径别名，并保持现有 ESLint 架构边界。

## 测试指南

业务测试通过 `scripts/run-tests.mjs` 使用 `node:test`，文件命名遵循 `*.test.ts`。运行全部业务测试用 `pnpm test`；也可传入子串过滤，例如 `pnpm test sort-key`。维护脚本测试运行 `pnpm test:scripts`。修改 protocol 逻辑、store、同步、安全守卫、provider 或部署脚本时，应补充聚焦测试。

## 提交与 Pull Request 指南

近期提交使用 Conventional Commit 前缀，如 `feat:`、`fix:`、`chore:`、`ci:`。标题保持简洁、动作明确，例如 `fix: guard workspace hydration`。日常开发面向 `dev`，`main` 用于稳定发布。PR 应说明行为变化，关联相关 issue，列出已运行的验证命令；涉及 UI 时附截图。

## 安全与配置提示

本地配置可复制 `.env.example` 为 `.env.local`；不要提交密钥或本地端点。schema 变更后用 `pnpm gen:api` 重新生成 API 类型，并确保 DTO 使用仍限制在服务端适配器边界内。
