# 旧版 Tauri App 兼容手册

> **历史兼容文档（2026-07-27）**：本文记录旧 Next.js/Tauri 壳，只用于导出 V2
> 工作区归档和排查旧安装。生产 App、根开发命令与发布流水线已切换到 Rust + GPUI；
> 当前操作请以 [`native/README.md`](../native/README.md) 和
> [`native/RELEASE.md`](../native/RELEASE.md) 为准。

下表和后续章节只描述停产前的 Tauri 口径，不代表当前构建、平台覆盖或发布状态。

| 项 | 当前口径 |
| --- | --- |
| **构建** | `next build`（`output: export` → `out/`），再由 Tauri 打包 |
| **渲染** | 静态导出；无 Node 运行时 / 无 SSR 生产部署 |
| **数据** | 客户端经 `ServerPort` 直连后端数据服务（CORS + JWT） |
| **开发** | `pnpm dev` 本地 SSR 开发服供 `pnpm app:dev` 的 Tauri 壳加载 |
| **发布** | 桌面 CI → GitHub Releases；Android 仅有 debug APK workflow artifact；iOS 未入 CI，移动应用商店发布尚未接入 |

历史迁移路线见 [app-history.md](app-history.md)。

## 目录

```text
ideall/
├── src/                      # Next.js 应用（静态导出）
├── src-tauri/                # Tauri 2.0 app 外壳（Rust）
│   ├── Cargo.toml
│   ├── tauri.conf.json       # devUrl=localhost:5020 / frontendDist=../out
│   ├── capabilities/         # 权限能力集
│   ├── icons/                # 应用图标
│   └── src/{main,lib}.rs
├── out/                      # 静态导出产物（gitignore）
└── docs/app.md               # 本文件
```

## 开发环境 / 前置依赖

日常 Web 开发只需 **Node.js ≥ 22** + **pnpm 9**（见根目录 `package.json`）。Tauri 桌面/打包另需 **Rust ≥ 1.77.2** 与下表平台依赖；装完后在仓库根执行 `pnpm exec tauri info`，**Environment** 段应全绿。

| 平台 | 系统依赖（概要） | 验证 |
| --- | --- | --- |
| **Linux / WSL** | `libwebkit2gtk-4.1-dev`、`librsvg2-dev`、`libgtk-3-dev`、`libayatana-appindicator3-dev`、`build-essential`、`libssl-dev`、`libxdo-dev`、`pkgconf` 等 | `pnpm exec tauri info` → webkit2gtk / rsvg2 ✔ |
| **Windows** | MSVC Build Tools + WebView2 Evergreen | 同上 |
| **macOS** | Xcode 或 Command Line Tools | 同上 |
| **Android** | JDK、Android SDK/NDK；`pnpm tauri android init` | `rustup target add` 各 Android triple |
| **iOS** | macOS + Xcode；`pnpm tauri ios init` | 仅 macOS 构建机 |

**Linux / WSL 一键安装（Debian 系，含 Kali）：**

```bash
sudo apt-get update && sudo apt-get install -y \
  libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev \
  librsvg2-dev libgtk-3-dev pkgconf \
  desktop-file-utils gnome-keyring
```

WSL 图形依赖 **WSLg**；dbus/GTK 包更新后窗口仍异常时，Windows 侧 `wsl --shutdown` 后重开。详见 [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)。

> **系统凭据库（Secret Service）**：桌面 Linux 依赖 `gnome-keyring`（或其它 Secret Service 实现）完成 0.2 data-epoch 清凭据与 API key 存取。WSL 若未安装/未启动 keyring，App 会回退到会话级 linux keyutils，避免 BootGate 卡在 `BOOT_REGISTRATION_FAILED`；跨重启持久化仍建议安装并启动 `gnome-keyring`。

> **窗口空白（WSL2 无 GPU）**：WebKitGTK 的 DMABUF / 加速合成渲染在 WSL2 常初始化失败（`libEGL` / `MESA ZINK` / `dri2` 报错）导致窗口空白。`scripts/app-dev.mjs` 会在 Linux 启动前注入 WebKitGTK 软件渲染与 WSLg 相关环境变量；已手动 export 同名变量则以你的为准。

## 构建 / 运行命令

