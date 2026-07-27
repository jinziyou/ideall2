# ideall 开发约定

> **原生切换说明（2026-07-27）**：生产开发入口已经迁到 `native/` Rust + GPUI
> workspace，根命令见 [README](../README.md) 与
> [`native/README.md`](../native/README.md)。本文的 `src/`、Next.js 与 Tauri 章节只约束
> V2 归档导出兼容层，不再描述生产 App。

## Repository

ideall 是**独立项目 / 独立仓库**（`git@github.com:jinziyou/ideall.git`）的 **Next.js 客户端应用**，本仓库为源码权威仓库。
整体定位见 [README.md](../README.md)；架构权威说明见 [architecture.md](architecture.md)；API 契约同步见下方“API codegen”。
ideall 是后端数据服务的**外部消费方 / 客户端**：经 `ServerPort` 契约消费 wonita 服务的数据服务 API（wonita 服务是 `ServerPort` 的参考实现；第三方 / 嵌入式 / 局域网节点亦可实现 `ServerPort`）。信息采集 / NLP / 知识图谱 / 鉴权由该后端数据服务提供，ideall 经 `NEXT_PUBLIC_SERVER_ADDR` 连接，不在本仓库范围内；ideall 不被单一后端绑死。

ideall **仅以 App 形态分发**（Tauri 跨平台静态导出，无 Node 运行时 / 无 SSR 生产部署）。Windows / Linux / macOS 桌面包由 CI 自动构建并进入 GitHub Release；Android 目前只有 tag / 手动任务的 debug APK 工件，iOS 未纳入 CI。

**分支模型（dev / main）**：`main` 稳定/发布（App 版本 tag 基于它），`dev` 集成/日常开发；改动先进 `dev`，CI 在 `main` / `dev` / PR 均运行，稳定后合并 `main` 发布。

## Positioning

ideall 是**开源、本地优先的个人信息终端**（独立项目）：从个人视角出发，把分散的他人、信息、资源和工具聚合到一处。
**设计思想**：一切皆文件、一切皆标签页——各类 Storage 经 FileSystem 挂载到统一命名空间，以 `FileRef` 寻址；打开任意文件即由 Engine 渲染为工作区标签页。`ideall.core` 的笔记、书签、资源、关注和对话收敛为 Node 库；音频、数据库、Agent 配置和第三方 App 等来源保留各自存储语义。
**设计风格**：现代 · 面板 · 留白的标签工作区。

**导航信息架构**：活动栏的一级分区固定为“我的 / 活动 / 浏览 / 应用 / 设置”，二级侧栏依次为“关注 / 书签 / 资源 / 文件”、“空间 / 任务 / 删除”、“新闻 / 社区 / 浏览器”、“搜索 / 本地应用”、“基本 / AI”。五个一级入口始终可见，本机数据、联网资源与 App 挂载继续经同一合成文件系统进入 Display。有目录位置的运行态入口（活动栏、侧栏、移动底栏、命令面板、Home 卡片/动态与保存后的回流）统一发出 `OpenTarget { type: "path" }`，路径从 `ideall.navigation` 派生；尚无目录位置的能力直接发出 file/command target。Next `href` 只留给认证、错误页和外部深链等浏览器边界。

**活动与设置也是文件**：`/activity/audit` 链接到 `app.agent-write-audit`，`/activity/spaces`、`/activity/tasks` 链接到 `app.agent-config` 的 workspace/task 文件，`/settings/ai` 链接到 agent settings，`/settings/basic` 链接到 `app.settings:root`。基本设置根包含 appearance/device/data/connections/runtime-extensions 五个 section；data section 提供 secure-store、IndexedDB v20、持久化、全文/语义索引和 workspace archive 的健康/操作面。上述 provider 与 Engine/Display 均由随包 builtin runtime extension 原子安装，旧 panel、static tab 与旧 URL 只在打开和水合边界兼容。Display 读写、订阅、连接/扩展管理、归档和凭据操作必须经 FileSystem registry 与 specialized action，不能直连 backing store。任务与 Node 目录都使用版本 cursor 有界分页；v17 的 `[kind,sortKey,title,id]` 覆盖索引用于摘要页，v18 的 `local_search_index` 保存可重建的全文投影，v19 的 `agent_write_audit` 保存脱敏 Agent 写入 outbox，v20 的 `local_semantic_index` 保存可选的可重建 embedding，v16 引入的 `[kind,sortKey]` 索引继续承担事务内尾键追加。文件 Blob、Task 关系以及关注/笔记/书签同步批次分别保持跨 store 原子性。API key 等凭据只进入安全存储；Tauri secure-store 写入失败必须 fail closed，公开 FileSystem 正文、导出和 action 结果始终不回显 secret。

