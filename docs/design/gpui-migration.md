# GPUI 全平台原生化迁移

> 状态：生产代码与发布入口已切换到 GPUI。共享核心、五平台 CI、桌面安装包、Android
> 模拟器与 iOS Simulator 门禁已落地；真机输入、无障碍与商店签名仍需按发布手册人工验收。
> Next.js/Tauri 只保留旧数据 V2 归档导出兼容性，不再生成生产 App。

## 1. 决策

ideall 将采用同一套 Rust 领域核心覆盖五个平台：

- macOS、Windows、Linux 使用 GPUI 与 gpui-component；
- iOS、Android 使用 GPUI 与 gpui-mobile；
- Node、FileSystem、Engine、Workspace、同步、Agent、归档和本地数据库跨平台共享；
- 桌面和移动分别组合符合输入方式的 Display，不强行共享一套壳布局；
- 新闻、社区和浏览器仍是隔离的外部 Web 内容，只允许出现在受限 WebView Engine 中；
- 旧 Tauri 壳只作为只读迁移兼容入口；GPUI 是唯一生产 App 与发布目标。

本迁移借鉴 OxideTerm 的“UI 与领域服务同进程”方向，但不复制其 GPL-3.0 代码或
vendored GPUI fork。ideall 继续使用 Apache-2.0 许可；仓库内 gpui-mobile 快照选择其
Apache-2.0 许可，并保留未修改的上游 Apache/GPL/AGPL 许可证与 revision 来源说明。
每个引入的 Zed、GPUI、gpui-component 与 gpui-mobile 文件或 crate 都需独立核对许可证。

## 2. 不变量

迁移不得改变以下产品语义：

1. 本地优先，无账号和断网时仍能使用本地核心。
2. `FileRef` 是稳定身份；路径和目录项只是投影。
3. 标签身份是 `FileRef + Engine`，Workspace 切换不改变文件身份。
4. Display 不绕过 FileSystem registry 访问底层 Storage。
5. Agent 只能经 Grant → MCP/FileSystem 能力链访问数据。
6. 笔记、书签、关注和同步 wire 在共存期与 0.2 客户端兼容。
7. 凭据只进入系统安全存储，不进入数据库、归档、日志或 Display 快照。
8. V2 工作区归档保持可读；迁移失败不能修改原数据或留下半导入数据库。

## 3. 目标工作区

生产代码位于独立的 `native/` Cargo workspace；`src/` 与 `src-tauri/` 不参与生产构建：

```text
native/
├── apps/
│   ├── ideall-desktop/
│   └── ideall-mobile/
├── crates/
│   ├── ideall-protocol/
│   ├── ideall-domain/
│   ├── ideall-application/
│   ├── ideall-storage/
│   ├── ideall-filesystem/
│   ├── ideall-engines/
│   ├── ideall-workspace/
│   ├── ideall-sync/
│   ├── ideall-agent/
│   ├── ideall-platform-desktop/
│   ├── ideall-platform-mobile/
│   ├── ideall-ui-desktop/
│   └── ideall-ui-mobile/
└── vendor/
    └── gpui-mobile/
```

早期阶段只建立产生实际依赖的 crate，避免预先制造空包。依赖方向固定为：

```text
protocol → domain → application → adapters → UI → platform app
```

`protocol` 与 `domain` 禁止依赖 GPUI、SQLite、HTTP、系统 API 或具体 UI。
`ideall-presentation` 只保存无框架类型的导航、文案、语义色和异步请求协调状态；桌面和移动
Render 模块分别组合各自 GPUI revision，避免复制产品语义。

## 4. GPUI 依赖策略

GPUI 仍处于 pre-1.0。桌面与移动入口是不同二进制，可以暂时使用各自经过验证的 GPUI revision；任何共享 crate 不得在公开 API 中暴露 GPUI 类型。

首个验证基线：

| 依赖 | 固定版本 |
| --- | --- |
| desktop `gpui` | crates.io `=0.2.2` |
| `gpui-component` | crates.io `=0.5.1` |
| `gpui-mobile` | 本地源码快照，上游 `1d3ec2a1d14a63b74d1f4269340441d4eeada27a` |
| mobile `gpui` / `gpui_wgpu` | Zed `74798c68d5c63d31e2ccca5c8f5ec0a02c90679c` |

