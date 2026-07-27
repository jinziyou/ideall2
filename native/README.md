# ideall native

这里是 ideall 的 Rust + GPUI 生产实现。仓库根目录的 Next.js/Tauri 代码只保留为旧数据
V2 归档导出兼容层，不再进入 App 构建或发布；两套实现使用独立数据目录，不能直接共享数据库。

## 当前入口

```bash
cd native
cargo test --locked --workspace
cargo run --locked -p ideall-desktop
cargo run --locked -p ideall-cli -- doctor --database /path/to/ideall.db
cargo run --locked -p ideall-cli -- archive inspect /path/to/workspace.json
cargo run --locked -p ideall-cli -- archive import /path/to/workspace.json --database /path/to/ideall.db
cargo run --locked -p ideall-cli -- acp-probe --program node --arg /absolute/path/to/acp-agent.mjs --cwd /absolute/workspace --prompt "hello"
# key 只从 IDEALL_MODEL_API_KEY 读取
cargo run --locked -p ideall-cli -- model-probe --base-url https://model.example/v1 --model staging-model --prompt "hello"
# code/token 只从 IDEALL_SYNC_CODE / IDEALL_SYNC_BEARER_TOKEN 读取
cargo run --locked -p ideall-cli -- sync-probe --database /tmp/ideall-smoke.db --server https://sync.example --seed-title ideall-smoke
bash scripts/verify-gpui-mobile-vendor.sh
# 联网拉取固定 revision，并确认 vendor 仅包含登记过的差异
bash scripts/verify-gpui-mobile-vendor.sh --upstream
bash scripts/package-desktop.sh
bash apps/ideall-mobile/build-mobile.sh android release
bash apps/ideall-mobile/build-mobile.sh android-bundle release
bash apps/ideall-mobile/build-mobile.sh ios-simulator release
# 需要 IDEALL_DEVELOPMENT_TEAM 与本机发行身份/描述文件
bash apps/ideall-mobile/build-mobile.sh ios-archive release
# 需要手动签名描述文件名称，并从 archive 导出 App Store Connect IPA
bash apps/ideall-mobile/build-mobile.sh ios-ipa release
```

Debian/Ubuntu 的桌面构建需要 `libxkbcommon-x11-dev`、`libfontconfig1-dev`、
`libfreetype6-dev`、`libxcb1-dev` 与 Vulkan loader 开发包。Linux 不编译只供 Windows/macOS 使用的
嵌入式 WebView feature，因此安装包不依赖 GTK/WebKit。Linux 同时暴露
Wayland/X11 但没有声明 Wayland session 时默认回退 X11；可用
`IDEALL_FORCE_X11=1` 显式选择 X11。
官方 Linux 产物固定在 Ubuntu 22.04 构建，并由 `verify-linux-binary.sh` 强制最高
`GLIBC_2.35`；在更高版本发行版上本地打出的 Preview 不自动获得这一兼容性声明。

移动文本状态按 UTF-8 byte boundary 与 Unicode grapheme 维护，已覆盖 CJK、emoji、光标中部
插入、Home/End、前后删除和单/多行换行规则。移动根视图持有 GPUI 键盘焦点，并对
Backspace、Delete、Enter 与左右/Home/End 提供带平台回调去重的硬件键盘兜底。iOS 使用
隐藏 `UITextView`、Android 使用隐藏 `EditText` 作为原生输入与无障碍代理，把组合态、选区
和提交文本同步回 GPUI；候选窗、系统选择句柄及 VoiceOver/TalkBack 行为仍必须以
Android/iOS 真机结果为准。

加密归档通过 `IDEALL_ARCHIVE_PASSPHRASE` 环境变量提供口令；CLI 不接受明文
口令参数，避免口令进入 shell history 和进程列表。

桌面 Preview 先执行 `cargo build --locked --release -p ideall-desktop`，再运行
`bash scripts/package-desktop.sh`，产物和 `SHA256SUMS` 位于
`target/native-packages/`。CI 同时上传 Linux tar.gz/DEB/RPM、Windows zip/MSI/NSIS setup、
macOS `.app` zip/DMG、Android APK/AAB 与 iOS simulator `.app`；这些未签名 Preview 不能替代
商店签名和真机门禁。Windows 安装器分别由仓库锁定的 WiX 4.0.6 与 NSIS 3.12.0 生成；
WiX 4 保持 v4 schema 且不要求在无人值守 CI 中接受 WiX 7 的 OSMF EULA。
Android 模拟器门禁会真实创建笔记、聚焦原生输入代理、输入标题/正文、滚动、旋转并验证
前后台恢复期间 PID 不变；iOS 使用 XCTest 执行同类输入、旋转、后台恢复、重启和截图。
它们用于捕获运行时回归，仍不能替代中文 IME、候选框、选区句柄和无障碍真机矩阵。
手动 `native-release` workflow 使用受保护 environment 中的发行密钥生成签名 AAB、iOS
archive、签名/公证 DMG 与 Authenticode MSI/NSIS 安装器；
所需 secret、人工 IME/无障碍矩阵与真实服务步骤见 [`RELEASE.md`](RELEASE.md)。