**系统投递**：安装后的桌面 App 注册 `ideall://capture?url=<encoded-http-url>&title=<optional>`，并为 HTML、PDF 和常见图片提供“打开方式”；运行中第二实例和冷启动参数都会转交主窗口。窗口拖入文件走同一原生队列。原生层只接受明确格式，单文件上限 32 MiB、待处理编码载荷总量上限 64 MiB；前端排空后仍经收件箱 FileSystem 导入和 URL 去重，不能把原生事件当作已提交回执。

**快速捕获纪律**：新闻、社区、关注流、普通外链和浏览器不得各自调用 bookmark store 或普通 `create` 复制去重逻辑。统一调用 bookmarks 根的 `capture.bookmark` specialized action；嵌入页只能经持有 `hub.bookmarks:write` 的 `hub.addBookmark` 工具转入该 action。provider 负责输入上限、HTTP(S) 协议和收件箱标签，Storage 必须在同一 IndexedDB readwrite 事务内完成 canonical URL 查询与创建。重复捕获返回 existing，不能重新添加收件箱标签或覆盖用户已编辑的书签。首次新建书签、网页快照、摘录、导入文件或系统分享只通过 `capture-onboarding` 公开配置状态触发一次非模态引导；toast 只负责回流，收件箱内联说明负责解释整理，归档或主动关闭后永久完成。existing 不能重置状态；配置损坏或公开存储不可用也不能阻断捕获。

**树写入纪律**：笔记/书签移动、笔记子树删除、书签/收藏夹删除，以及同时修改父节点与字段的笔记/书签 `fs.write`，必须把活跃源与目标校验、循环检查、同级排序快照、回收站快照和实际写入放进相应的单一 readwrite 事务。不得先读后跨事务写，也不得通过普通字段 patch 直接覆盖 `parentId/sortKey`；通知只能在事务完成后发布。提交点失败测试应断言 nodes 与 trash snapshot 同时回滚。

**回收站写入纪律**：单项恢复、永久删除与整站清空都必须在覆盖 `nodes` / `blobs` / `trash_snapshots` 的一个事务内 fresh-read 并完成全部写入；文件删除也必须原子生成快照、写 tombstone 并移除 Blob。FileSystem 在列表后执行单项动作时必须传入 `{ kind, updatedAt, deletedAt }` revision；清空确认还必须同步冻结整个 `{ id, kind, updatedAt, deletedAt }[]`，用确定性序列化的 `trash-v2` SHA-256 作为集合版本，并把同一快照下沉到事务比较。`empty` 必须声明为 specialized action，由回收站 Display 在异步摘要前冻结点击时 items 并确认；通用动作菜单不能自行调用。计算期间的刷新不能混用数量与 token。Display 的异步读取必须同时绑定 root target generation 与递增 request generation；只有当前 target 的最后请求可更新 items/loading 或 toast，切换 target 立即隐藏旧列表，卸载使全部 lease 失效。同 root 读取失败保留 last-good，mutation 无论成功、冲突或其它失败都要触发一次重新读取。任一 CAS 不匹配都应返回 conflict，不得让旧界面的动作命中同 id 的新一代墓碑，也不得永久删除确认后才进入回收站的新项。笔记恢复以当前删除子树为单一事务单元，提交后发布一次 kind 级失效通知。

**Node mutation CAS**：`edit` / `move` / `delete` / `restore` / `write-blob` 的预读基线必须以 `{ kind, updatedAt, deletedAt }` 下沉到各 kind Storage 的同一 write transaction 内比较，不能只在 FileSystem 外层检查 `expectedVersion`。`FileSystem.invoke` 的调用方版本通过独立 options 传递，不得混入 provider 自有 action input；确认界面必须传最初展示的版本，不能用执行前重读值替换。预读前已经不存在是 `not-found`；预读后被其它窗口修改、删除或替换 kind 是 `conflict`；空编辑返回当前 live Node 且不提升版本；delete 只有在 Storage 实际写入 tombstone 时才能报告成功。mutation 成功响应必须直接使用事务返回的 committed Node/ResourceRecord 或明确 receipt，不能提交后再 `stat/get` 并把另一笔写入误认为本次结果。