所有可直接控制的 Git 依赖必须使用完整 revision；上游传递 manifest 仍使用 branch 时，
必须由提交的 `Cargo.lock`、机器可读期望 revision 与 CI 漂移检查共同固定。标准入口与 CI
必须使用 `--locked`。gpui-mobile 快照的来源、本地差异校验和与升级步骤记录在
[`native/vendor/gpui-mobile/README.md`](../../native/vendor/gpui-mobile/README.md)；其对
wgpu 通过 `gpui_wgpu` 重导出统一使用 crates.io `29.0.4`，由 lockfile 与 vendor verifier
共同拒绝 Git source 或版本漂移，避免移动二进制编译两套 wgpu。CI 还会从固定
gpui-mobile revision 联网比对整个 `src/` 文件集和许可证。依赖升级必须作为单独变更，同时
通过五平台构建、输入、渲染、生命周期和数据回归。

gpui-component 是桌面组件库，移动端不依赖它。移动 UI 可以复用领域状态、设计 token 和无 GPUI 类型的 ViewModel，但使用 gpui-mobile 触摸组件重新组合。

## 5. 运行时边界

- GPUI `Entity` 只拥有界面状态与短生命周期草稿。
- 数据库、网络、同步和 Agent 由 application service 持有。
- 后台任务不能从 Tokio 线程直接修改 GPUI Entity；结果通过有界 channel 回到 GPUI executor。
- SQLite 使用单写入 actor 串行 mutation；读操作使用只读连接。
- 多窗口共享 application service，不再复制一套浏览器 store。
- 外部 WebView 不获得 ambient authority；每个 bridge 请求校验 origin、tab、grant 与 action。

## 6. 数据与迁移

原生数据层使用 SQLite WAL。权威表包括 `nodes`、`blobs`、`trash_snapshots`、`agent_tasks`、`agent_write_audit` 与 `schema_migrations`；全文和语义索引是可删除、可重建 cache。

迁移只通过现有 V2 工作区归档完成，不直接解析 WebKit/WebView2 的 IndexedDB 文件：

1. 在旧版导出明文或加密 V2 归档；
2. 原生 importer 校验 envelope、PBKDF2/AES-GCM、CRC32、计数和体积预算；
3. 在临时数据库中完成反序列化、引用校验与单事务导入；
4. 导入成功后原子替换目标数据库，并保留导入前备份；
5. 密钥、登录 token、同步码与 API key 不迁移；
6. Rust 与 TypeScript 使用同一组 golden fixture 锁住 wire 兼容。

Tauri 与 GPUI 版本不得同时写同一个数据库或数据目录。

## 7. 平台能力

平台能力通过 ideall 自有 trait 暴露。业务 crate 不直接调用 gpui-mobile package 模块：

- 桌面：窗口、文件对话框、系统 keyring、安装应用、PTTY、更新器、WebView；
- iOS：UIKit 生命周期、Keychain、DocumentPicker、Share Sheet、WKWebView、Universal Links；
- Android：NativeActivity、Keystore、Storage Access Framework、Intent、Android WebView、App Links；
- 后台同步服从平台生命周期，不把桌面常驻假设带到移动端。

gpui-mobile 当前仍缺完整无障碍树、原生 View 嵌入和部分文本输入能力。ideall 已在应用层用
iOS `UITextView` 与 Android `EditText` 代理补齐正文的组合输入、选区同步和基础
VoiceOver/TalkBack 语义；更完整的 GPUI 无障碍树与通用原生 View 嵌入仍需要维护最小 fork
（当前以可追溯的本地源码快照承载）或向上游贡献，不能用空 feature 或 demo 行为冒充
完成的平台能力。

## 8. 阶段与门禁

### P0：可行性

