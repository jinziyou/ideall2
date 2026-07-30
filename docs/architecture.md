# GPUI 原生架构

ideall 的唯一生产实现位于 `native/` Cargo workspace。桌面与移动壳使用不同的 GPUI
revision，但共享不含 UI 类型的协议、领域、用例和适配器。

## 分层

```text
protocol → domain → application → adapters → UI → platform app
```

- `ideall-protocol`：稳定 ID、Node/FileRef、同步与归档 wire 契约。
- `ideall-domain`：Engine、Workspace、导航和纯业务规则。
- `ideall-application`：创建、编辑、回收、恢复、导入等跨界面用例；固定 id 的「收集箱」
  目录（`INBOX_ROOT_ID`）承接未整理捕获。
- `ideall-presentation`：文案、导航模型、设计 Token 和无 GPUI 类型的视图状态。
- `ideall-storage`：SQLite 权威数据、迁移与归档。
- `ideall-sync` / `ideall-sync-http`：端到端加密同步与网络适配。
- `ideall-agent` / `ideall-acp`：Grant、MCP 工具、模型传输和外部 ACP 客户端。
- `ideall-secrets` / `ideall-updater`：平台凭据库与签名更新。
- `ideall-webmeta`：书签页面的有界 HTML 元数据抓取（标题、摘要、favicon 与 canonical）。
- `ideall-desktop` / `ideall-mobile`：平台组合根和 GPUI Render 层。

共享 crate 的公开 API 不得包含 GPUI、SQLite、HTTP 或平台系统类型。依赖只能沿上图向右；
UI 不得直接穿过 application 层修改存储。

## 核心模型

```text
Storage → FileSystem → FileRef → Engine → Display
```

- `FileRef` 是内容的稳定身份，路径只是目录投影。
- FileSystem 负责 `stat/read/write/list/watch/action` 与授权边界。
- Engine 决定打开方式；标签身份是 `FileRef + Engine`。
- Display 只渲染 application/presentation 提供的状态，不持有数据库权威数据。
- Agent 只能通过 Grant 后的 MCP/FileSystem 能力访问内容，写操作必须留下脱敏审计。

## 运行时

SQLite WAL 是本地权威存储。写操作串行提交，读取和长耗时工作放到后台；后台线程不能直接
修改 GPUI Entity，结果通过受控任务回到 GPUI executor。应用启动时安全存储、数据目录和
SQLite 初始化失败会进入可恢复错误页。

桌面壳维持三段结构：

```text
侧边导航栏 → 纵向活动/文件树 → 标签内容工作区
```

活动栏只使用纵向文件树，不设横向工具条。新建、重命名、移动、删除等低频操作进入节点
菜单或快捷键。Editor、Preview 与 Browser 是 Engine，不是额外一级导航。

移动壳使用单列导航栈与底部栏，复用业务规则和设计语义，但不复用桌面 GPUI 组件类型。

## 数据与迁移

笔记、书签、关注、对话、同步状态与 Agent 审计默认写入本机 SQLite。模型 Key、同步码、
登录凭据和扩展 consent 只进入平台安全存储，不写入数据库、归档或日志。

旧数据只通过 V2 工作区归档导入：

1. 旧版本导出归档；
2. importer 校验 envelope、密码学参数、CRC32、引用与预算；
3. 在临时数据库中执行单事务导入；
4. 成功后原子替换，并保留导入前备份。

旧 Next.js/Tauri 版本冻结于 `legacy/tauri-v2`。它不属于默认分支的构建图，也不得与 GPUI
版本共享数据目录。

## 外部能力

- 同步服务只接收端到端加密密文。
- OpenAI-compatible 模型端点由用户配置；发送内容以当前会话和显式授权上下文为界。
- 外部 ACP Agent 以当前系统账户运行，不等同于 OS 沙箱，只应配置可信程序。
- 签名 connector 在启动前验证 manifest、Minisign 与 executable SHA-256，但进程外隔离
  仍不等同于内核沙箱。
- Web 内容只进入受限 Browser Engine；Linux 无法安全内嵌时使用系统浏览器。

## 发布边界

`rust.yml` 负责 workspace、桌面三平台及移动模拟器门禁，`native-release.yml` 负责签名
安装包与更新清单。Node 脚本只做仓库维护和发布产物验证，不承载产品运行时。

更细的模块和平台说明见 [native/README.md](../native/README.md)，FileSystem/Engine 契约见
[file-system-engine-architecture.md](file-system-engine-architecture.md)。
