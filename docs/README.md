# 文档索引

默认分支只描述 Rust + GPUI 版本。旧 Next.js/Tauri V2 的实现和历史手册位于
`legacy/tauri-v2`，不在此处维护。

## 架构与开发

| 文档 | 用途 |
| --- | --- |
| [architecture.md](architecture.md) | GPUI 生产架构、模块边界、运行时与数据流 |
| [development.md](development.md) | 目录结构、开发命令、依赖约束与贡献入口 |
| [file-system-engine-architecture.md](file-system-engine-architecture.md) | Storage → FileSystem → FileRef → Engine → Display 契约 |
| [design/ui-style.md](design/ui-style.md) | GPUI 视觉、布局和设计 Token |
| [design/desktop-components.md](design/desktop-components.md) | Desktop Workbench、Explorer、Editor、Browser 边界 |
| [design/desktop-editor.md](design/desktop-editor.md) | Notion 式块编辑、快捷键与数据安全不变量 |
| [platform-support.md](platform-support.md) | 五平台构建证据、能力降级与人工门禁 |
| [../.github/SECURITY.md](../.github/SECURITY.md) | 安全策略、信任边界与漏洞报告 |

## 操作手册

| 文档 | 用途 |
| --- | --- |
| [../native/README.md](../native/README.md) | 原生开发、构建与平台入口 |
| [../native/RELEASE.md](../native/RELEASE.md) | 五平台签名、发布与人工验收 |
| [scripts.md](scripts.md) | 仓库验证、维护与发布脚本 |

## 产品与设计记录

| 文档 | 状态 |
| --- | --- |
| [product-strategy-roadmap.md](product-strategy-roadmap.md) | 产品定位与阶段路线图 |
| [sync-lww-tradeoff.md](sync-lww-tradeoff.md) | 已落地：LWW、墓碑传播与 GC |

维护文档时应同步更新本索引；命令、路径和发布口径必须以默认分支当前实现为准。
