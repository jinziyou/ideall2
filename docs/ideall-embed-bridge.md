# ideall 嵌入桥（Embed Bridge）方案与技术规格

> **ideall 实现权威**：本文对应宿主实现 `src/plugins/embed/*`。工作区根 `docs/ideall-embed-bridge.md` 与 wonita 的 `docs/integrations/ideall-embed-bridge.md` 是跨仓协同副本；改协议时需同步。
> 范围：把 ideall 从"原生实现 info/community"改为"**Web 容器**"——ideall 定义嵌入协议，info/community 只是 wonita 实现的两个嵌入应用。
> 嵌入与传输用 **iframe + postMessage**，能力契约用 **MCP（JSON-RPC 2.0）跑在 postMessage 之上**。
> 决策已锁定：**发布身份/token 由 ideall 持有**（微信式 SSO，token 永不进 iframe）。
> 关联文档：[`docs/app.md`](app.md)（Tauri / 静态导出）、[`docs/architecture.md`](architecture.md)（终端架构）。
> **定位**：轻量级**嵌入式 Web 应用**（webview + 桥，Telegram Mini App / 飞书 H5 一档）——provider 的网页经 iframe 集成并通信，**非**微信式（打包 / 自有 DSL / 双线程深沙箱）。
> 最后核对：2026-07 · 决策人：lyping
>
> **本仓内说明**：本文是嵌入桥的设计规格，已随仓落地（实现见 `src/plugins/embed/*`）。规格成文于 `src/` 终端化重组前，文中个别概念名已演进——宿主本地数据端口 `HubData` → 现 `FilesPort`（`@protocol/files` / `getFilesPort()`）；面向用户的「订阅」措辞现统一为「关注」（端口/工具的英文标识如 `hub.subscriptions` 不变）。下文涉及本仓的路径已对齐当前结构。

---

## 0. TL;DR

- ideall = Web 容器（浏览器能力 + 数据桥），工作区“浏览”分区中的 info/community 是**远程嵌入应用**；wonita 是默认实现、可换。
- **两个面分开**：嵌入/呈现面 = iframe+postMessage；能力/数据面 = MCP。MCP 不渲染页面，只做能力调用。
- **数据按类型分流**（关键）：公共语料**页面直连 wonita**；主权数据、身份与导航**经 iframe 桥的 MCP**；带鉴权的写（发布）经 ideall，用宿主持有的 token。内置 Agent 不向 iframe 暴露，另经进程内 loopback 复用同一能力层。
- **隔膜**（membrane）= 宿主壳。它是唯一留在开源客户端、可审计的部分；私有 E2E 同步身份/密钥永不进契约。
- 本地优先 = **用户数据主权在用户**，不是"全部离线/本地渲染"。info/community 是"链接外部世界"层，无离线要求。

---

## 1. 目标 / 非目标

**目标**
- 用一套 ideall 自有协议承载可嵌入的 web 扩展，info/community 为头两个实例。
- 保住三条主张：后端可换·可自建、用户数据主权、开源可审计——且把"可换"从数据层延伸到 UI 层。
- 让 wonita 能用 web 技术快速迭代 info/community 富 UI，不被客户端发版节奏卡住。

**非目标**
- 不做离线打包/缓存（info/community 非主权数据，不需要）。本规格只覆盖远程嵌入应用。
- 不改 home 中枢（订阅/书签/资源/同步）的本地优先与原生实现——那是主权内核，永远原生。
- 不替代 ServerPort：ServerPort 仍管原生核心与 agent 的取数后端可换。

---

## 2. 架构总览

### 2.1 三层模型

| 层 | 内容 | 形态 | 可换机制 |
| --- | --- | --- | --- |
| 主权内核 | home：订阅/书签/资源/私有同步身份 | 本地 / E2E / 原生 / 开源 | ServerPort（后端可换/自建） |
| 链接层（扩展） | info / community / 未来第三方 | 远程嵌入应用（iframe） | 可换整个嵌入应用 |
| 隔膜（桥） | 宿主壳：握手、起 MCP server、按权限门控 | 留在开源客户端、可审计 | —— |

### 2.2 两个通信面

