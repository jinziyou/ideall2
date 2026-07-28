# GPUI 桌面组件边界

> 状态：现行。本文描述 `native/apps/ideall-desktop` 的代码组织和挂载状态；
> 视觉规则见 [ui-style.md](ui-style.md)，跨 crate 依赖见
> [architecture.md](../architecture.md)。

## 目标

桌面壳采用浅色、低噪声的 AI 原生面板工作台；框架使用紧密网格，文档使用有意留白，
并以深色纵向导航和智能绿色强调当前上下文。
组件边界不等于导航入口。
组件可以先具备独立状态、动作和测试，再由产品能力评审决定是否挂载到界面。

`IdeallDesktop` 是组合根，只负责把 application 用例、组件状态和窗口生命周期连接起来。
可复用业务规则不得放进 GPUI 组件，共享 crate 的公开 API 也不得暴露 GPUI 类型。

## 当前结构

```text
native/apps/ideall-desktop/src/
├── main.rs              # 组合根、application 调用、持久化与后台任务编排
├── actions.rs           # 工作台级快捷键
├── view.rs              # 顶栏、侧边导航/活动栏、标签壳、设置/Agent 等页面组合
├── windowing.rs         # 主显示器选择、窗口定位与 X11 RandR 适配
├── editor/
│   ├── mod.rs           # 文档、草稿、输入与光标会话状态
│   ├── actions.rs       # 标签切换
│   ├── commands.rs      # Notion 式块/行内命令与 GPUI 输入事务桥接
│   └── view.rs          # 笔记工具栏、斜杠菜单、EditorMode 与 Engine 视觉
├── explorer/
│   ├── mod.rs           # 树选择、折叠、重命名与面板布局状态
│   └── view.rs          # 树节点视觉与拖拽预览
├── browser/
│   ├── mod.rs           # URL 校验、历史与 WebView 生命周期
│   ├── actions.rs       # 受限导航和平台降级
│   └── view.rs          # 独立 Browser Surface、地址栏与浏览器状态栏
└── shell/
    ├── mod.rs           # 输入、运行状态、订阅和任务生命周期
    ├── actions.rs       # GPUI 后台任务桥接
    └── command.rs       # 与 UI 无关的 argv 解析和进程执行
```

## 挂载状态

| 组件 | 当前状态 | 产品入口 |
| --- | --- | --- |
| `editor` | 已挂载 | 文件/笔记/书签元数据标签中的编辑区域 |
| `explorer` | 已挂载 | 左侧活动栏中的目录区域 |
| `browser` | 已挂载 Surface | 承接书签 Engine 与资讯/社区门户；拥有独立视图，但没有一级导航页 |
| `shell` | 未挂载 | 没有导航、快捷键、页面或 Agent 工具；Engine capability 仍为 unavailable |

新增组件不得仅为“占位”修改 `NAVIGATION`。只有交互、生命周期、安全模型和平台降级均通过
评审后，才能增加可见入口。

## 组件职责

### Editor

- 持有当前文档、草稿脏状态、自动保存版本和每文档光标会话。
- 根据文件元数据选择代码高亮语言。
- 笔记提供块类型、斜杠命令、行内格式、块重排/复制/删除、待办和层级操作；完整行为见
  [desktop-editor.md](desktop-editor.md)。
- 块变换规则位于 `ideall-application::note_edit`，桌面和移动端共用；GPUI 层只负责
  UTF-16 选区、最小文本事务、焦点与状态反馈。
- Engine 偏好和 workspace 快照由 application/domain 持久化，组件只持有 UI 状态。

### Explorer

- 持有可见树、选择、折叠集合、重命名输入和侧栏尺寸。
- “私有页面”和“回收站”作为同级虚拟树根；节点行按 `parent_id` 与 `depth` 呈现层级。
- 根节点提供直接新建和更多菜单，文件夹节点的上下文菜单将新建与导入目标绑定到当前选择。
- 树变更通过 `LocalWorkspace` 用例完成；组件不得直接访问 SQLite。
- 布局状态使用稳定排序后写入 `WorkspaceState`。

### Browser

- 只接受 HTTP(S)，拒绝控制字符、超长地址和 URL credentials。
- WebView 不注入本地文件、凭据或隐式 bridge。
- Browser 是标签宿主中的独立 Surface，不进入 `render_editor`，也不复用文档标题输入、查找、
  保存和行列状态栏；书签仍以 `FileRef + ideall.browser` 保持统一标签身份。
- Surface 提供地址导航、前进/后退、刷新、外部打开和返回书签编辑 Engine；浏览历史仍只属于
  Browser 会话状态。
- macOS/Windows 使用受限子 WebView；Linux 当前明确交给系统浏览器。
- Browser 历史属于组件会话状态，不进入 Node 正文或 workspace 标签身份。

### Shell

- program 与 argv 分开传递，禁止经系统 shell 解释变量、重定向或命令替换。
- 带目录的程序必须使用绝对路径；工作目录必须是现有绝对目录。
- 子进程清空环境后只复制受控的基础环境变量，stdin 固定关闭。
- stdout/stderr 各保留最多 512 KiB，继续排空管道避免子进程阻塞；单次运行 30 秒超时。
- 组件析构会请求取消任务，但在具备 PTY、目录授权、进程隔离和审计前不得挂载到产品界面。

## 变更检查清单

1. 状态和纯逻辑进入对应组件目录，`main.rs` 只保留组合与 use case 编排。
2. 共享规则优先进入 domain/application，避免在桌面和移动端复制。
3. 新后台任务必须有取消、超时、输出预算和迟到结果处理。
4. 新可见入口必须同时更新 presentation 导航、键盘行为、空态、文档和人工截图验收。
5. 至少运行：

```bash
pnpm native:check
pnpm native:test
pnpm native:lint
pnpm lint:docs
```
