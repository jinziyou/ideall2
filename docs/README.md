# 文档索引

本目录按“当前事实优先、历史设计明确归档”组织。修改代码时先看对应的现行文档；设计记录用于理解取舍，不覆盖当前代码与现行契约。

## 现行架构与开发规范

| 文档 | 用途 |
| --- | --- |
| [architecture.md](architecture.md) | 架构总览、领域模型、模块边界、数据流与关键不变量 |
| [product-strategy-roadmap.md](product-strategy-roadmap.md) | 产品定位、核心信息循环、阶段路线图、指标与版本验收 |
| [file-system-engine-architecture.md](file-system-engine-architecture.md) | Storage → FileSystem → IdeallFile → Engine → Display 五层契约 |
| [app-data-navigation.md](app-data-navigation.md) | App 数据落点、活动栏与二级侧栏到真实 FileRef/Storage 的映射 |
| [extensions.md](extensions.md) | 扩展概念模型、信任边界与当前落地范围 |
| [runtime-extension-packages.md](runtime-extension-packages.md) | 联网签名目录、桌面扩展包格式、Minisign 验证、connector 生命周期与残余风险 |
| [development.md](development.md) | 目录结构、依赖边界、开发命令与贡献约定 |
| [design/ui-style.md](design/ui-style.md) | UI 视觉和组件规范 |
| [../.github/SECURITY.md](../.github/SECURITY.md) | 安全策略与漏洞报告方式 |

## 操作手册

| 文档 | 用途 |
| --- | --- |
| [../native/README.md](../native/README.md) | GPUI 原生开发、构建与平台入口 |
| [../native/RELEASE.md](../native/RELEASE.md) | GPUI 五平台签名、发布与人工验收 |
| [app.md](app.md) | 历史 Tauri 兼容层；只用于旧数据导出 |
| [app-data-safety-acceptance.md](app-data-safety-acceptance.md) | 历史 Tauri/keychain 与归档验收记录 |
| [release-0.2.md](release-0.2.md) | 0.2 破坏性数据基线、升级清理和当前兼容边界 |
| [mobile-share-acceptance.md](mobile-share-acceptance.md) | Android/iOS 系统分享宿主接入、安全契约与平台验收 |
| [scripts.md](scripts.md) | 验证、冒烟、API codegen 与维护脚本 |
| [ideall-embed-bridge.md](ideall-embed-bridge.md) | iframe + MCP 嵌入桥协议及宿主安全边界 |

## 决策与后续设计

这些文档记录特定决策或尚未全部落地的方案；其中的“现状”以文首状态说明和现行架构文档为准。

| 文档 | 状态 |
| --- | --- |
| [sync-lww-tradeoff.md](sync-lww-tradeoff.md) | 已落地：关注同步的 LWW、墓碑传播与 GC 取舍 |
| [extension-registry-design.md](extension-registry-design.md) | 历史红队设计基线；运行时 FileSystem + Engine 注册已落地，部分能力/端口扩展仍是方向 |
| [extension-registry-operations.md](extension-registry-operations.md) | 签名 Registry 的目录真相源、原子发布、服务代理与事故处置 |
| [local-data-provider.md](local-data-provider.md) | P1 已落地；运行期连接查看/断开已实现，持久 consent 与外部 transport 停放 |
| [freedesktop-alignment.md](freedesktop-alignment.md) | 已全部落地：S1（XDG 存储分类）、S1b（登记盲区收口）、S2（MIME subclassing）、S3（Engine 关联文件化）、S4（Engine 描述符投影）、S5（recently-used + 缩略图缓存） |
| [design/gpui-migration.md](design/gpui-migration.md) | 已切换：桌面 GPUI、移动 gpui-mobile 的全平台 Rust 原生化决策、证据与人工门禁 |

## 历史归档

| 文档 | 用途 |
| --- | --- |
| [app-history.md](app-history.md) | App-only 与 Tauri 化阶段记录；当前操作以 `native/README.md` 为准 |
| [design/archive/README.md](design/archive/README.md) | 已完成或废弃的设计稿、原型与路径快照索引 |

维护文档时应同时更新本索引；路径、命令和部署口径必须以仓库当前实现为准，历史数字不要复制到现行说明中。