```
            ┌───────────────────────── ideall (Tauri webview) ─────────────────────────┐
            │  宿主壳 (Next 静态导出, 持 Tauri 能力, withGlobalTauri=false)              │
            │   ├─ 工作区五分区导航 / 标签壳                                              │
            │   └─ EmbedHost: <iframe src="https://www.wonita.link/..."> ───────┐     │
            │         │  mcpPort  ── MCP (JSON-RPC) ──┐                            │     │
            │         │  uiPort   ── UI 事件 (裸 JSON)┘                            │     │
            └─────────┼──────────────────────────────────────────────────────────┼─────┘
                      │ (宿主壳 = MCP server; 隔膜在 handler 内)                    │
        ┌─────────────▼──────────────┐                              ┌─────────────▼─────────────┐
        │ 主权/身份/agent 经 MCP:      │                              │ 被嵌入页 (wonita web)       │
        │  ScopedHost / auth-store    │                              │  MCP client + UI 事件       │
        │  ServerPort.publish(token)  │                              │  公共语料: 直连 ↓           │
        └────────────┬───────────────┘                              └─────────────┬─────────────┘
                     │ 带 token 的写                                                │ 公共读 (读开放)
                     ▼                                                              ▼
              ┌────────────────────────  wonita apiserver (api.wonita.link)  ─────────────────────────┐
              │  /v2/data/** (公共读)   /v2/app/me/publications (JWT)   /v2/app/auth/* (X25519)   │
              └───────────────────────────────────────────────────────────────────────────────────────┘
```

- **能力面（mcpPort）**：请求/响应、要 schema 的调用 —— 身份、发布、本地 hub、agent、导航、（可选）可换后端 data 代理。
- **UI 事件面（uiPort）**：高频 fire-and-forget —— 主题、resize、可见性、返回键、就绪。**不塞进 MCP**，避免把 UI 节奏耦合进 server 生命周期。

---

## 3. 数据拓扑（谁 ↔ 谁）

**判定规则（唯一标准）**：这份数据是否**主权 / 需用户身份 token / 只有宿主才有**？是 → 经 ideall；否（公共、provider 自有、读开放）→ 页面直连后端。

| 数据 | 谁 ↔ 谁 | 通道 | 说明 |
| --- | --- | --- | --- |
| 公共语料：info 文章 / 实体图 / peer 列表 / peer 发布 | **页面 ↔ wonita** | 页面自身 fetch | 非主权、读开放；wonita 的页连 wonita 的语料天然；大列表/分页直连更快，不被 MCP 契约卡形状 |
| 发布 / 删除 / 改 profile（需 JWT） | **页面 ↔ ideall(MCP) → ideall 持 token → wonita** | `community.publish` 等 | token 归宿主，永不进 iframe；仅此类写发生 ideall↔wonita |
| 本地主权数据：关注 / 书签 | **页面 ↔ ideall** | `hub.*` | 只有宿主有（IndexedDB，经收窄的 `ScopedHost.files` 访问） |
| 当前发布身份（CurrentUser） | **页面 ↔ ideall** | `identity.me` | 来自宿主 auth-store |
| agent（摘要/问答） | **不经 iframe 桥开放** | — | 内置 Agent 经进程内 loopback 消费同一 MCP 能力层 |
| 导航 / 外链 / 主题 | **页面 ↔ ideall** | `host.*` / uiPort | 应用外壳能力 |
| 可选：后端可换读（保留设计） | **尚未实现** | `data.*` | wonita 一方页直接取数；当前 `PERMISSIONS` 与工具注册面均没有 `data.*` |

> 可换分两层、两套机制：**核心层可换 = ServerPort**（原生 home/agent 可换后端）；**嵌入应用层可换 = 可换整个嵌入应用**（info 是协议，wonita 是默认实现）。所以"wonita 页直连 wonita 语料"不破可换性——可换单位是嵌入应用，不是语料读路径。

---

## 4. 嵌入与传输

### 4.1 连接时序（两次握手，别混）

```
宿主壳                                              被嵌入页 (wonita web)
  │ 1. 渲染 <iframe src=entry sandbox=...>
  │ 2. ◄── ideall:embed-hello ──────────────────────────────────────────  页面就绪后约每 200ms 重发，直至收到 init
  │ 3. 校验 event.source === iframe.contentWindow 且 event.origin === manifest origin
  │ 4. new MessageChannel() ×2 (mcp, ui)
  │ 5. iframe.contentWindow.postMessage(
  │      {type:"ideall:init", protocol, appId, permissions, theme, backendApiVersion, capabilities},
  │      MANIFEST_ORIGIN, [mcpPort2, uiPort2])  ─────────────────────────►  6. 校验宿主 origin，接收两个 port
  │ 7. McpServer.connect(MessagePortTransport(mcpPort1))                  8. McpClient.connect(MessagePortTransport(mcpPort2))
  │ 9. ◄── initialize ──────────────────────────────────────────────────  client 发起 MCP initialize
  │10. ── result(serverInfo, capabilities) ─────────────────────────────►
  │                                                                      11. notifications/initialized
  │                                                                      12. listTools / callTool / 公共语料直连
```