```bash
# 仅在维护旧兼容层时使用
pnpm web:dev                 # 旧 Next.js 开发服
pnpm legacy:web:export       # V2 归档导出兼容层
pnpm legacy:tauri:dev        # 排查旧 Tauri 安装时使用
pnpm legacy:tauri build      # 不属于生产发布
pnpm verify:smoke:static     # 旧静态导出冒烟
```

发布前还需在真实桌面进程与系统凭据库上执行 [Tauri App 数据安全验收](app-data-safety-acceptance.md)；设置页的“运行系统凭据库自检”会完成一次性随机值的写入、读回和清理。

脚本入口、参数和新增脚本约定见 [scripts.md](scripts.md)。

### 指定备用端口启动（5020 被占用时）

旧 `pnpm legacy:tauri:dev` 流程默认使用 5020。WSL 下编辑器自动端口转发可能占用该端口。

换个端口启动即可绕开。用 Tauri 的 `--config` 内联覆盖 `devUrl`；`app:dev` 会据此选择 Next 端口，并把参数透传给 `tauri dev`：

```bash
pnpm legacy:tauri:dev --config '{"build":{"devUrl":"http://localhost:5026"}}'
```

- 旧启动脚本读取 `devUrl` 后，在 5026 复用已有 Next 开发服或自行启动一个。
- Tauri 壳使用同一个 `devUrl`，因此会加载 5026 上的开发服。
- 端口可以按需调整；5026 仅为示例，可先用 `ss -ltn` 确认端口未被占用。

## 环境变量

完整示例见 [`.env.example`](../.env.example)。常用变量：

| 变量 | 作用 | 默认 |
| --- | --- | --- |
| `NEXT_PUBLIC_SERVER_ADDR` | App 客户端直连的后端 API 基址 | `https://api.wonita.link` |
| `SERVER_ADDR` | 仅 `pnpm dev` 的 SSR 渲染期读取；不设时回退到上一项 | 同 `NEXT_PUBLIC_SERVER_ADDR` |
| `NEXT_PUBLIC_EMBED_BASE` | info/community iframe 嵌入的 portal 基址 | `https://www.wonita.link` |
| `NEXT_PUBLIC_XSTATE_INSPECT` | 开发态 XState Inspector；设为 `0` 关闭 | 开启 |
| `IDEALL_BROWSER_CDP` | Linux/WSL 内嵌浏览器后端；设为 `1` 启用 CDP 模式 | WebKit 内嵌 |

`NEXT_PUBLIC_EMBED_BASE` 会在构建期写入前端，但它本身不会放宽 Tauri 的 CSP。改用自建 portal 时还必须把对应 origin 加入 `src-tauri/tauri.conf.json` 的 `frame-src`，并让 portal 的 `frame-ancestors` 放行 ideall，然后重新打包 App。仅修改环境变量会使 iframe 被 WebView 拒绝。

## 目标平台矩阵

下表描述工程目标；“支持作为 Tauri target 构建”不等于已经进入 CI、签名或商店发布。

| 平台 | Tauri 目标 | 目标产物 | 当前自动化 |
| --- | --- | --- | --- |
| Linux | desktop | `.deb` / `.rpm` / `.AppImage` | CI 自动构建并进入 GitHub Release（x64） |
| Windows | desktop | `.msi` / `.exe`（NSIS） | CI 自动构建并进入 GitHub Release（x64） |
| macOS | desktop | `.dmg` / `.app` | CI 自动构建并进入 GitHub Release（arm64 / x64） |
| Android | mobile | `.apk` / `.aab` | tag / 手动任务仅生成 debug APK workflow artifact；release 签名与商店发布未接入 |
| iOS | mobile | `.ipa` | 需 macOS、Apple 证书与描述文件；未纳入 CI 或商店发布流程 |

## CI / 发布

CI 分三层：

- 历史 `.github/workflows/ci.yml` quality/build 分层曾运行 `app:export`；当前等价兼容入口为
  `legacy:web:export`，生产 CI 以原生 workflow 为准。
- `.github/workflows/rust.yml`：`src-tauri/**` 改动时跑 rustfmt、cargo check、clippy 零警告和 cargo test。
- `.github/workflows/smoke.yml`：应用面改动时跑静态导出生产形态的浏览器冒烟；本地等价入口是 `pnpm verify:smoke:static`。

App 发布由 `.github/workflows/app-build.yml` 负责：

