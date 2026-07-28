# ideall

> [!CAUTION]
> **冻结归档：Tauri V2**
>
> 本分支只保留旧版 V2 数据导出与迁移参考，不再接收功能、依赖或安全维护，也不进入生产
> 发布。当前 GPUI 实现与日常开发位于 `dev`，稳定发布位于 `main`。此归档的最终快照同时
> 标记为 `tauri-v2-final`。

[![License: Apache 2.0](<https://img.shields.io/badge/License-Apache%202.0-blue.svg>)](LICENSE)

**功能定位：个人信息终端**

**设计风格：现代 · 面板 · 留白**

> 0.2 会在首次启动时自动清理旧版本地状态；升级前请先阅读 [0.2 数据与扩展基线](docs/release-0.2.md)。

**设计思想：借鉴 Linux“一切皆文件”的思想，将能力统一抽象为 Storage、FileSystem、IdeallFile、Engine 与 Display。不同存储来源（本地、远端、内置 App、第三方 App）经文件系统汇聚为 ideall 文件；同一文件可在音频、开发等场景中由不同 Engine 渲染为不同视图。左侧导航栏和活动栏相当于符号链接，例如“我的”链接到 `/home`，“关注”和“书签”分别对应 `/home` 下的目录文件。**

核心能力：AI Agent、Shell、浏览器、本地文件与远程文件，汇聚数据、链接与 AI。

## 项目组成

| 组件             | 说明                                                                                                 |
| ---------------- | ---------------------------------------------------------------------------------------------------- |
| **ideall** | 项目本体：以 Windows / Linux / macOS / Android / iOS 为目标的 Tauri App，管理本地数据，并连接 AI、应用与 Web |
| **wonita** | 数据服务（独立项目）：数据同步、资讯与社区；提供默认后端实现                                         |

## 统一文件系统

ideall 在底层不按数据来源拆成多套文件系统。IndexedDB、Blob、远端服务、内置 App 和第三方 App 都作为独立存储来源，经文件系统挂载到一个隐藏的合成根；Display 层用固定五分区组织它们，工作区则负责同一文件的场景化显示。

```text
Storage → FileSystem → IdeallFile → Engine → Display
```

- 活动栏的一级入口始终固定为五个分区；选中分区后，左侧二级侧栏展示对应入口。App 在授权后贡献的动态挂载也会汇入同一导航体系。
  | 一级分区       | 二级入口               |
  | -------------- | ---------------------- |
  | **我的** | 关注、书签、资源、文件 |
  | **活动** | 空间、任务、删除       |
  | **浏览** | 新闻、社区、浏览器     |
  | **应用** | 搜索、本地应用         |
  | **设置** | 基本、AI               |
- 工作区即视图，通过全局命令面板（`⌘K` / `Ctrl+K`，或顶栏搜索入口）切换，不作为左侧导航项：**文件**（默认文件视图 + AI Agent）、**音频**（文件视图 + 音频播放）、**开发**（文件视图 + Git / 数据库 / Shell）。音频和开发工具显示在工作区 Dock 中，关闭 Dock 即返回文件工作区。切换工作区保留已有标签和脏草稿；若当前打开的是文件，会激活同一 `FileRef` 在新场景下的默认 Engine 标签。
- 文件使用稳定 `FileRef` 寻址，路径和目录项只是投影。同一书签可同时出现在“书签”和“浏览器”中而不复制数据。
- 普通打开由默认引擎在当前标签页显示；“打开方式”选择其他引擎时创建同一文件的独立 Engine 标签。只有文件 capability 与 Engine 策略同时允许时，才可另开独立 App 窗口。
- 默认引擎支持按工作区、单文件或内容类型配置；任意“文件 + 引擎”都可设为首次启动界面。正常启动优先恢复上次关闭的工作现场。
- 音频和 Code 是针对同一文件的场景化引擎；Git、数据库与 Shell 是保持挂载的工作区工具。精确命令路由 `/audio`、`/git`、`/database`、`/shell` 只切换对应 Workspace Dock，不创建标签；完整 App surface 通过 `FileRef + Engine` 深链打开。0.2 工作区持久化只接受当前 `FileRef + Engine` 标签身份。
- 普通领域 CRUD 即使经兼容 `FilesPort` 调用，也会进入 FileSystem registry；只有需要墓碑全量快照与原子批处理的同步流程使用独立的窄 `StorageSyncPort`。可信运行时扩展可把 FileSystem 与 Engine/renderer 作为组合贡献原子安装、卸载；package 重放记录绑定版本、内容/权限摘要与宿主恢复的 consent receipt，不从存储执行代码。桌面端支持官方内置及用户核对指纹后导入的第三方 Minisign publisher 根、单调累积签名撤销清单、当前/下一密钥双签名计划轮换，以及签名 envelope 的安装、更新、回滚和物理卸载；轮换后的旧指纹永久退役，旧签名包不会继续运行。固定官方联网 Registry 的分页页逐页验签并缓存原始信封；生产发布链复用 updater 根在 Actions 中签名并经 staging Release 原子切换，API 服务只代理公开信封而不持有私钥。已安装扩展的更高版本可由用户触发安全下载，经过 DNS/IP 钉连、包 SHA、publisher、manifest 与权限差异复验后再确认安装，旧授权不会继承。包在发现、授权和每次启动都会复验根、撤销状态、manifest 与 MCP stdio connector 摘要，不向 webview 加载包代码。完整 receipt 进入系统凭据库，公开快照只有不可自证的 receipt id；未注入 verifier 或非桌面环境均 fail closed。详见[签名运行时扩展包](docs/runtime-extension-packages.md)。

完整契约、权限和 0.2 数据基线说明见 [文件系统与引擎架构](docs/file-system-engine-architecture.md)、[architecture.md](docs/architecture.md)以及 [0.2 数据与扩展基线](docs/release-0.2.md)。

## 开源客户端与后端服务

本仓库（**ideall**）是 **Apache 2.0** 开源的客户端，可自由使用、修改、自托管。后端取数经 `ServerPort` 契约消费——**ideall 不被任何单一后端绑死**，wonita 只是默认与参考实现。

|      | ideall（本仓库）                                        | wonita 后端服务（默认，可换）                     |
| ---- | ------------------------------------------------------- | ------------------------------------------------- |
| 内容 | Next.js 前端、本地 home、插件、BYO-key agent            | 采集、NLP、知识图谱、实体/事件追踪、鉴权 API      |
| 许可 | [Apache 2.0](LICENSE)                                    | 闭源，官方运营（也可自行实现`ServerPort` 替换） |
| 角色 | 独立可用的本地终端                                      | 可选的「语料级智能」增强                          |
| 商标 | **Wonita** 名称与官方标识不随 Apache 2.0 许可转让 | **Wonita** 品牌与官方数据                   |

**要点：**

- **本地能力（home / apps / tool / agent）不依赖后端**——离线、无账号、数据存在你设备上（本地应用启动器仅在 Tauri 桌面端可用）。跨端同步同样无账号且只上传密文，但执行同步仍需可用的 Sync 服务；当前同步范围是关注、笔记、书签与收藏夹。
- **agent 是横跨本地的 AI 环境层**：可用 BYO-key 直连 OpenAI 兼容端点，也可在桌面端选择用户配置的外部 ACP CLI Agent 作为对话后端；还能连接外部 MCP、联网搜索 / 抓取网页（出站受 egress 守卫约束），并把 ideall 经 ACP 暴露给编辑器。外部 CLI 是当前 OS 用户权限下的本机进程，不是沙箱。
- **需要聚合 / 知识图谱时**：默认连接 [wonita 后端](#连接后端)；也可换用自建后端或自行实现 `ServerPort`。
- **请勿**用 Wonita 商标对外提供竞争性信息服务，或冒充官方 Wonita / ideall 网络。

本仓库是该客户端的源码权威仓库。本地数据不会被自动整库上传；同步、发布、模型、外部 MCP 与联网工具会在用户启用相应能力时按各自边界出站，详见[数据出站矩阵](.github/SECURITY.md#数据出站矩阵)。

ideall **仅以 App 形态分发**：同一套 Next.js 代码经 **Tauri 2.0** 静态导出后，以 Windows / Linux / macOS 桌面和 iOS / Android 移动端为目标。当前 CI 自动构建并发布桌面包；Android 仅在 tag / 手动任务中生成 debug APK 工件，iOS 尚未纳入 CI。无 Node 运行时、无 SSR 生产服务端，客户端直连后端数据服务。详见 [App（桌面 / 移动）](#app桌面--移动) 与 [docs/app.md](docs/app.md)。

## 开发环境

日常 Web 开发需要 **Node.js ≥ 22** + **pnpm 9**（见 `package.json` 的 `packageManager`）。Tauri 桌面 / 移动构建还需要 Rust 与平台系统依赖，详见 [docs/app.md#开发环境--前置依赖](docs/app.md#开发环境--前置依赖)。

## 快速开始

```bash
git clone git@github.com:jinziyou/ideall.git
cd ideall
cp .env.example .env.local   # 按需选择官方或自托管后端地址
pnpm install
pnpm dev                     # 浏览器开发：启动 http://localhost:5020
# 或
pnpm app:dev                 # 桌面开发：复用或启动 Next 开发服，再启动 Tauri 壳
```

## 本地验证

```bash
pnpm verify:checks        # 不含构建：format / lint（代码、workflow、依赖、死文件、文档）/ version / typecheck / test / API drift
pnpm verify:base          # verify:checks → app:export（构建 + 静态入口 + bundle 预算）
pnpm verify:smoke:static  # 生产形态冒烟：app:export → serve out/ → notes/files/plugins/trash
pnpm verify:full          # 基础门禁 + 开发服冒烟（自动挑 5020-5023 可用端口）
```

依赖使用和孤立文件检查可分别运行 `pnpm lint:deps`、`pnpm lint:dead-code`；`pnpm test:coverage` 生成 c8 text/lcov 报告并执行核心层覆盖率基线，它也是 `verify:checks` 的业务测试步骤。`pnpm app:export` 是生产导出的统一入口，会依次构建、检查关键静态入口并验证 JavaScript bundle 预算；已有 `out/` 时仍可单独运行 `pnpm verify:static-export` 或 `pnpm verify:bundle`。使用 `node scripts/verify-static-smoke.mjs --no-build smoke:notes` 可只跑指定冒烟脚本。脚本入口、参数与新增脚本约定见 [docs/scripts.md](docs/scripts.md)。

### 连接后端

开箱默认连官方 wonita（`https://api.wonita.link` + iframe `https://www.wonita.link`），见 [`.env.example`](.env.example)。home / tool 不依赖后端；info / community 需可用后端（经 `ServerPort` 契约；wonita 是默认实现）。

| 场景                          | 做法                                                                                                                                                                                                                                                                                    |
| ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **独立运行 / 官方后端** | 无需配置，或复制`.env.example` → `.env.local`                                                                                                                                                                                                                                      |
| **联调自建 / 本地后端** | 复制`.env.example` → `.env.local`，按需修改 `NEXT_PUBLIC_SERVER_ADDR` / `NEXT_PUBLIC_EMBED_BASE`（`.env.local` 不入库）。Tauri App 使用自建 embed 源时还须同步修改 `src-tauri/tauri.conf.json` 的 CSP `frame-src`，并让门户的 `frame-ancestors` 放行 ideall 后重新打包 |

客户端直连需后端放行 CORS（见 [docs/app.md](docs/app.md)）。

## App（桌面 / 移动）

ideall 同一套代码以 Windows / Linux / macOS / Android / iOS 为目标；当前自动发布覆盖桌面端，Android 只有 CI debug APK，iOS 未入 CI，移动应用商店发布尚未落地。开发、构建、目标矩阵、CI、签名与环境变量口径见 [docs/app.md](docs/app.md)；环境变量示例见 [`.env.example`](.env.example)。

## API 类型同步（codegen）

ideall 调用后端数据服务的类型来自 OpenAPI schema（经 `ServerPort` 契约消费；wonita 的 server 是其一个参考实现），**不手写 DTO**：

```text
openapi/server.json             ← 已提交的契约源
src/lib/api/server.d.ts         ← openapi-typescript 生成物
scripts/sync-server-openapi.mjs ← 维护者刷新契约用（可选）
```

```bash
pnpm gen:api        # openapi/server.json → server.d.ts（离线；普通贡献者只需这一步）
pnpm gen:api:check  # CI：schema 与生成物一致

# 维护者：拿到后端新导出的 openapi.json 后刷新契约源并提交
SERVER_LOCAL=/abs/path/to/openapi.json pnpm sync:api
```

`openapi/server.json` 已随仓库提交，构建/类型生成完全离线；后端契约更新后由维护者 `sync:api` 刷新并提交。

## 文档导航

| 文档                                              | 内容                                                                                |
| ------------------------------------------------- | ----------------------------------------------------------------------------------- |
| [docs/architecture.md](docs/architecture.md)       | 架构权威说明：领域模型、模块与边界、数据流图、技术选型、关键不变量                  |
| [docs/README.md](docs/README.md)                   | 全部文档按现行规范、操作手册、决策记录与历史归档分类                                |
| [docs/app.md](docs/app.md)                         | App（桌面 / 移动）当前开发、构建、CI、发布与签名手册                                |
| [docs/scripts.md](docs/scripts.md)                 | 本地验证、冒烟、API codegen、发布与脚本维护入口                                     |
| [docs/app-history.md](docs/app-history.md)         | App-only / Tauri 化历史路线图                                                       |
| [docs/design/ui-style.md](docs/design/ui-style.md) | UI 视觉规范（现代 · 面板 · 留白）：阴影 / 颜色 / 圆角 / 间距 / 公共组件的统一口径 |
| [docs/development.md](docs/development.md)         | 仓库结构与开发约定（贡献者速查）                                                    |
| [.github/SECURITY.md](.github/SECURITY.md)         | 安全策略与漏洞报告（含跨端同步加密关注点）                                          |

## 参与贡献

欢迎 Issue 与 PR（UI、home 本地能力、插件等）。开发约定见 [docs/development.md](docs/development.md)，架构说明见 [docs/architecture.md](docs/architecture.md)。

## 许可与商标

- 源码：[Apache License 2.0](LICENSE)
- 商标：**Wonita** 名称与官方 logo 不随 [Apache 2.0](LICENSE) 许可转让