- **端口移交握手**（被嵌入页 hello 触发）：宿主**只在收到被嵌入页 hello 后**才建桥发 `ideall:init`——不在 iframe `load` 时主动发（`load` 常早于被嵌入页 JS 就绪，提前发 init 会丢包且 `started` 守卫阻止重试）；被嵌入页就绪后每约 200ms 重发 hello 直到收到 init。把两条 `MessageChannel` 的 port2 经 `postMessage` 转移给 iframe；此后只走专用 port，不再用 `window.postMessage` 广播（点对点、更安全）。
- **MCP 握手**（client 发起）：标准 `initialize` 协商，跑在 mcpPort 之上。

### 4.2 MCP-over-postMessage 传输（两侧共用）

MCP TS SDK 的 `Transport` 可插拔，包一个 `MessagePort` 即可：

```ts
// ideall 侧实现：src/plugins/embed/transport.ts（被嵌入页用 @ideall/embed-sdk 同款实现，见 §12）
import type { Transport } from "@modelcontextprotocol/sdk/shared/transport.js"
import type { JSONRPCMessage } from "@modelcontextprotocol/sdk/types.js"

export class MessagePortTransport implements Transport {
  onmessage?: (m: JSONRPCMessage) => void
  onclose?: () => void
  onerror?: (e: Error) => void
  constructor(private port: MessagePort) {}
  async start() {
    this.port.onmessage = (e) => this.onmessage?.(e.data as JSONRPCMessage) // 一条消息 = 一个 JSON-RPC 对象 (结构化克隆)
    this.port.onmessageerror = () => this.onerror?.(new Error("messageerror"))
    this.port.start()
  }
  async send(m: JSONRPCMessage) { this.port.postMessage(m) }
  async close() { this.port.close(); this.onclose?.() }
}
```

**Tauri 配套**（`src-tauri/tauri.conf.json`，CSP 已收紧到嵌入源；下段为示意，**以 `tauri.conf.json` 现值为准**——2026-06 加固后已补 `script-src` / `font-src` / `connect-src 'self' tauri:` / `worker-src`，见 [SECURITY.md](../.github/SECURITY.md)）：

```jsonc
"app": { "security": {
  "csp": "default-src 'self' tauri:; script-src 'self' tauri: 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src * data: blob:; font-src 'self' data:; connect-src 'self' tauri:; frame-src 'self' https://www.wonita.link https://wonita.link; worker-src 'self' blob:"
}}
```

- `withGlobalTauri: false` 已设 → iframe 跨域子框架**拿不到** Tauri API（隔膜成立，无需额外封堵）。
- 宿主壳 fetch 绕 CORS 已放行（capabilities `http:default` allow `https://*`/`http://*`），供 `community.publish` 等带 token 的写经 `@tauri-apps/plugin-http` 走。
- iframe 加 `sandbox="allow-scripts allow-forms allow-popups allow-same-origin"`（按需收紧；`allow-same-origin` 仅在 wonita 页需自身存储时给）。

### 4.3 宿主源（origin）

| 场景 | HOST_ORIGIN |
| --- | --- |
| 桌面 macOS/Linux 生产 | `tauri://localhost` |
| Windows / Android 生产 | `https://tauri.localhost` |
| 开发（`pnpm dev` / `app:dev`） | `http://localhost:5020` |

被嵌入页须按平台校验来源白名单（见 §9.2）。

---

## 5. MCP 能力契约

宿主壳 = 一个 MCP server（**每个 iframe 一个独立实例**，只注册该嵌入应用有效授权范围内的工具 → `tools/list` 天然只暴露可用项）。下面 backed-by 列出对应的 ideall 现有端口/方法。类型见 `src/protocol/{server-port,files,subscription}.ts`；handler 通过按权限收窄的 `ScopedHost` 访问这些端口。

### 5.1 Resources（只读、可枚举的宿主自有数据）

| URI | 返回 | 权限 | backed-by |
| --- | --- | --- | --- |
| `identity://me` | `CurrentUser \| null` | `identity:read` | `auth-store.getSession().user` |
| `hub://subscriptions` | `Subscription[]` | `hub.subscriptions:read` | `ScopedHost.files.listSubscriptions()` |
| `hub://bookmarks` | `Bookmark[]` | `hub.bookmarks:read` | `ScopedHost.files.listBookmarks()` |
| `fs://nodes` | 脱敏后的 `Node[]` | `fs:read` | `ScopedHost.files.listStripped()` |