**Agent 工具写入纪律**：本地 `fs.write` / `fs.move` / `fs.delete` 在审批前必须以 Agent 的实际 grant 通过 FileSystem registry 读取目标 metadata；目标不存在、不可读或没有 provider version 时 fail closed。可信 runtime 丢弃模型提供的 `expectedVersion`，只把刚读取的真实 version 写入审批预览和执行参数，再经 MCP → ScopedFiles → FilesPort → Resource FileSystem 下沉到上述 Storage CAS。获批的 mutating tool 在副作用前必须通过 `app.agent-write-audit` 写入 `pending` 脱敏意图；意图落盘失败就阻止执行。工具取得明确回执后，使用同一 IndexedDB 事务把该记录一次性结算为 `committed` 或 `failed`；transport 异常或进程中断无法证明结果时保留 `pending`，由“活动 / 审计”提示人工核对，不得自动重放。容量裁剪不得删除 pending；若 1,000 条全为未结算意图，应拒绝新工具。

**外部 MCP 诊断纪律**：stdio、SSE、Streamable HTTP 必须经 `agent-mcp-diagnostics.ts` 记录同形状态，不能把 SDK/Rust 原始异常直接写进设置、对话 tool event 或持久日志。只允许记录 server id、transport、公开配置 revision、状态、时间/耗时、工具数量、活动会话数和经控制字符清理的工具名；禁止 URL、command/args、env/header、凭据、工具参数及返回正文。新增 transport 或调用入口时须覆盖连接成功/失败、超时、配置变更迟到结果、最近调用和恢复测试。

**签名 connector 映射纪律**：只有经 Catalog verify + consent 且处于 active 的签名 package 才能挂载 connector FileSystem。`resources:read` 与 `tools:invoke` 必须分别控制目录是否存在；FileRef 只能使用 URI/工具名的 SHA-256 不透明身份，目录项、搜索文档和错误不得出现 URI、cursor 或远端原始错误。统一搜索只读授权 metadata，不得为命中搜索调用 `resources/read`。工具 action 只允许可信 UI，必须绑定用户看到的 tool version、限制 JSON 参数大小，并在调用前通过审计 FileSystem 写 pending；审计失败阻止调用，返回后结算失败则保留 pending 且提示不得自动重试。

**FileAction 版本纪律**：生成式 action 菜单、Node 文件专用工具栏与 `FileDocumentClient` specialized action 都必须固定用户看到的 `IdeallFile.version`，并经 `FileActionInvokeOptions` 第五参数传入 provider。provider 对 versioned 目标必须在同一 mutation 锁域内 fresh-read、校验后再触发副作用；插件 data port、旧兼容 UI 等旁路写入口也必须经 adapter 加入相同锁域。Database 的所有 mutation 与整库导入共享 `DATABASE_ROOT_REF`；Audio 的播放状态、曲目写入和整库导入共享 `AUDIO_LIBRARY_ROOT_REF`；Agent 整包导入按稳定全序获取六个 section FileRef；Git 挂载列表导入先固定根和当前 repo refs，repo action 与子文件写共享 repo ref。数据库表版本必须以确定性顺序覆盖完整 table 结构及全部 row 身份、归属、时间和字段值，不能只取最大时间或只信任导入时间戳；Git 根版本必须覆盖 Display 实际读取的 branch/upstream/status/log/remotes/diff，以及 heads、remote-tracking refs 与 tags，不能使用只反映目录 inode 的 guarded version。`undefined` 表示调用方没有提供前置条件，`null` 表示只接受无版本目标；根级创建、导入、集合追加与只读探测没有稳定单目标版本时不应伪造 CAS。冲突不得清草稿、关标签页、发成功/撤销提示或调用后端 mutation。

Database table/rows 的聚合 token 固定使用 `database-v3:<sha256>`；确定性序列化必须覆盖完整 table 与全部 row，摘要统一调用 `src/lib/semantic-version.ts`，禁止重新引入 FNV32 等短哈希。根目录必须先按扁平 table/rows entry offset 应用 cursor/limit，再对当前页 table 去重并以固定正数有界并发读取 rows 和计算摘要；页外表不得因当前页读取而扫描 rows，跨 pair 的奇数 cursor 仍须保持全局 sortKey 与 nextCursor。