- 五平台最小窗口可构建；iOS 与 Android 必须至少各有一台真机运行。
- 中文 IME、emoji、选区、软键盘、触摸和惯性滚动可用。
- Windows/Linux/macOS 窗口、缩放与 GPU 回退可用。
- VoiceOver/TalkBack、键盘导航和焦点模型有可执行方案。
- WKWebView/Android WebView 能嵌入 GPUI surface 并正确处理层级、旋转与生命周期。
- SQLite 和系统安全存储完成最小读写与中断恢复。

任一核心门禁失败时继续修复平台层，不进入功能批量迁移。

### P1：共享核心

- Rust FileRef、Node、Engine 与 Workspace 契约通过 TypeScript parity fixture。
- SQLite schema、事务原语、V2 archive dry-run importer 完成。
- 原生 CLI 能执行 doctor、archive inspect 和 import dry-run。

### P2：本地纵切

- 五分区导航、文件树、标签、Note、Bookmark、File、Settings 和命令面板可用。
- Workspace 可恢复；V2 归档可原子导入。
- 移动端使用导航栈和底部栏，桌面使用 Dock 和标签壳。

### P3：本地功能对等

- 编辑、回收站、全文搜索、Engine 偏好、多窗口或移动生命周期恢复完成。
- Plate 笔记未知节点无损往返；不支持的块不得静默丢失。

### P4：联网与 Agent

- 同步 wire、加密、CAS、MCP、ACP、BYOK provider、Grant 与审计完成。
- Rust/旧客户端交叉同步测试通过。

### P5：外部与专业 Engine

- info、community、browser、audio、code、git、shell、database 和扩展能力按平台完成或显式 capability 降级。

### P6：发布切换

- 桌面安装包、AAB、iOS archive、系统签名、更新签名与真实迁移验收完成。
- 连续两个 Preview 版本没有数据损坏或 schema 回滚。
- 已停止 Tauri 发布；回滚窗口内只保留 V2 归档导出兼容层。

### 当前证据（2026-07-23）