### 5.2 Tools

**identity / 发布（token 由宿主持有，永不返回给页面）**

| tool | 入参 | 出参 | 权限 | backed-by |
| --- | --- | --- | --- | --- |
| `identity.me` | — | `CurrentUser \| null` | `identity:read` | `ScopedHost.getSession()` |
| `community.publish` | `{title:string, url?:string, body?:string}` | `Publication` | `identity.publish` | `ServerPort.publish(hostToken, draft)` |
| `community.deletePublication` | `{id:number}` | `{ok:true}` | `identity.publish` | `ServerPort.deletePublication(hostToken, id)` |
| `me.updateProfile` | `{name?:string, avatar?:string}` | `{ok:true}` | `identity.publish` | ✅ 已由 `ServerPort.updateProfile(token, patch)` 提供（封装 `PUT /v2/app/me/profile`，204 无响应体） |

> `community.publish` 入参 schema 复用 `PublishDraft`（`server-port.ts`，已有）。`community.publish` / `deletePublication` 对应的 `ServerPort.publish/deletePublication` 已存在；`me.updateProfile` 对应的 `ServerPort.updateProfile` 亦已补齐。页面**不传 token**；宿主从 `auth-store` 取。未登录时宿主返回 `-32002 not-authenticated`，页面据此提示"在 ideall 中登录"。

**hub / 本地主权数据**

| tool | 入参 | 出参 | 权限 | backed-by |
| --- | --- | --- | --- | --- |
| `hub.listSubscriptions` | — | `Subscription[]` | `hub.subscriptions:read` | `listSubscriptions()` |
| `hub.addSubscription` | `NewSubscription` | `Subscription` | `hub.subscriptions:write` | `addSubscription(input)` |
| `hub.removeSubscription` | `{type:SubscriptionType, key:string}` | `{ok:true}` | `hub.subscriptions:write` | `removeSubscription(type,key)` |
| `hub.isSubscribed` | `{type, key}` | `boolean` | `hub.subscriptions:read` | `isSubscribed(type,key)` |
| `hub.addBookmark` | `CaptureBookmarkInput` | `Bookmark` | `hub.bookmarks:write` | bookmarks FileSystem `capture.bookmark` |

> 用户在 info/community 里“保存到我的”时，宿主只接受有界标题、HTTP(S) URL、摘要和 favicon，再经 `UiActions` 转入 bookmarks FileSystem。新对象自动带收件箱标签；canonical URL 的检查与创建位于同一 IndexedDB 事务，重复调用返回已有 `Bookmark`。主权动作落在主权层，不留扩展侧。

**宿主导航（当前能力）**

| tool | 入参 | 出参 | 权限 | 说明 |
| --- | --- | --- | --- | --- |
| `host.navigate` | `{route:string}` | `{ok:true}` | `host.nav` | 打开 ideall 内部路由（白名单前缀） |
| `host.openExternal` | `{url:string}` | `{ok:true}` | `host.external` | 经宿主 UI action 打开“浏览器”模块；URL 协议校验 |
| `host.toast` | `{message:string, kind?:"info"\|"error"}` | `{ok:true}` | （随容器默认） | 用 ideall toast |

**data / 后端可换读（历史保留设计，未实现）**

| tool | 入参 | 出参 | 权限 | backed-by |
| --- | --- | --- | --- | --- |
| `data.queryInfo` | `InfoQuery` | `Info[]` | `data.info:read` | `ServerPort.queryInfo` |
| `data.getEntityDetail` | `{label, name}` | `EntityDetail \| null` | `data.info:read` | `ServerPort.getEntityDetail` |
| `data.listPeers` | — | `PeerPublisher[]` | `data.peers:read` | `ServerPort.listPeers` |
| `data.getPeerPublications` | `{id:string}` | `Publication[]` | `data.peers:read` | `ServerPort.getPeerPublications` |

### 5.3 Agent / Prompts（历史保留设计，未实现）

预置 agent 工作流，供嵌入应用一键调用：`summarize-selection`、`compare-entities`、`digest-peer`。映射到 `agent.run` 的固定模板。