桌面设置页通过固定 `native-preview` GitHub Release 检查更新。更新清单自身必须通过旧版
独立的官方 minisign 根验证，目标版本必须高于当前 SemVer；用户显式下载后，
客户端还会强制核对清单声明的字节数与 SHA-256，校验成功才交给系统安装器。可用
`IDEALL_NATIVE_UPDATE_URL` 与 `IDEALL_NATIVE_UPDATE_CHANNEL=preview|stable` 指向受控测试
频道；更换 URL 不能绕过签名验证。受保护 release 构建会把所选频道编译为默认值，因此
Stable 二进制不会回看 Preview；运行期环境变量仅用于受控诊断。原生端不做静默自替换。

- `ideall-desktop`：GPUI + gpui-component 桌面壳。
- `ideall-mobile`：gpui-mobile iOS/Android 静态/动态库入口，并包含 Android Gradle 与 iOS XcodeGen 工程壳。gpui-mobile 使用 `vendor/gpui-mobile` 中可追溯的固定源码快照，来源、许可证、本地差异校验和与升级步骤见其 `README.md`。CI 还会联网比对固定上游 revision，拒绝未登记的源码漂移。编辑器变换收口在 `src/editor.rs`，系统文本输入桥收口在 `src/native_text.rs`。Android 需要 SDK、NDK、`cargo-ndk` 和 Gradle 8.9+；iOS 需要 Xcode 16 与 XcodeGen。真机签名通过 `IDEALL_DEVELOPMENT_TEAM` 传入。
- `ideall-cli`：数据库 doctor、V2 归档预检/原子导入、外部 ACP v1，以及不回显凭据/正文的真实模型与同步 staging 探测。
- `ideall-protocol`：与现有 TypeScript wire 对齐的纯契约。
- `ideall-domain`：不依赖 UI/Storage 的 Engine 与 Workspace 逻辑。
- `ideall-application`：创建、编辑、回收和恢复本地节点的跨 UI 用例。
- `ideall-agent`：实时 Grant、内容安全的本地 MCP 工具、OpenAI-compatible BYOK 传输与脱敏写操作审计。
- `ideall-acp`：基于官方 Rust SDK `1.3.0` 的桌面 ACP v1 stdio 客户端；program/argv 直接启动、不经 shell，权限请求默认拒绝。
- `ideall-storage`：SQLite 权威存储、V2 工作区归档校验及可恢复的原子导入。
- `ideall-sync`：与旧客户端兼容的 HKDF/AES-GCM 派生、分片预算、LWW 合并与 tombstone GC。
- `ideall-sync-http`：带 Bearer 鉴权、响应预算和稳定错误映射的 V2 manifest/parts HTTP 适配器。
- `ideall-secrets`：桌面系统凭据库、iOS Keychain 与 Android Keystore 的类型化适配；敏感值不进入 SQLite。
- `ideall-updater`：严格解析并验证 minisign 更新清单，选择平台安装器并执行有预算的 SHA-256 下载。

原生笔记编辑器使用可逆 Markdown 块投影：标题、段落、引用、列表、任务、代码块与
分隔线可直接编辑；移动端提供作用于光标所在块的格式工具栏，以及输入 `/` 后按名称筛选
的块命令菜单。工具栏还可整体上移/下移当前段落、多行引用、代码块或受保护块，作为移动端
精确块拖拽的可操作替代。已有代码围栏和受保护块不会被工具栏或命令改写。未修改块保留
原始 Plate JSON 与 marks。尚不认识的块显示
`⟦ideall:受保护块:…⟧` 指纹占位，保存时必须保留该行，application 层会把原始
JSON 精确放回并维护与旧客户端兼容的稳定块 ID、`blockMeta` 与删除 tombstone。
正文编辑使用 700ms 防抖草稿保存，并在窗口退到后台时立即调度持久化；数据库写入和
Agent 活动读取都在 GPUI 后台执行，不阻塞渲染热路径。桌面与移动入口在安全存储、支持目录
或 SQLite 初始化失败时显示可恢复的错误页，不再由 `expect` 直接终止进程。

桌面和移动端都可配置 OpenAI-compatible API 基址、模型与系统安全存储中的 API Key，
对话与脱敏工具回执逐步写入 SQLite Thread 节点。桌面端还可运行用户明确配置的外部
ACP v1 Agent；参数只按 argv 引号规则拆分，不执行变量展开、命令替换或重定向。
ACP 权限请求当前全部拒绝；外部进程本身仍拥有当前系统账户和所选工作目录允许的权限，
因此只应配置可信程序。移动端不具备子进程能力，明确仅提供 BYOK 模型后端。

原生壳在“应用”中展示同一份按目标平台计算的 Engine capability 清单。页面、书签、
关注、对话、文件树、Preview 和 Code 使用原生 renderer；资讯/社区仅进入隔离 WebView，
Linux 因 GPUI 子窗口限制改由系统浏览器打开。桌面音频和 SQLite 文件只导出只读临时
副本并交给系统应用，移动端仅展示安全元数据。Git 与 Shell 当前明确为不可用：在具备
目录授权、跨平台 PTY、进程隔离和审计前，不注册 renderer，也不把任意路径或无约束
shell 暴露给 UI/Agent。这些降级是可测试的产品能力，不以占位入口冒充专业实现。

原生 UI 依赖的许可证选择记录见 [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md)。

完整迁移门禁见 [`docs/design/gpui-migration.md`](../docs/design/gpui-migration.md)。
