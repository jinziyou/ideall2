# 安全策略

## 适用范围

本策略覆盖默认分支中的 GPUI 桌面/移动 App、SQLite 本地数据层、跨端同步、Agent/ACP、
平台安全存储、签名扩展目录与原生更新器。官方信息服务后端不在本仓库范围内。

停止维护的 Next.js/Tauri V2 位于 `legacy/tauri-v2`，不属于当前生产版本。

## 报告漏洞

请勿在公开 Issue、PR 或 Discussion 中披露安全漏洞。请使用仓库
**Security → Report a vulnerability** 的 GitHub Security Advisory 私密通道，并尽量提供
受影响版本/平台、复现步骤、影响评估和修复建议。

## 主要信任边界

- SQLite 保存本地内容与公开配置；模型 Key、同步码、登录 token 和敏感 consent 只进入
  平台安全存储。安全存储失败时 fail closed。
- 同步服务是不可信存储，只接收 HKDF/AES-GCM 保护的密文。同步码必须是应用生成的高熵值。
- OpenAI-compatible 模型端点由用户选择，会看到发送给它的系统提示、会话和显式授权上下文。
- 外部 ACP Agent 与 connector 是当前系统账户下的进程，不等同于 OS 沙箱；只应运行可信程序。
- Agent 访问本地内容必须经过实时 Grant → MCP/FileSystem，写操作先记录脱敏审计。
- Browser Engine 中的外部网页不获得应用的 ambient authority；无法安全内嵌的平台使用系统浏览器。
- 更新清单、扩展目录和 connector 包在使用前验证 Minisign、SHA-256、大小与版本/序列预算。

## 数据出站

“本地优先”不代表所有联网功能零出站：

| 能力 | 可能发送 | 不会自动发送 |
| --- | --- | --- |
| 加密同步 | 密文、分片元数据、认证 token | 明文内容、模型 Key |
| 模型/Agent | 当前会话、系统提示、用户授权上下文、工具回执 | 整库内容、未授权文件 |
| 资讯/社区 | 用户发起的查询、登录或发布内容 | 本地草稿之外的其它内容 |
| 更新/扩展目录 | 平台、版本、常规网络元数据 | 笔记正文、凭据 |

用户把服务地址改为自建端点时，接收方随配置改变。

## 已知取舍

- 外部 ACP Agent、MCP connector 和系统浏览器都继承当前系统账户的能力；进程隔离不是内核沙箱。
- 端到端同步当前没有前向保密；关闭同步不等同于删除服务端已有密文。
- 更新与扩展信任根需要受保护的发行流程；私钥不得进入仓库、日志或普通 CI artifact。
- 真机 Keychain/Keystore、中文 IME、无障碍和安装升级仍需按发布手册执行人工矩阵。

实现与验证细节见 [原生开发说明](../native/README.md)和
[原生发布手册](../native/RELEASE.md)。