> **实现现状（设计已 pivot）**：`agent.run`（§5.2）与本节 Prompts **均未**作为嵌入工具落地——ideall 的 agent 改为经**宿主侧回环**直接消费同一 MCP server（见 `src/plugins/agent/`），而非 iframe 调 `agent.run`；故被嵌入页拿不到、也不需要 `agent.*`。同理 `data.*`（§5.2）**按设计留空未实现**——被嵌入页对公共语料一律**页面直连**取数，不经宿主 data 代理。上表 `agent.*` / `data.*` 行属保留的协议契约面，当前无对应注册工具（实际注册项见 `src/plugins/embed/tools.ts`）。

---

## 6. 权限模型与 manifest

### 6.1 manifest（见 `src/plugins/embed/manifest.ts`）

```jsonc
{
  "id": "community",
  "name": "Wonita 社区",
  "version": "1.0.0",
  "entry": "https://www.wonita.link/community",   // 嵌入入口 URL
  "origins": ["https://www.wonita.link"],          // 允许的 iframe 源 (校验 + CSP frame-src)
  "minHostProtocol": "1.0",                        // 要求的最低宿主协议
  "permissions": [
    "identity:read", "identity.publish",
    "hub.subscriptions:read", "hub.subscriptions:write",
    "hub.bookmarks:write",
    "host.external", "host.nav"
  ]
}
```

info 与 community 的现行 manifest 都授予 `hub.subscriptions:read|write`、`hub.bookmarks:write`、`host.external` 与 `host.nav`，因此资讯文章和社区发布使用同一捕获能力；公共语料由页面直连，不存在 `data.info:read`。community 另有 `identity:read` / `identity.publish`。完整清单以 `src/plugins/embed/manifest.ts` 为准。

### 6.2 权限位清单

现行 `PERMISSIONS` 包含 identity、hub、host、`fs.*`、`agent.config:read`、`ui.tabs`、`web.*` 与 `browser.*` 分组。`agent.invoke` 和 `data.*:read` 不在联合中，只是 §5.2–5.3 的历史保留设计；实际生效清单以 `src/plugins/embed/protocol.ts` 为准。

### 6.3 授权流程

- 一方（first-party，如 wonita）嵌入应用：随包预置 manifest，当前自动建立 Grant，不弹连接前 consent。
- 已建立连接会登记到设置页“已连接的应用”，展示 origin 与权限；用户可立即断开对应 MCP server。该撤销只作用于当前运行期连接，刷新后仍会按一方 manifest 重连。
- 三方 manifest、持久 Grant、连接前 consent 与逐次敏感操作确认尚未实现；未进入 manifest/CSP 白名单的任意来源不能建立桥。
- 宿主只注册 `effectivePermissions(grant)` 允许的 tool/resource；未授权能力不会出现在 `tools/list`，直接调用未注册工具由 MCP 层返回未知工具错误。

---

## 7. 身份与 token（ideall 持有）

- **现状即如此**：发布身份的 token 存于 secure-store key `ideall:auth:token`（Web 降级为 `ideall:secure-fallback:ideall:auth:token`），用户资料存于 `localStorage` 的 `ideall:auth:user`；历史 `wonita:auth:*` 键仅作一次性迁移来源。会话经 `auth-store`（`getSession/setSession/clearSession`）管理。这是**公开发布身份**，与无账号 E2E 同步身份是两套（见 `auth-store.ts` 注释）。
- **登录只在 ideall 做**：X25519 方案（`getServerPublicKey → encryptPassword → login/register → setSession`）。被嵌入页**不做登录**、不存 token。
- **嵌入应用如何"以发布身份行事"**：调 `community.publish` / `me.updateProfile`，宿主在 handler 内取 `getSession().token` 调 `ServerPort.publish(token, draft)`。**token 不出现在任何返回值里**。
- **未登录**：`identity.me` 返回 `null`、写类 tool 返回 `-32002`；页面提示用户在 ideall 登录（可附 `host.navigate({route:"/auth"})`）。

---

## 8. UI 事件面（uiPort，非 MCP）

裸 JSON 通知，`{type, payload}`，单向、无需 id 关联。

**宿主 → 页面**

| type | payload | 用途 |
| --- | --- | --- |
| `theme` | `{mode:"dark"\|"light", tokens:{...}}` | 初始 + 变更，套 ideall 色板 |
| `visibility` | `{visible:boolean}` | 标签切换/挂起 |
| `navigated` | `{route}` | 宿主路由变化通知 |

`resize` 与移动端 `back` 仍是保留设计，当前宿主未发送。

**页面 → 宿主**

| type | payload | 用途 |
| --- | --- | --- |
| `ready` | `{}` | 内容挂载完成（宿主可撤 loading） |