Settings section 与 Agent config 的文件版本分别固定为 `settings-v2:<sha256>`、`agent-config-v2:<sha256>`，统一复用 `src/lib/semantic-version.ts`，不得退回 FNV32。Settings 摘要覆盖公开 section 的确定性 JSON；Shell 主题按钮、命令面板与兼容 Connected Apps 面板的用户写入统一经 `settings-write-adapter` 调用 FileSystem registry，并与 provider 共用规范 section FileRef 锁，UI 不得直接修改 theme/connection store。连接 register/deregister 是瞬时宿主生命周期，仍位于该用户 mutation 锁域之外；同 id 替换必须以 registration generation 隔离，旧 disposer/revoke 不得删除新实例。Agent 摘要还显式绑定 section、workspace 的 tasks 依赖，以及不含密钥内容的 credential configured/revision；默认 provider 在 settings 的首个 `stat/read/write/invoke` CAS 计算版本前必须等待安全存储水合，runtime 解析全局模型也必须先等待同一水合，摘要不得包含 key 或 key hash。已水合快路径不得再次访问 secure-store 或发布失效通知，避免 `watch -> read -> hydrate -> watch` 自激。SubtleCrypto 摘要使带版本的 Agent `watch` 成为异步事件：同一 watcher 必须按源顺序交付，dispose 或订阅建立回滚后丢弃 pending digest，跨窗口无版本失效必须推进 generation 并压掉更早的瞬时版本。Agent runtime task mutation 统一经 `agent-task-write-adapter` 获取 `config:tasks` 锁，并在锁内先重读耐久 revision 再调用 `*Raw` Storage 原语；通用 thread 写入和删除固定按 `tasks -> thread` 取锁，因此其事务内 task revision 推进也不能穿过旧 CAS。Workspace 的公开 state 与内部十进制 `_revision` 写入同一份 localStorage JSON，单次 `setItem` 后才发布内存快照；legacy 文档按 revision `0` 读取。即使公开正文损坏，只要 raw 对象仍携带合法 `_revision`，后续普通写入或强制修复也必须把它作为单调下限，写入 `max(memory, rawFloor)+1`，不能让其它窗口永久拒绝降序结果。revision 限制为 64 位；空间耗尽时，真实 mutation/rewrite 必须在任何 secure/public 写入前拒绝，但 identity update、当前 active、删除不存在项和同正文 replace 等 no-op 不请求下一 revision。Workspace secure entry 使用版本记录 `{version:2,target,apiKey,revision}`；其中 record revision 是本次 secure-first 写入预期提交的公开 envelope revision，清除凭据写入 `{target:null,apiKey:""}` 墓碑，不依赖删除。`target` 是仅允许 HTTP(S)、清除 userinfo/query/fragment 后的 canonical baseURL；只有 target 精确匹配且 record revision 不超前于 envelope 才恢复 key。record 超前表示公开提交中断，重载后也必须 fail closed，并在锁内以墓碑完成该 intended revision；旧 v1/bare 值只在锁内迁移。端点变化且未显式提供新 key 时同样写墓碑。secure 写入必须先 await 成功，再持久化公开快照并通知；公开 envelope 失败时尝试回滚，无法证明回滚成功则凭据缓存 fail closed，但仍不宣称 secure-store 与 localStorage 组成跨后端原子事务。runtime workspace mutation 统一经 `agent-workspace-write-adapter` 按 `tasks -> workspaces` 取锁，锁内刷新耐久快照后调用 `*Raw`；UI updater 必须基于该 fresh state 合并，不能用陈旧整对象覆盖其它窗口的字段。workspace revision 参与语义版本，公开正文相同的 ABA 或隐藏凭据变化也会使旧 CAS 失效；workspace 的 `stat/read/write` prepare 与快照均持有两把锁，tasks 自身仍只持 tasks。专用失效广播只携带 scope，远端窗口和页面恢复路径都须重读耐久 revision。provider/importer 已持规范锁时只能直接调用 Raw，禁止重入 adapter；整包 importer 继续按稳定全序正序获取六个 section FileRef 并逆序释放。

