# 脚本与验证入口

根目录 pnpm 只统一调用 Rust 工具链和少量 Node 维护脚本；产品运行时不依赖 Node。

## 原生应用

| 命令 | 用途 |
| --- | --- |
| `pnpm app:dev` / `pnpm dev` | 启动 GPUI 桌面 App |
| `pnpm app:build` / `pnpm build` | Release 构建并打包桌面 App |
| `pnpm native:check` | 检查 workspace 全 targets 与移动 host-check |
| `pnpm native:test` | 运行全部 Rust 测试 |
| `pnpm native:lint` | rustfmt 与 clippy `-D warnings` |
| `pnpm native:android*` | Android APK/AAB 入口 |
| `pnpm native:ios:*` | iOS simulator/device/archive/IPA 入口 |

## 仓库门禁

| 命令 | 用途 |
| --- | --- |
| `pnpm lint:actions` | 校验 GitHub Actions |
| `pnpm lint:docs` | 校验 Markdown 相对链接 |
| `pnpm version:check` | 校验 Cargo 与平台包版本 |
| `pnpm test:scripts` | 隔离运行 `scripts/*.test.mjs` |
| `pnpm verify:checks` | 运行格式、文档、Actions、版本与脚本门禁 |
| `pnpm verify` | 在轻量门禁后运行完整原生 check/lint/test |

`test:scripts` 支持用文件名子串过滤：

```bash
pnpm test:scripts -- native-release
pnpm test:scripts -- native-mobile
```

## 发布与目录

- `scripts/native-release-artifacts.mjs`：整理、校验和签名 GPUI 五平台产物。
- `scripts/lib/minisign.mjs`：原生更新产物使用的 Minisign key-id 校验。
- `scripts/bump-version.mjs` / `check-version.mjs`：维护多平台版本一致性。

生产发布流程见 [native/RELEASE.md](../native/RELEASE.md)。旧 Web/Tauri 的构建、静态导出、
Playwright 冒烟和 OpenAPI TypeScript codegen 只保留在 `legacy/tauri-v2` 分支。
