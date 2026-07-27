# Native release gate

原生 Preview 与签名 Release 是两条不同管线。`.github/workflows/rust.yml` 对每次变更构建
无签名五平台 Preview，并检查产物内部确实包含目标架构的 ideall 原生库或可执行文件。
`.github/workflows/native-release.yml` 只能手动触发，并绑定受保护的 `native-release`
environment；它为 Android、iOS、macOS、Windows 使用发行密钥，Linux 发布 tar.gz、
DEB、RPM 与 SHA-256 清单。macOS 同时发布 `.app` zip 和 DMG，Windows 同时发布便携 zip、
MSI 和 NSIS setup。缺少任一密钥时任务必须失败，不能回退无签名包。

普通 Rust CI 还会把双 ABI debug APK 安装到 x86_64 Android 模拟器，并把 simulator `.app`
安装到临时 iOS Simulator 后启动；这两项只验证可加载、前台存活和无立即崩溃，不能替代下表
的真机输入、无障碍和生命周期验收。Linux 官方产物只在 Ubuntu 22.04 生成，并拒绝高于
`GLIBC_2.35` 或意外重新链接 GTK/WebKit 的二进制。

## Release environment

在 GitHub `native-release` environment 中配置以下 secrets，并开启人工审批：

- Android：`IDEALL_ANDROID_KEYSTORE_BASE64`、`IDEALL_ANDROID_KEYSTORE_PASSWORD`、
  `IDEALL_ANDROID_KEY_ALIAS`、`IDEALL_ANDROID_KEY_PASSWORD`。
- iOS：`IDEALL_IOS_DEVELOPMENT_TEAM`、`IDEALL_IOS_CERTIFICATE_BASE64`、
  `IDEALL_IOS_CERTIFICATE_PASSWORD`、`IDEALL_IOS_PROVISIONING_PROFILE_BASE64`。
- macOS：`IDEALL_MACOS_CERTIFICATE_BASE64`、`IDEALL_MACOS_CERTIFICATE_PASSWORD`、
  `IDEALL_MACOS_SIGNING_IDENTITY`、`IDEALL_APPLE_ID`、`IDEALL_APPLE_TEAM_ID`、
  `IDEALL_APPLE_APP_PASSWORD`。
- Windows：`IDEALL_WINDOWS_CERTIFICATE_BASE64`、
  `IDEALL_WINDOWS_CERTIFICATE_PASSWORD`。
- 更新清单：`IDEALL_UPDATE_PRIVATE_KEY`、`IDEALL_UPDATE_PRIVATE_KEY_PASSWORD`；必须与
  `native/updater-public-key.txt` 里的 updater 公钥匹配。聚合任务由 `ideall-cli`
  在内存中完成 Minisign 签名，再用 Rust 客户端做完整验签，
  不是只比较 key id。
- Staging 服务变量：`IDEALL_MODEL_BASE_URL`、`IDEALL_MODEL_NAME`、
  `IDEALL_SYNC_SERVER`；secrets：`IDEALL_MODEL_API_KEY`、`IDEALL_SYNC_CODE`、
  `IDEALL_SYNC_BEARER_TOKEN`。同步环境必须是可清理的 staging 空间，探测会写入随后 tombstone
  的一次性笔记。

证书和 keystore 只写入 runner 临时目录；Apple 临时 keychain 在 `always()` 步骤删除。
CI 日志不得打印任何 secret、口令、证书正文或签名私钥路径以外的密钥信息。

workflow 先强制输入版本与仓库九处版本源一致，再等待五个平台构建、签名和 staging smoke
全部成功，最后生成 `native-preview.json` 或 `native-stable.json`、分离签名、总 SHA-256 与
发行 inventory。`publish_release` 默认关闭；打开后只推进固定 `native-preview` / `native-stable`
频道，并最后替换签名清单，切换中发生不一致时客户端会安全拒绝。Stable 发布还必须通过
单独受保护的 `native-stable` environment，未完成本文人工矩阵时不得批准。iOS 任务必须从
签名 archive 成功导出 IPA，只有 `.xcarchive` 不算可发布产物。

## 每个候选版本的人工矩阵

每个平台都必须记录：commit SHA、workflow run、产物 SHA-256、设备/系统版本、执行人、
开始/结束时间，以及下面每项的 pass/fail 与失败截图。模拟器结果不能替代真机结果。

| 范围 | Linux | Windows | macOS | Android 真机 | iPhone/iPad 真机 |
| --- | --- | --- | --- | --- | --- |
| 首启、重启、后台/恢复 | 必测 | 必测 | 必测 | 必测 | 必测 |
| 新建/编辑/搜索/回收/恢复 | 必测 | 必测 | 必测 | 必测 | 必测 |
| 中文、英文、emoji、组合输入与退格 | 必测 | 必测 | 必测 | 至少 Gboard + 一种中文 IME | 至少系统拼音 + emoji |
| 文件导入与超限/取消 | 必测 | 必测 | 必测 | SAF | DocumentPicker/security scope |
| 书签系统打开与隔离 WebView | 系统浏览器降级 | 必测 | 必测 | Android WebView | WKWebView |
| 系统凭据保存、清除、重启后状态 | Secret Service | Credential Manager | Keychain | Keystore | Keychain |
| 屏幕阅读器与键盘导航 | Orca | Narrator | VoiceOver | TalkBack | VoiceOver |
| 100%/200% 缩放、旋转、安全区 | 必测 | 必测 | 必测 | 必测 | 必测 |
| 断网、超时、服务 4xx/5xx | 必测 | 必测 | 必测 | 必测 | 必测 |

当前 gpui-mobile revision 尚未向宿主暴露完整的原生 accessibility tree API。TalkBack /
VoiceOver 只要无法逐个发现、朗读并激活主要控件，候选版本就必须失败；不能以可点击或有
可见文字代替无障碍通过。修复需要上游能力或维护过的受审 fork，发布门不得豁免。

## 真实服务验收

使用专门的 staging 账户和可撤销短期凭据，禁止把生产 token 写入 fixture、issue 或日志。

1. 同步：旧稳定版创建关注/笔记/书签，原生端拉取并修改；再由旧版拉取。覆盖并发 CAS、
   tombstone、离线重试、大 Blob 拒绝和错误同步码。数据库 quick-check 必须通过。
2. BYOK：对一个 OpenAI-compatible staging 模型完成普通回复、`fs.list`、
   `fs.create-note`、工具错误、网络中断后恢复；确认 API key 仅在系统安全存储。
3. ACP：仅桌面，用受信 echo Agent 完成 v1 握手；权限请求仍应拒绝，转录不得出现原始
   tool input/output、本机绝对路径或 secret。
4. 归档：稳定版导出明文和加密 V2，原生端 dry-run 后原子导入；篡改/错误口令不得改变
   现有数据库。导入结果再次导出并由稳定版读取。

## 切换条件

只有以下条件同时满足，才可批准 `native-stable` environment 并把原生版设为 Stable：签名 workflow 全绿；上述人工矩阵全绿；
真实服务验收全绿；两个连续 Preview 版本未出现数据损坏或 schema 回滚。切换后至少保留
两个 Stable 周期的旧版可回滚发布物与 V2 导入路径。回滚窗口结束前，不删除旧
Next/Tauri 数据导出兼容代码或下载渠道；生产签名与发布配置只以 `native/` 为权威。