**Agent Workspace 草稿纪律**：目录、instructions、baseURL、model、template、override 与 API key 文本使用同一类单尾串行 debounce 队列。未开始的输入可合并，已在途提交不得被后续输入越过；`keep` 失败的最新 generation 不自动自旋，只保留到下一次显式 flush/cleanup 重试，新输入会取代它。外部快照不得覆盖 dirty generation，提交返回的 Storage 实际值用于识别 canonical acknowledgement。收敛判断必须同时比较值和 workspace source revision，使更高 revision 的同值 ABA 也能结束等待并采用远端真值。跨 await 的“生成最终提示”必须捕获 `workspaceId + generation` operation token；工作区切换、卸载或随后编辑都会使旧生成失效，保存后采用结果也必须再次校验。模型 global/preset/custom 的直接选择必须先等待 baseURL、model、API key 三条草稿队列全部 settle，再以 selection generation 串行应用；快速连续选择以最后意图为准，旧 debounce 不能反向覆盖。API key 提交必须先 flush baseURL，并在 `tasks -> workspaces` 锁内校验捕获的 canonical target；target 已变化或无效时拒绝并要求重新输入。

**外部 ACP Agent 纪律**：反向客户端实现集中在 `src/plugins/agent/lib/acp/acp-client.ts`。program / argv / cwd 只能来自设备本地用户设置，不得接收模型输出、网页内容或工具返回；argv 直接交给 Rust `Command`，禁止改成 shell 字符串。客户端不得默认声明文件/终端能力或注入 ideall MCP。新增权限类型仍须投影成不含 rawInput/rawOutput/路径正文的 `AgentToolPreview`，获批后先写 pending 审计；取消、超时和连接关闭必须收尸。协议测试使用 SDK 内存 Agent，真实 CLI echo 冒烟可用 `scripts/acp-echo-agent.mjs`。

**Local Data 修复纪律**：有 owner durability 协议的 schema 不得由共享诊断层直接 `setItem`。`repairMutation` 必须把 fresh inspect → repair → apply → inspect 放进 owner 规范锁域，`applyRepair` 必须经专用 force Raw store 提交并推进 revision、发布失效；即使修复后的公开正文与内存相同，也要覆盖坏 revision/同 revision 异 token。仅注入 Storage 的隔离测试可使用默认直接 patch。

**战略方向 D（已定）**：ideall **能独立成立**，命运不与 wonita 深度绑定。

- **本地核心不依赖后端**：“我的”（home）/ tool 本地能力与 BYO-key agent 可离线使用。跨端同步只传密文，但同步动作需要登录账号和可用的 Sync 服务；当前同步范围为关注（`subs`）、笔记（`notes`）、书签与收藏夹（`bookmarks`）。
- **wonita 是默认且最好的后端选项，而非命根**：取数经 `@protocol/server-port` 的 `ServerPort` 契约，**后端可换 / 可自建**；wonita 只是默认与参考实现。后端可换 / 可自建是核心对外能力，必须兑现。
- **开源核心商业模型**：ideall（开源免费）做漏斗与信任护城河；wonita（闭源付费）以“语料级智能”变现。免费/付费线守在“语料级智能”（聚合 / 知识图谱 / 实体事件追踪），基础本地功能不入墙。
- **目标盘**：信息密集型专业人士，以及重视数据自持的高级用户 / 极客（非大众市场）。
- **community / peer 发布降级为远期可选赌注**：Follow、腾讯 ima 已大规模占据，验证本地产品留存后再投。

**home 是“我的”（本机数据区）；info / community / tool 是三个发现模块，其内容经关注汇入“我的”**：“我的”通过**关注**把“发现”里的来源（发布者 / 实体 / 工具 / 搜索 / 社区发布者 peer）加入 `/home/following` 关注流；关注偏好本地优先（IndexedDB），内容实时拉取。
**跨端同步（端到端加密、账号绑定密文）**：登录账号只负责密文存储授权；同步码仍只在客户端派生 `storageId` 与 AES 密钥，服务端不能解密。JSON 按 UTF-8 字节切成不可变 generation parts，全部上传后以 manifest 版本 CAS 原子发布；409 会有界重试。本地落地由 `StorageSyncPort` 在同一 readwrite 事务内按完整逻辑快照 CAS，不会用陈旧合并结果覆盖同步窗口内的本地写。
**社区是用户 / peer 发布层（账号）**：登录后发布内容成为社区发布者，他人可关注其发布并汇入关注流。
**账号与同步码职责分离**：账号决定谁能访问密文，同步码决定谁能解密；跨设备需同时使用同一账号和同一同步码。

