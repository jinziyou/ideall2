# FileSystem 与 Engine 架构

ideall 使用同一套五层模型组织本地内容、打开方式与界面：

```text
Storage → FileSystem → FileRef → Engine → Display
```

## Storage

Storage 是物理数据与事务边界。当前权威实现是 `ideall-storage` 的 SQLite 数据库；同步、
安全存储和网络是独立适配器，不伪装成 UI 状态。

- Node、Blob、回收快照和 Agent 审计在事务内保持引用一致。
- 全文/语义投影属于可删除、可重建缓存。
- 凭据不进入 SQLite。
- V2 归档导入先写临时数据库，校验成功后再原子替换。

## FileSystem 与 FileRef

`FileRef` 是内容稳定身份，目录路径、名称和排序只是投影。移动、重命名或从不同目录链接同一
对象时，不应改变其身份。

FileSystem 对上提供受控的读取、列举、写入和动作语义；provider 保留自己的权限、事务和
版本规则。Display 和 Agent 都不能绕过 FileSystem 直接访问底层 Storage。

关键约束：

- metadata 不携带需要额外授权的正文、Blob 或凭据；
- mutation 使用版本/CAS 语义，冲突不能静默覆盖；
- destructive action 在执行点再次校验权限和目标；
- Agent 访问额外受实时 Grant 和脱敏写审计约束；
- 目录分页、批量读取和后台任务都有明确数量/体积预算。

## Engine

Engine 描述“如何打开一个 FileRef”。匹配输入包括 media type、文件属性、显式用户选择和
平台 capability。标签身份由 `FileRef + Engine` 组成，因此同一内容可以同时用 Editor、
Preview、Code 或 Browser 打开。

Engine registry 位于 domain 层，只保存无 GPUI 类型的 descriptor 与选择规则。具体 renderer
由桌面或移动组合根注册：

- 桌面可组合 Editor、Preview、Code、Browser 与平台外部打开；
- 移动端按触屏、系统输入代理和平台能力选择 renderer；
- 不可用能力返回明确降级，不显示无法兑现的入口；
- Browser、Shell 等 Engine 不升级为一级导航分区。

## Display

Display 是短生命周期投影，不是数据权威：

- GPUI Entity 持有选择、焦点、展开状态和未提交草稿；
- application service 提供用例和持久化结果；
- 数据库、网络、同步与 Agent 工作在后台；
- 后台任务通过受控任务返回 GPUI executor，不能直接跨线程修改 Entity。

桌面布局固定为“侧边导航栏 → 纵向活动/文件树 → 标签内容区”。活动栏只负责树形浏览和节点
操作；Editor/Preview/Browser 在内容区切换。移动端使用导航栈和底部栏，不照搬桌面三栏。

## Agent 边界

Agent 只能通过 Grant 后的 MCP/FileSystem 能力访问数据。写操作先建立最小、脱敏的 pending
审计，再执行 mutation 并结算结果；不记录原始密钥、完整工具参数或无关正文。外部 ACP
Agent 是用户主动启动的系统进程，不等同于 OS 沙箱。

## 依赖位置

- 契约：`native/crates/ideall-protocol`
- Engine/Workspace 规则：`native/crates/ideall-domain`
- FileSystem 用例：`native/crates/ideall-application`
- SQLite：`native/crates/ideall-storage`
- 桌面组合：`native/apps/ideall-desktop`
- 移动组合：`native/apps/ideall-mobile`

旧 TypeScript provider、Tauri capability 与 WebView bridge 只存在于
`legacy/tauri-v2`，不再是默认分支的实现参考。