`set-title`、`request-resize` 与 `loading` 已保留消息名，但当前宿主忽略这些事件。

---

## 9. 隔膜与安全

### 9.1 不变量

- **token / 私有身份永不进 iframe**：所有带鉴权写经宿主 handler 持 token 完成。
- **私有 E2E 同步身份/密钥/同步块不进契约**：没有对应 tool/resource。
- **每个 iframe 独立 server + 独立授权集**：多嵌入应用互不可见彼此能力。

### 9.2 校验

- `ideall:init` 必须校验 `event.origin === HOST_ORIGIN`（按平台，§4.3）。
- 宿主侧校验 iframe `entry` 源在 manifest `origins` 白名单内；CSP `frame-src` 同步收紧到该源。
- 握手后只用移交的 `MessagePort`，不监听/不广播 `window.postMessage`。
- `host.openExternal` / 页面内外链：宿主只接受 `http:` / `https:`，再经 UI action 打开隔离的“浏览器”表面；页面不能借此提交其它协议。

### 9.3 被嵌入页的 header（wonita 侧）

```
Content-Security-Policy: frame-ancestors tauri://localhost https://tauri.localhost http://localhost:5020;
# 去掉 X-Frame-Options: DENY/SAMEORIGIN（否则浏览器直接拒绝被 iframe）
```

---

## 10. 宿主侧实现（ideall）

### 10.1 新增 / 改动

- `src/plugins/embed/host.tsx`：`EmbedHost`——建 `MessageChannel`、握手、按 manifest 起 `McpServer`、注册授权工具、生命周期清理。
- `src/plugins/embed/transport.ts`：`MessagePortTransport`（§4.2）。
- `src/plugins/embed/tools.ts` / `grant.ts` / `scoped-host.ts` / `local-mcp-server.ts`：先按 Grant 得出有效权限并构造收窄的宿主句柄，再注册 tool/resource、启动本地 MCP server。
- info / community 的现行入口通过 `ideall.connected` File Engine 渲染 `EmbedHost`；`src/workspace/registry.tsx` 中同名静态标签项仅保留旧快照兼容。
- `src/plugins/embed/connections.ts` / `connected-apps-view.tsx` 与设置页：登记活动连接，并提供运行期查看与一键断开。
- `src-tauri/tauri.conf.json`：设 CSP（§4.2）。capabilities 已够用（`http:*`、`opener`）。

### 10.2 server 接线（要点）

```ts
// 仅在来源与 iframe 窗口均校验通过的 ideall:embed-hello 后调用。
function startBridge(manifest: Manifest, iframe: HTMLIFrameElement) {
  const mcp = new MessageChannel()
  const ui = new MessageChannel()
  iframe.contentWindow!.postMessage(
    { type: "ideall:init", protocol: "1.1", appId: manifest.id,
      permissions: manifest.permissions, theme: currentTheme(), backendApiVersion: "v2",
      capabilities: ["community:string-ids", "profile:display-name", "sync:partitioned-v2"] },
    new URL(manifest.entry).origin, [mcp.port2, ui.port2])

  const grant = firstPartyGrant(manifest, Date.now())
  const server = createLocalMcpServer(grant, { navigate: (route) => router.push(route) })
  void server.connect(new MessagePortTransport(mcp.port1))
  wireUiEvents(ui.port1)
  return () => {
    void server.close()
    mcp.port1.close()
    ui.port1.close()
  }
}
```

具体 handler 不直接取模块单例，而是使用 `makeScopedHost(effectivePermissions(grant))` 生成的收窄句柄；现行接线见 `local-mcp-server.ts`、`scoped-host.ts` 与 `tools.ts`。

---

## 11. 被集成页改动（wonita web）

所有改动都在边缘，核心组件不动，且须**双模式**（嵌入态 vs 独立站点态）。

**必须（否则跑不起来）**
1. **放行被嵌入**：服务端去 `X-Frame-Options`、设 `frame-ancestors`（§9.3）。
2. **接握手 + transport**：页面就绪后循环发送 `ideall:embed-hello`，收到 `ideall:init` 时校验 origin、取 ports、启动 MCP client 并执行 `initialize`。封进 `@ideall/embed-sdk` 后由 `IdeallEmbed.connect()` 处理。
3. **取数分流**：公共语料**仍页面直连** wonita；发布/删除/profile、订阅/收藏、当前身份**改走 MCP**；页面不再自存 token、嵌入态不做登录。
4. **双模式检测（两个嵌入态信号）**：① **主判据 = 同步 URL 标记**——ideall 给 iframe `src` 注入 `?embed=ideall&embedApp=<id>`（宿主 `src/plugins/embed/host.tsx`，query 不改 origin），被嵌入页**首帧**即可据此判嵌入态、立刻去 chrome，**无闪烁**；② **`ideall:init` 握手**——收到即确认嵌入态并接 transport（§4）。两信号皆缺、超时无 init=独立站点态；此超时非 ~800ms 而是 **8s 水合容差下限**（`Math.max(timeoutMs, 8000)`，见 `wonita/portal/src/embed/client.ts`），吸收宿主慢水合 / dev 冷编译。