## 架构：终端分层（src/ 下）

> 本节是日常开发速查；完整架构（领域模型 / 数据流图 / 不变量）见 [architecture.md](architecture.md)。

`src/` 已按终端分层重组：路由薄标记 / 终端外壳 / 一切皆标签 / 一切皆文件 / 功能模块 / 插件 / 契约 / UI 原语 / 纯工具。

| 目录 | 别名 | 内容 |
| --- | --- | --- |
| **app** | `@/app/*` | Next 薄路由层——`(workspace)/[[...path]]/page.tsx` 统一分发工作区目标，`(standalone)/auth/page.tsx` 渲染独立登录页；其余仅保留根 layout/error/global-error/loading/not-found、图标和全局样式。静态深链由 `src/workspace/static-routes.ts` 声明 |
| **shell** | `@/shell/*` | 终端外壳 —— 五分区导航 / 命令台 / header / bottom-tab-bar / 主题 / account / mobile-nav，以及唯一组合根 `boot`、启动闸 `boot-gate` 和可信扩展生命周期 `runtime-extensions` |
| **workspace** | `@/workspace/*` | Display 编排 —— File + Engine 标签、三种工作区、目录树、标签生命周期、脏 Engine 休眠，以及仅位于深链解析/工作区水合边界的旧 Resource/Node 标签迁移 |
| **filesystem** | `@/filesystem/*` | FileSystem registry、隐藏合成根、`ideall.navigation` 路径链接、provider 挂载、`statMany/readMany` 批量读取与 watch 生命周期；`resource-file-system` 将 `resource-sources/` 下的 Node/连接数据 Resource source/provider 适配到 `ideall.core`，`app.settings` 提供基本设置根与四个 section 快照 |
| **engines** | `@/engines/*` | Engine descriptor、匹配、默认选择以及按工作区隔离的偏好 |
| **files** | `@/files/*` | 一切皆文件——统一 Node 数据层。`stores/`（各 kind store + `nodes-store` 跨 kind 协调层）；顶层 Node 原语：note-blocks / sort-key / notes-tree-util / note-write-queue / flowback / feed-node（关注↔feed 节点投影）/ `files-port`（经 FileSystem registry 的兼容外观）/ `storage-sync-port`（同步专用原子存储面）/ bookmark-import |
| **modules** | `@/modules/*` | 功能模块——`home`（“我的”：本机笔记/书签/资源/关注/对话的功能 UI = overview/notes/bookmarks/resources/publications/subscriptions）/ `info` / `community` / `tool` / `apps`（本机已安装应用启动器，Tauri 桌面专属；由 `src-tauri/src/installed_apps.rs` + `src/lib/installed-apps.ts` 支撑） |
| **plugins** | `@/plugins/*` | 插件——`agent`（AI 环境层，BYO-key）/ `sync`（跨端 E2E 同步）/ `embed`（嵌入页 + AI 共用的 Grant→`createLocalMcpServer` 能力链路）/ `code` / `git` / `shell` / `audio` / `database`；公共插件数据能力在 `plugins/shared` |
| **protocol** | `@protocol/*` | 契约 / 端口（纯类型 / 纯函数，不含运行时实现与 UI）：file-system / engine / node / files（FilesPort 兼容领域外观）/ storage-sync / note-merge / subscription / content / flowback / sync / api-result / server-port |
| **ui** | `@/ui/*` | shadcn 原语 + 块编辑器（`editor/`） |
| **shared** | `@/shared/*` | 跨层共享 UI；`feeders/` 仅保留工具固定按钮 `PinToolButton` 与统一回流提示 `flowbackToast` |
| **lib** | `@/lib/*` | 工具与运行时适配——utils/format/idb/id/sync-crypto/auth/api（wire DTO 生成物）/server（HTTP 适配器、端口 registry 与社区 facade）/ui-actions/active-node/safe-url/theme/env/tauri/updater 等 |

**别名**：`@/*` → `src/*`；`@protocol/*` → `src/protocol/*`（其余层一律使用 `@/<layer>/...`；app 路由使用 `@/app/*`、`@/app/globals.css`）。

ESLint 强制五条边界：

