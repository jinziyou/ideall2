# GPUI 平台支持状态

> 状态：现行
>
> 本文只记录默认分支当前能够持续验证的能力和发布前仍需人工完成的门禁。历史
> Next.js/Tauri V2 已冻结在 `legacy/tauri-v2`，不再作为生产实现或回退路径。

## 支持口径

平台状态分为三层，文档和发布说明不得混用：

1. **可编译**：目标代码能在锁定工具链和依赖下构建。
2. **CI 已验证**：真实产物通过仓库自动化检查或模拟器交互冒烟。
3. **可发布**：签名产物还通过真实设备、安装升级、输入法和无障碍人工矩阵。

CI 通过不等于商店级发布验收完成。

## 当前矩阵

| 平台 | 自动化证据 | 明确限制 |
| --- | --- | --- |
| Linux | Ubuntu 22.04 构建 GPUI release；校验 ELF、最高 `GLIBC_2.35`、tar.gz、DEB、RPM 与 SHA-256 | Browser 使用系统浏览器；仍需多发行版、Wayland、IME 与无障碍人工验收 |
| Windows | 构建 GPUI release；校验 zip、MSI 与 NSIS 安装器 | 签名 workflow 需要发行证书；仍需真实安装/卸载、WebView、IME 与辅助功能验收 |
| macOS | 构建 GPUI release；校验 `.app` zip 与 DMG | 公证 workflow 需要 Developer ID；仍需真实安装升级、WebView、IME 与 VoiceOver 验收 |
| Android | 构建双 ABI APK/AAB；x86_64 模拟器执行创建、原生文本输入、滚动、旋转和前后台恢复 | 发布需要 keystore；候选窗、选择句柄和 TalkBack 必须真机验收 |
| iOS | 构建 arm64 Simulator `.app`；XCTest 执行创建、原生文本输入、旋转、后台恢复和重启 | 发布需要证书/描述文件；候选窗、选择句柄和 VoiceOver 必须真机验收 |

共享 Rust workspace 的 fmt、check、clippy 和测试在 Linux 快速门中执行。桌面安装包与移动
模拟器任务按变更路径分流；`dev`、`main` 推送和工作流自身变化仍运行完整矩阵。

## 生产代码边界

```text
native/
├── apps/
│   ├── ideall-desktop/       # GPUI 桌面组合根
│   ├── ideall-mobile/        # gpui-mobile + iOS/Android 壳
│   └── ideall-cli/           # doctor、迁移与 staging probe
├── crates/
│   ├── ideall-protocol/
│   ├── ideall-domain/
│   ├── ideall-application/
│   ├── ideall-presentation/
│   └── ideall-{storage,sync,agent,acp,secrets,updater}/
├── scripts/                  # 打包、产物校验和模拟器冒烟
└── vendor/gpui-mobile/       # 可追溯的固定上游快照
```

依赖方向固定为：

```text
protocol → domain → application → adapters → UI → platform app
```

共享 crate 的公开 API 不暴露 GPUI、SQLite、HTTP 或平台系统类型。桌面和移动可以使用不同的
已验证 GPUI revision，但不能通过共享 crate 传递 UI 类型。

## 依赖与 vendor 约束

- 桌面 `gpui`、`gpui-component` 和平台 WebView 依赖使用精确版本。
- `gpui-mobile` 的上游 revision、许可证和本地差异记录在
  [`native/vendor/gpui-mobile/README.md`](../native/vendor/gpui-mobile/README.md)。
- `verify-gpui-mobile-vendor.sh` 校验本地文件摘要；带 `--upstream` 时还会联网比对固定
  revision。
- 标准构建和 CI 使用 `--locked`；GPUI 或 vendor 升级必须单独提交并运行完整平台矩阵。
- `native/vendor/gpui-mobile` 是上游镜像边界，不做与 ideall 产品无关的格式化或顺手重构。

## 平台能力降级

| 能力 | Desktop | Mobile |
| --- | --- | --- |
| Browser | macOS/Windows 使用受限子 WebView；Linux 使用系统浏览器 | 使用平台 WebView |
| 文本 | 原生轻编辑；按文件后缀优化显示 | 原生轻编辑 |
| 音频 | 导出只读临时副本交给系统应用 | 仅展示安全元数据 |
| 其他二进制文件 | 通用预览与安全元数据 | 通用预览与安全元数据 |
| 外部 ACP | 用户显式配置可信程序后可用 | 不提供子进程能力 |

不可用能力不进入导航、快捷键或 Agent 工具。新增本机执行能力必须先具备目标目录授权、
参数边界、进程隔离、取消/超时、输出预算和脱敏审计。

## 数据迁移边界

GPUI App 不解析 WebKit/WebView2 的 IndexedDB 文件。旧数据只通过 V2 工作区归档迁移：

1. 在 `legacy/tauri-v2` 的独立检出中导出归档；
2. 原生 importer 校验 envelope、密码学参数、CRC32、引用和体积预算；
3. 在临时 SQLite 数据库中完成单事务导入；
4. 成功后原子替换目标数据库并保留导入前备份。

旧版与 GPUI 版不得同时写同一数据目录。API Key、同步码和登录凭据不进入归档。

## 验证入口

日常完整门禁：

```bash
pnpm verify
pnpm native:check
```

平台构建和发布入口：

- `.github/workflows/rust.yml`：workspace、桌面三平台、Android 和 iOS 自动门禁；
- `.github/workflows/native-release.yml`：签名安装包、staging probe 与更新清单；
- [`native/RELEASE.md`](../native/RELEASE.md)：发行密钥、真实设备和人工验收。

## 发布前人工清单

- 中文/日文组合输入、emoji、候选窗和系统选区句柄；
- 键盘导航、VoiceOver、TalkBack 和高缩放；
- 桌面安装、升级、卸载、签名与公证；
- Android/iOS 真机旋转、后台恢复、分享/文件选择和低内存恢复；
- 使用真实发行密钥运行签名 workflow；
- 使用 staging 凭据运行模型与三分域同步 probe；
- 使用真实 V2 数据执行导出、导入、备份和跨版本同步验收。

只有自动化与适用的人工项同时通过，才能把对应平台声明为可发布。