- push `main` → 滚动预发布 `app-edge`。
- push `app-v*` tag → 正式 Release 草稿（人工 review 后发布）。
- workflow_dispatch → 仅允许从 `main` 手动构建 `app-edge`。
- 桌面 matrix 的四个构建目标（macOS arm64 / x64、Linux x64、Windows x64）先上传 1 天保留的 workflow artifacts，不直接写 GitHub Release。最终 publish job 要求这些目标的安装包、updater 签名、`latest.json` 与 `SHA256SUMS` 全部校验通过，才创建正式 draft 或切换 `app-edge`。
- Android 仅 tag / 手动时产出 debug APK；iOS 需 Apple 证书与 macOS 构建机，未纳入 CI。
- 所有 edge/tag 构建都会重新执行 JavaScript 质量门禁；tag、手动构建及涉及 `src-tauri/**` 的 push 还会执行 Rust 门禁。tag 另会确认提交已包含在 `main`。
- 发布采用 artifact-first：平台构建或 staging 上传失败不会改动旧 `app-edge`；最终切换使用备份 tag，并在 promotion 失败时回滚旧 Release。正式 tag 仍保持 draft，edge 仍保持 prerelease。

静态导出地址由 GitHub 仓库变量注入；未设置时回退到生产默认：

- `NEXT_PUBLIC_SERVER_ADDR`
- `NEXT_PUBLIC_EMBED_BASE`

仓库变量同样不能动态改写已编译的 Tauri CSP；CI 构建自建 embed 源前须先在源码中维护匹配的 `frame-src`。

## 签名 / 自动更新

**自动更新（桌面，已启用）**：

- `tauri-plugin-updater` 已配置，endpoint 指向 GitHub 最新正式 Release 的 `latest.json`。
- `bundle.createUpdaterArtifacts: true` + CI 注入 `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 后，tauri-action 为安装包生成 `.sig`；publish job 根据四个桌面构建目标的已验证资产确定性聚合 `latest.json`。
- 自动更新只认已发布的「非草稿、非预发布」正式 tag 版。

发版流程：

```bash
pnpm bump <x.y.z>      # 同步 package.json / tauri.conf.json / Cargo.toml / Cargo.lock
git tag app-v<x.y.z>
git push origin main app-v<x.y.z>
```

CI 会检查 `app-v*` tag 与 `package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml` / `Cargo.lock` 版本一致，并确认 tag 提交已进入 `main`；不一致直接失败。

**OS 代码签名 / 公证（待证书，当前出未签名包）**：

| 用途 | 状态 | 所需 secret / 配置 |
| --- | --- | --- |
| 更新签名（minisign） | ✅ 已启用 | `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` |
| macOS 签名 + 公证 | ⏳ 待证书 | Apple 开发者会员 → `APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`、`APPLE_ID`、`APPLE_PASSWORD`、`APPLE_TEAM_ID` |
| Windows Authenticode | ⏳ 待证书 | OV/EV 证书，或 Azure Trusted Signing |
| Android release | ⏳ | keystore（`*.jks`）+ 别名/口令 |
| iOS | ⏳ | Apple 证书 + 描述文件 |

> `app-build` 的 preflight 会实际签名临时文件，验证 `TAURI_SIGNING_PRIVATE_KEY`、密码及 `tauri.conf.json` 公钥的 key id 匹配，并验证 embed origin 已进入 Tauri CSP；不是只检查 secret 是否非空。该密钥只签 updater 产物，不等于 macOS/Windows 的 OS 代码签名；未启用的 `APPLE_*` / Windows 证书变量仍应整组不注入。

## 风险与注意

- **静态导出限制**：不可用 Server Actions / Route Handlers / 请求时 `headers()` / 任意动态路径段 / `next/image` 优化（已设 `images.unoptimized`）。依赖请求时服务端数据的路由需客户端取数。
- **CORS / 鉴权**：App 静态导出无服务端代理，客户端直连 `NEXT_PUBLIC_SERVER_ADDR`。后端侧需放行 app 来源（`tauri://localhost` / 开发期 `http://localhost:5020`）。App 内 agent 经 `tauri-plugin-http` 绕过 webview CORS。
- **深链/路由**：Tauri 加载静态资源用相对路径；如遇路由 404，可加 `trailingSlash: true` 或自定义协议处理。
- **图标**：已生成全平台图标集到 `src-tauri/icons/`；替换为正式品牌图后重跑 `pnpm tauri icon <1024.png>`。