1. **protocol 纯度**：契约层只可使用同目录相对依赖，不得 import `lib` 运行时、功能层或 UI。
2. **wire DTO 边界**：后端数据服务的 OpenAPI 生成类型（`@/lib/api/server`）仅允许 `@/lib/server` 与 `@/lib/api` import；protocol 与业务代码一律使用 `@protocol/server-port` 领域类型。
3. **app 路由不可被反向 import**（路由层只分发打开目标或工作区命令）。
4. **modules 三应用 info/community/tool 互隔**（互不 import）。
5. **plugins 不反向 import shell/workspace**：插件触达工作区只能经 `@/lib/ui-actions` / `@/lib/active-node` 端口。

### 依赖反转（模块经 protocol 而非互相直连）

- **内容 feed**：“我的”关注流调用 `@protocol/content` 的 `resolveSubscription`；info/community 在各自 `manifest.ts` 注册 resolver（info 管 publisher/entity/search，community 管 peer）。
- **本机文件数据**：`@/shared/feeders/pin-tool-button` 与 agent 插件经 `@protocol/files` 的 `getFilesPort()`
  使用兼容领域 DTO；普通 CRUD 由该外观继续分派到 FileSystem registry，不直接依赖底层 Node 存储。只有 sync 经 `StorageSyncPort` 使用含墓碑全量、快照 CAS 与原子 bulk；bulk 返回 Storage 规范化后的实际提交快照。
- **跨端同步**：“我的”同步面板调用 `@protocol/sync` 的 `getSyncPort()`；sync 插件 `manifest.ts` 注册 SyncPort。
- **后端取数（wonita 服务数据服务）**：`@protocol/server-port` 只定义 ServerPort 与自有领域类型；
  运行时通过 `@/lib/server/port-registry` 的 `getServerPort()` 取实现。默认 HTTP 适配器对接 wonita
  server，也是**唯一** import wire DTO 的位置；App、嵌入式、局域网节点和测试可经
  `registerServerPort()` 覆盖。这是 ideall 作为外部消费方、不被单一后端绑死的支点。
- 启动注册由 `@/shell/boot-gate.tsx`（客户端启动闸，挂在根 layout）调用 `boot.ts#registerAll()` 完成（组合根，import 各模块 manifest）。

## Common commands

