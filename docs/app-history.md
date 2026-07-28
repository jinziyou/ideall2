# ideall：App 历史路线图

本文件记录 App-only / Tauri 化过程中的阶段性决策与迁移结果。当前开发、构建、发布与签名流程以 [app.md](app.md) 为准。

## ✅ Phase 0 — 基础骨架

- Tauri 2.0 工程 `src-tauri/`（dev 壳已可 `pnpm app:dev` 加载 SSR 开发服）。
- `next.config.ts` 默认 `output: export`（App-only 唯一生产构建）。
- 客户端取数所需 env 接通：`NEXT_PUBLIC_SERVER_ADDR`，`lib/env.ts` 改为**同构**读取（`pnpm dev` SSR 渲染期服务端用 `SERVER_ADDR`，App 客户端用 `NEXT_PUBLIC_SERVER_ADDR`）。
- gitignore 覆盖 `src-tauri/target`、`src-tauri/gen`、`out/`。

## ✅ Phase 1 — 数据层客户端化（`app:export` 已绿）

静态导出不支持 Server Actions / Route Handlers / 请求时 `headers()` / 任意动态路径段，已全部改造：

- 5 个 `"use server"` 文件去指令 → **同构数据访问**（`pnpm dev` SSR 渲染 / app 客户端直连后端共用）：
  现分别为 `community/data`、`info/data`、`lib/auth/auth-api`、`lib/peer-api`、`plugins/sync/lib/sync-api`（已由 `*-action` 改名）。
  （`apiFetch` 本就同构、token 走 localStorage、`SERVER_ADDR` 已同构，故迁移机械低风险。）
- `community/page` async Server Component → 客户端组件（`useEffect` 取数）；去 `force-dynamic`。
  `getVisitorLocation` 暂返回 null（客户端无法读自身公网 IP）→ 地图回退全国，可手动切城市。
- 动态路径段 → **查询参数路由**：`/info/entity/[label]/[name]` → `/info/entity?label=&name=`；
  `/info/publisher/[domain]` → `/info/publisher?domain=`（`useSearchParams` + `Suspense`）。
- `info/analysis` (`await searchParams`) → 客户端 `useSearchParams` 取数。
- **Agent**：删服务端代理 Route Handler，`agent-chat` 改客户端直连 OpenAI 兼容端点（BYO key 留本地）。
  ⚠️ 浏览器直连受厂商 CORS 限制（本地 Ollama / 放行 CORS 的端点可用）；桌面/移动 App 已接 Tauri HTTP 插件（`@tauri-apps/plugin-http`，Rust 侧请求绕过 CORS）即可全端点可用。

> 验证（Phase 1 当时快照）：`pnpm build`（= `app:export`，21 静态页）✓ · lint ✓ · typecheck ✓ · test 12/12 ✓（测试集此后已扩展，当前命令见 [development.md](development.md)）
> 后端侧：后端数据服务（如 wonita 的 server）的 `CORS_ALLOW_ORIGINS` 需放行 app 来源（`tauri://localhost` / 开发期 `http://localhost:5020`）。

## ✅ Phase 2 — 平台构建与 CI（桌面 + Android 已接，iOS 待证书）

- **图标**：已用品牌字形生成全平台图标集到 `src-tauri/icons/`（桌面 `.ico`/`.icns`/png + Windows Store + Android/iOS）。重做：`pnpm tauri icon <1024.png>`。
- **CI**：`.github/workflows/app-build.yml`（触发：push `main` / `app-v*` tag / 手动）：
  - `gate` 前置 job：tag / 手动总构建；`main` 推送仅当含「非文档/配置」改动才构建（纯文档、`*.md`、`.github/`、`.prettier*` 等提交跳过全平台构建，省 CI）。
  - 桌面 matrix（macOS arm64/x64、Linux、Windows）经 `tauri-apps/tauri-action` 出安装包；先 `create-release` 建唯一 Release、矩阵 `releaseId` 仅上传（规避并发各自建滚动 tag 的竞态）。
  - 两条发布渠道：**push `main` → 滚动预发布 `app-edge`**（永远是 main 最新、直接可下载）；**push `app-v*` tag → 正式 Release 草稿**（人工 review 后发布）。
  - Android：仅 tag/手动时 `tauri android init` + `tauri android build --debug --apk` → 上传 debug APK 工件。
  - `checksums` job：桌面包上传后生成 `SHA256SUMS` 挂回同一 Release（供下载校验完整性）。
  - 静态导出地址由仓库变量注入（未设回退生产默认）：`NEXT_PUBLIC_SERVER_ADDR`（数据服务 apiserver）、`NEXT_PUBLIC_EMBED_BASE`（info/community 内嵌的 wonita/portal）。
- **iOS**：需 Apple 开发者证书/描述文件，未纳入 CI。本地：`pnpm tauri ios init && pnpm tauri ios build`（macOS + Xcode）。

## ✅ Phase 3 — HTTP 插件 / 自动更新（已启用）/ OS 代码签名（待证书）

- **HTTP 插件（已接，功能完整）**：`tauri-plugin-http` 已装 + 授权（`capabilities/default.json` 放行任意 `http/https`），`agent-chat` 在 Tauri 内经插件 `fetch`（Rust 侧请求）绕过 webview CORS → **App 内 agent 可直连任意 LLM 端点（含 OpenAI 等云厂商）**；`pnpm dev` 浏览器内调试时仍受厂商 CORS 限制（用本地 Ollama / 放行 CORS 的端点）。
- **自动更新（桌面，已启用）**：`tauri-plugin-updater` 已装、`lib.rs` 据 `plugins.updater` 配置条件挂载、桌面能力 `capabilities/desktop.json`（移动端走应用商店）；JS 侧 `lib/updater.ts#checkForUpdate()`，入口在命令台「检查更新」。
- **OS 代码签名 / 公证（待证书，当前出未签名包）**：未签名时 macOS 被 Gatekeeper 拦（需右键→打开）、Windows 触发 SmartScreen「未知发布者」。