```ts
const ideall = await IdeallEmbed.tryConnect({ timeoutMs: 800 })  // 嵌入则连上, 否则 null

// 公共语料: 两态都直连 wonita apiserver v2
export const queryInfo = (p) =>
  fetch(`${API}/v2/data/corpus/articles/query`, { method: "POST", body: JSON.stringify(p) }).then((r) => r.json())

// 发布: 嵌入态经 host(无 token), 独立态用自己的登录
export const publish = (d) =>
  ideall ? ideall.call("community.publish", d)
         : fetch(`${API}/v2/app/me/publications`, { headers: authHeader(), method: "POST", body: JSON.stringify(d) })

export const me = () => ideall ? ideall.call("identity.me") : fetchMeStandalone()
export const canPublish = ideall ? ideall.permissions.includes("identity.publish") : isLoggedIn()
```

**强烈建议（像原生 + 安全）**
5. **去掉自己的导航/头尾**，只渲染内容区（外壳由 ideall 工作区提供）。**首帧**就读 §11.4 的 `?embed=ideall` URL 标记来隐藏 chrome——别等异步 `ideall:init`，否则导航会闪一下。注意：ideall 作宿主**无法**跨域删你的导航（同源策略），故此条必须被嵌入页自己做。
6. 主题：吃 init 的 `theme` + 听 uiPort `theme`，套 ideall token。
7. 导航：外链/跳 ideall 路由改调 `host.openExternal`/`host.navigate`，不用 `window.open`/`target=_blank`。
8. 能力门控：按 init 的 `permissions` 显隐 UI（没授 publish 就别显示发布按钮）。
9. UI 事件：挂载后发送当前已处理的 `ready`；`request-resize`、移动端 `back` 与 safe-area 仍需宿主实现后再接入。
10. 安全：每条入站消息校验 origin；只用移交 port；收紧页面自身 CSP。
11. 版本：`initialize` 协商 `protocolVersion`，过旧提示升级。

> 实操：wonita portal 已是相 1 的 web 入口；嵌入版与独立站点共享组件，只在“外壳 / 取数 / 鉴权 / 门控”分叉。wonita web 侧已有 info/analysis/entity/publishers 组件可复用作内容区。

---

## 12. `@ideall/embed-sdk`（被嵌入页用）

封装握手 + transport + MCP client，作者无需自己实现：

```ts
class IdeallEmbed {
  static tryConnect(opts?: {timeoutMs?: number}): Promise<IdeallEmbed | null>  // 超时无 init 返回 null
  static connect(): Promise<IdeallEmbed>                                       // 嵌入态必连
  readonly permissions: string[]
  readonly theme: ThemeTokens
  call<T>(tool: string, args?: unknown): Promise<T>          // tools/call (越权/错误 → 抛带 code 的错误)
  read<T>(uri: string): Promise<T>                            // resources/read
  on(type: "theme"|"resize"|"visibility"|"back"|"navigated", cb): () => void  // uiPort 事件
  emit(type: "ready"|"set-title"|"request-resize"|"loading", payload?): void   // → 宿主
}
```

> 注：`tryConnect` 的实际超时有 **8s 下限**（`Math.max(timeoutMs ?? 800, 8000)`）以吸收宿主慢水合 / dev 冷编译；顶层窗口（非 iframe）立即返回 `null`（独立态），仅 iframe 内才等握手。双模式主判据是同步 `?embed=ideall` URL 标记（§11.4），此超时仅兜底。

宿主侧也复用本包的 `MessagePortTransport`。

---

## 13. 版本化与协商

- 传输/握手版本：`ideall:init.protocol`（当前 `"1.1"`）；页面 `minHostProtocol` 不满足则降级（独立态/提示升级）。
- 数据面协商：`backendApiVersion` 声明宿主实际连接的 wonita API，`capabilities` 逐项声明 string ID、展示名与分区同步能力；1.0 宿主缺失时客户端禁止 V2 写入。
- MCP 层：`initialize` 协商 `protocolVersion` 与 server `capabilities`。
- 能力契约演进：新增 tool/permission 向后兼容；破坏性变更走大版本，宿主可并行支持多版本契约。