> 环境：Node ≥ 22、pnpm 9；Tauri 另需 Rust ≥ 1.77.2 与平台系统库 —— 见 [README.md#开发环境](../README.md#开发环境)。

```bash
pnpm install
pnpm dev          # 浏览器开发服（SSR）http://localhost:5020
pnpm build        # 生成本地语义 Worker/runtime，再执行 Next 静态导出 → out/
pnpm semantic:bundle # 生成 public/generated 下的按需编排器、Worker 与本地 ONNX WASM runtime
pnpm verify:semantic-runtime # 校验语义 Worker raw/gzip 与 WASM runtime 预算
pnpm clean:next   # 清理 .next/，verify:base 会在 typecheck 前自动执行
pnpm lint         # 含 protocol 纯度强制（no-restricted-imports）
pnpm lint:deps    # 检查未使用、未声明与多余依赖
pnpm lint:dead-code # 检查未被产品、测试、Worker 或 fixture 入口引用的孤立文件
pnpm lint:docs    # 检查仓库内文档链接与 docs/README.md 收录完整性
pnpm version:check # 校验 package/Tauri/Cargo/Cargo.lock 版本一致
pnpm test         # tsx + node:test（经 scripts/run-tests.mjs；可加子串过滤，如 pnpm test sort-key）
pnpm test:coverage # 为选定核心源码生成 c8 text/lcov，并检查仓库覆盖率基线
pnpm test:scripts # 维护脚本的 node:test 测试

# App（Tauri 跨平台桌面/移动；工程在 src-tauri/，见 docs/app.md）
pnpm app:dev      # 复用或启动 Next 开发服，再启动桌面开发壳
pnpm app:export   # build → 关键静态入口检查 → bundle 预算（生产导出统一入口）
pnpm app:build    # 为当前宿主平台打包（跨平台矩阵由 CI 在各平台构建）

# 本地基础门禁（对应 CI 基础检查；会先清理 .next 再 typecheck）
pnpm verify:checks # 不含生产构建的质量门禁，包含依赖与孤立文件检查
pnpm verify:base   # verify:checks → app:export
pnpm verify:bundle # 检查已有 out/ 的 JavaScript raw/gzip bundle 预算
pnpm verify:static-export # 检查 out/ 静态导出关键入口与 _next chunks
pnpm verify:smoke:static  # 生产形态浏览器冒烟：app:export → serve out/ → notes/files/plugins/trash
pnpm verify:full          # verify:base + 开发服冒烟（自动挑 5020-5023 可用端口）

# API codegen（后端数据服务的 schema 变更后）——产物是 wire DTO，仅供 HTTP 适配器消费
pnpm gen:api      # openapi/server.json → src/lib/api/server.d.ts（离线，普通贡献者只需这一步）
pnpm gen:api:check  # CI 卡点
# 维护者刷新契约源（拿到后端新导出的 openapi.json 时）：
SERVER_LOCAL=/abs/path/to/openapi.json pnpm sync:api
```

脚本入口、参数与新增脚本约定见 [scripts.md](scripts.md)。

## 形态：App-only（Tauri 跨平台）

ideall 仅以 App 形态分发（当前流程见 [app.md](app.md)，历史迁移见 [app-history.md](app-history.md)）：

- **构建目标**：`output: export` 静态导出（默认且唯一生产构建），Tauri 2.0（`src-tauri/`）以 **Windows / Linux / macOS / Android / iOS** 为目标。当前自动发布只覆盖桌面；Android 仅有 CI debug APK，iOS 未入 CI。无 Node 运行时 / 无 SSR 生产部署，数据层走**客户端直连后端**（`NEXT_PUBLIC_SERVER_ADDR`）；数据访问已同构客户端化。
- **开发**：`pnpm dev` 仍是本地 SSR 开发服（`pnpm app:dev` 会复用或启动它），不影响导出。
- `lib/env.ts` 的 `SERVER_ADDR` 同构：App 客户端 / 浏览器直连 `NEXT_PUBLIC_SERVER_ADDR`；`pnpm dev` 的 SSR 渲染期服务端读取 `SERVER_ADDR`。客户端直连需后端放行 CORS（App 内 agent 经 `tauri-plugin-http` 绕过）。

## Conventions

- 默认使用 Server Component，仅交互组件添加 `"use client"`。
- UI 复用 `src/ui` 的 shadcn 原语，禁止引入并行 UI 库；视觉决策（阴影 / 颜色 / 圆角 / 间距 / 公共组件）以 [docs/design/ui-style.md](design/ui-style.md) 为准。
- 脚本公共能力集中在 `scripts/lib/`；新增验证/冒烟脚本时优先复用 `process.mjs` 中的命令执行、
  端口探测、HTTP 就绪等待与子进程清理逻辑。`scripts/` 根目录只放可执行入口和对应测试，
  对外长流程脚本应支持 `--help`。
- TypeScript strict；后端取数与 DTO 一律经 `@protocol/server-port`（ServerPort + ideall 自有领域类型）。**业务/protocol 代码禁止 import wire DTO**（`@/lib/api/server`），它仅供 `@/lib/server` 适配器消费。
- 所有 fetch / 数据访问函数必须 `try-catch` + `res.ok` 检查
- 用户可见文案与代码注释均使用简体中文
- **新增功能模块 / 插件**：在 `src/modules/<name>` 或 `src/plugins/<name>` 建模块和 `manifest.ts`，在 `@/shell/boot.ts` 注册；跨模块交互一律经 `@protocol`（新增契约/端口加到 `src/protocol`）。
- **边界**：protocol 不得 import UI/页面（`pnpm lint` 强制）；app 路由不可被反向 import，info/community/tool 互不 import。
- `ideall.core` 的笔记、书签、资源、关注和对话使用统一 Node 对象仓 `STORE_NODES`；音频、数据库、Agent 配置等 provider 保留各自 Storage。本地数据不自动整库上传；同步、发布、模型、外部 MCP 与 web 工具的接收方、数据范围和控制见[数据出站矩阵](../.github/SECURITY.md#数据出站矩阵)。
- 从本地内容派生社区发布时，草稿继续使用普通 Note Node，不新增页面私有 store。来源只在用户主动生成时复制为有界快照；远端 mutation 必须经过公开预览、明确确认和副作用前 pending 审计，transport/5xx 等不确定结果不得自动重试，审计中不得保存草稿正文、URL 或来源身份。