| 范围 | 已完成证据 | 尚未满足的发布门禁 |
| --- | --- | --- |
| 共享核心 | protocol/domain/storage/application/sync/agent/secrets 全 workspace 测试通过；TypeScript V2 fixture 可导入 | 真实旧客户端双向同步联调 |
| Linux 桌面 | GPUI 窗口实际启动并持续运行；SQLite schema v3 可由并发 doctor 检查；release ELF、tar.gz、DEB 与 RPM 已在本机真实生成，包内路径和 SHA-256 已校验；Linux feature 集不再链接 GTK/WebKit，官方 CI 固定 Ubuntu 22.04 并强制最高 `GLIBC_2.35` | 多发行版真实安装、Wayland、IME 与无障碍人工矩阵；本机滚动发行版产物仍为 `GLIBC_2.39`，不能替代 CI 兼容产物 |
| Windows/macOS | CI 已配置目标 check、release build、包内结构校验并上传 zip/MSI/NSIS 与 `.app` zip/DMG Preview；受保护手动 workflow 已接二进制和安装器 Authenticode、Developer ID、DMG 公证 | 实际运行 Preview/签名 workflow；真实宿主安装/卸载、启动、WebView、IME 与无障碍人工验收 |
| Android | gpui-mobile 真实 UI 在 host 编译；CI 已配置双 ABI APK/AAB 校验，并在 x86_64 模拟器真实创建笔记、经 `EditText` 代理输入标题/正文、滚动、旋转、Home/恢复、截图和 PID 连续性检查；SAF 选择器把 content URI 有界复制到私有缓存；Keystore 与 Android WebView 已接线；隐藏 `EditText` 代理已同步组合态、选区、提交文本和基础 TalkBack 语义；受保护手动 workflow 支持 keystore 签名 AAB | 新交互 CI/签名 workflow 实际结果、真机中文 IME/候选窗/选择句柄、旋转、后台恢复与 TalkBack 人工验收 |
| iOS | CI 已配置 arm64 simulator release `.app` 构建和 XCTest 交互门禁，覆盖新建笔记、`UITextView` 标题/正文输入、滚动、横竖屏、后台恢复、重启和截图；Keychain、WKWebView、主线程 DocumentPicker 与 security-scoped 有界复制已接线；隐藏 `UITextView` 代理已同步组合态、选区、提交文本和基础 VoiceOver 语义；受保护手动 workflow 支持发行证书/描述文件和签名 IPA 导出 | 新交互 CI/签名 workflow 实际结果、真机中文 IME/候选窗/选择句柄、旋转、后台恢复与 VoiceOver 人工验收 |
| 本地纵切 | 五分区、树、桌面标签、移动恢复、Note/Bookmark/File/Feed、全文搜索、回收站、导入、系统打开可用；导航查询只读取 SQLite 投影列，不再反序列化完整正文；桌面与移动搜索使用 180ms 防抖、后台连接和过期结果拒绝；桌面提供全局搜索、新建、保存、关闭与标签循环快捷键；Plate 标题/段落/引用/列表/任务/代码/分隔线可通过可逆 Markdown 投影编辑，未改 marks 复用原 JSON，未知块以指纹占位无损保护；移动端提供当前块格式工具栏、可筛选的 slash 块命令菜单和显式上移/下移块重排，并拒绝改写受保护块或已有代码围栏；移动正文支持多行滚动、按点击位置设置光标和原生组合输入桥；输入模型已测试 CJK、换行过滤、光标中部插入、Home/End/前后删除、完整 emoji grapheme 删除，以及不重复软键盘事件的外接键盘 Backspace/Delete/Enter 兜底；桌面和移动草稿通过 700ms 防抖异步保存，窗口退到后台时立即调度写入，Agent 活动查询不再阻塞渲染热路径，初始化失败显示恢复页 | 移动端以显式重排替代 Web Plate 的精确拖拽手势；原生候选窗、系统选择句柄和辅助功能仍须真机验证/补强 |
| 联网、更新与 Agent | 三分域加密同步、HTTP CAS、系统安全存储、本地 MCP、实时 Grant/脱敏审计、OpenAI-compatible BYOK 工具循环与持久线程可用；桌面 ACP v1 使用官方 Rust SDK、直接 argv 启动、默认拒绝权限请求，CLI 已与官方 TS SDK echo Agent 完成真实 stdio 握手；移动端明确只提供 BYOK；桌面设置页已接 minisign 清单验签、SemVer 防回滚、精确大小/SHA-256 下载和显式系统安装；受保护 release workflow 已接五平台聚合、清单签名/独立 Rust 验签，以及不回显凭据/模型正文的 staging model probe 与三分域 sync probe | 使用真实发行密钥和 staging 凭据实际运行 workflow；外部 ACP 逐请求交互审批与 ideall 作为入站 ACP Agent 的发布需求评审 |
| Engine | 统一平台 capability 清单覆盖 info/community/browser/audio/code/git/shell/database/extension；移动资讯/社区使用隔离 WebView，Linux 使用系统浏览器；桌面 audio/database 只读副本交给系统应用，移动仅元数据；Git/Shell 因缺少目录授权/PTY/进程审计而不注册 renderer | 专业 Git/Shell/audio/database 原生交互属于后续增强，不再是 P5 可用性门禁；引入时仍须单独安全评审 |

“CI 已配置”不等于真实设备已经通过。上述人工项未关闭前不得宣称完成商店级五平台验收；
但生产构建不得回退到 Tauri，旧兼容层也不得写入 GPUI 数据目录。

## 9. 验证命令

原生工作区逐步提供统一入口：

```bash
cd native
cargo fmt --all -- --check
cargo check --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo run --locked -p ideall-desktop
```

移动端另在 macOS/Android 构建环境运行目标检查、模拟器冒烟和真机验收；host `cargo check` 不能替代移动目标验证。

## 10. 完成定义

只有以下证据同时成立，才能认定迁移完成：

- 当前架构文档、README、开发命令与发布流程均以 GPUI 为权威；
- 五个平台的核心用户流程在真实产物中通过；
- V2 数据导入、跨版本同步和凭据隔离通过真实数据验收；
- 所有现有功能已迁移、明确移除并记录，或由平台 capability 明确解释；
- Next.js、React、Tauri、IndexedDB 和旧桥接代码已从生产构建移除；
- CI、签名、更新和回滚流程覆盖全部发布平台。