---

## 14. 错误与降级

| 情况 | 行为 |
| --- | --- |
| 无 `ideall:init`（超时，实际 **8s 水合容差下限**） | 页面进独立站点态；主判据为同步 `?embed=ideall` URL 标记（§11.4），超时仅兜底 |
| 工具未出现在 `tools/list` / 调用未知工具 | 页面隐藏对应入口；授权位或信任档不允许时宿主不会注册该工具 |
| `-32002 not-authenticated` | 提示在 ideall 登录，可附 `host.navigate("/auth")` |
| 宿主协议过旧 | 提示升级 ideall；非关键功能降级 |
| iframe 加载失败 / 离线 | 宿主显示重试；公共语料离线即不可用（设计如此，非主权数据） |

---

## 15. 分期路线图

- **A 期（已落地）**：协议最小集（hello/init 握手 + `MessagePortTransport` + `identity.*` + `community.publish` + `host.*`）+ `EmbedHost`，community 验证 token 不出 iframe 的链路。
- **B 期（主链路已落地，方案有调整）**：info 作为嵌入应用，公共语料直连，关注/书签写回宿主；`agent.run` 未向 iframe 开放，内置 Agent 改走宿主侧 loopback。
- **C 期（未落地）**：任意第三方嵌入应用、持久 Grant、连接前 consent 与公开 SDK/发现流程。

---

## 16. 验收 / 测试

- 握手：iframe 收到 init、origin 校验通过、双 port 建立、MCP `initialize` 成功。
- 隔膜：iframe 内 `window.__TAURI__` 为 undefined；任何路径拿不到 `ideall:auth:token` 或 secure-store fallback；未授权 tool 不出现在 `tools/list`，直接调用按未知工具处理。
- 发布闭环：community 嵌入应用经 `community.publish` 成功发布，网络层确认 token 由宿主注入、未经 iframe。
- 写回主权：在嵌入应用内"订阅发布者"→ `hub://subscriptions` 出现该项 → home 中枢可见。
- 双模式：同一 wonita 页独立打开（直连+自登录）与嵌入打开（分流+SSO）均工作。
- 跨平台：桌面 + Android（iOS 待证书）下 iframe + postMessage + 主题/safe-area 正常。
- 旧兼容层回归：`pnpm legacy:web:export` / lint / typecheck / test 绿；生产 GPUI
  另运行 `pnpm native:check` / `native:test` / `native:lint`。

---

## 17. 附录：消息样例

```jsonc
// (A) 宿主 → iframe：init（随附两个 MessagePort）
{ "type":"ideall:init", "protocol":"1.1", "appId":"community", "backendApiVersion":"v2",
  "capabilities":["community:string-ids","profile:display-name","sync:partitioned-v2"],
  "permissions":["identity:read","identity.publish","hub.subscriptions:write","host.external"],
  "theme":{"mode":"dark","tokens":{"--bg":"#0b0b0c","--fg":"#e8e8ea"}} }

// (B) mcpPort：MCP 握手
{ "jsonrpc":"2.0", "id":1, "method":"initialize",
  "params":{ "protocolVersion":"2025-06-18", "clientInfo":{"name":"community","version":"1.0.0"}, "capabilities":{} } }
{ "jsonrpc":"2.0", "id":1, "result":{ "protocolVersion":"2025-06-18",
  "serverInfo":{"name":"ideall-host","version":"1.0.0"}, "capabilities":{"tools":{},"resources":{}} } }

// (C) 发布（token 不出现；宿主注入）
{ "jsonrpc":"2.0", "id":7, "method":"tools/call",
  "params":{ "name":"community.publish", "arguments":{ "title":"一篇笔记", "url":"https://x", "body":"..." } } }
{ "jsonrpc":"2.0", "id":7, "result":{ "content":[{"type":"text","text":"{\"id\":42,\"title\":\"一篇笔记\",\"created_at\":1718900000000}"}] } }

// (D) 未登录
{ "jsonrpc":"2.0", "id":8, "error":{ "code":-32002, "message":"not-authenticated" } }

// (E) uiPort：主题变更（宿主 → 页面，非 MCP）
{ "type":"theme", "payload":{ "mode":"light", "tokens":{ "--bg":"#ffffff" } } }
```
