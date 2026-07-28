# Extension Registry 发布运维

## 1. 架构与密钥边界

生产 Registry 分成两个互不共享私钥的平面：

1. `ideall` 仓库的 `.github/workflows/extension-registry.yml` 读取
   `registry/extensions.json`，以既有 Tauri updater Minisign Secret 对每页精确 payload 签名，
   并通过 staging Release 原子切换 `extension-registry` 固定通道。
2. `wonita` apiserver 的 `GET /v2/extensions/registry` 从该固定公开 Release 读取资产，限制
   `limit`、cursor、响应大小和 JSON 外形后短时缓存。服务端没有签名私钥，也不替代桌面验签。
3. 桌面端使用内置官方根重新验证每页，并拒绝序列回退、同序列不同信封、过期分页和条目约束
   违规。发现目录和扩展包下载/安装仍是两条独立验签链。

长期密钥只保留在 `ideall` 的 `IDEALL_UPDATE_PRIVATE_KEY` 与
`IDEALL_UPDATE_PRIVATE_KEY_PASSWORD` GitHub Actions Secret。不得复制到 wonita 仓库、服务器环境、
Registry Release 或本地目录。

## 2. 目录真相源

`registry/extensions.json` 是唯一受版本控制的目录源。空 `entries` 是合法的生产 Feed：它可先验证
签名发布和服务链路，同时不会宣传尚未准备好的扩展。

新增条目必须包含以下精确字段：

```json
{
  "id": "example.reader",
  "label": "Example reader",
  "summary": "Read bounded resources from Example.",
  "version": 1,
  "publisher": "ideall.official",
  "publisherFingerprint": "sha256:...",
  "permissions": ["resources:read"],
  "digest": "sha256:...",
  "packageFile": "example.reader-1.ideall-extension",
  "publishedAt": 1784260000000
}
```

包文件放在 `registry/packages/`。发布工具会拒绝符号链接、越界路径、超过 96 MiB 的文件、未知字段、
无序权限或 Registry 与包内 manifest 不一致，并从精确包字节计算 `packageSha256`。官方 publisher
指纹必须与 `native/updater-public-key.txt` 的 updater 根一致；connector Base64、64 MiB 上限及 SHA-256
也会在发布前复核。当前生产目录只接收 `ideall.official`，并在发布侧完整验证包内 manifest 的
Minisign 主签名与 trusted-comment 全局签名；第三方 publisher 目录要等独立根分发协议后再开放。

当前没有可发布的官方扩展包，因此目录保持为空；不要为验证 UI 放入测试包或占位下载地址。

## 3. 生成与发布

工作流在 `main` 的目录/发布脚本变更、手动触发及每日计划任务运行。仓库默认分支是 `dev`，因此
计划任务会显式 checkout 稳定 `main` 并把该提交 SHA 绑定到 Release，而不会发布默认分支上的未稳定
代码。默认有效期 14 天，sequence 与生成时间使用发布时 epoch 毫秒。发布前会：

- 最多生成 8 页、每页最多 64 条，cursor 只使用 `page_0001` 到 `page_0007`；
- 实际调用 `ideall-cli update-sign`，并校验签名 key id 与内置 updater 公钥一致；
- 生成包含 size/SHA-256 的 prepared manifest，再逐资产复核；
- 读取当前生产根页，拒绝 sequence 不递增；
- 先上传并验证 staging Release，最后切换固定 tag，失败时保留旧 Feed。

本地没有生产 Secret 时只运行纯校验测试：

```bash
pnpm test:scripts -- extension-registry-artifacts
```

只有受控发布环境才运行：

```bash
REGISTRY_GENERATED_AT=<epoch-ms> \
REGISTRY_SEQUENCE=<strictly-increasing-epoch-ms> \
REGISTRY_OUTPUT_DIR=registry-ready \
pnpm registry:prepare
pnpm registry:verify
```

## 4. 上线验收

发布顺序是：先让 `extension-registry` Release 成功，再部署包含代理路由的 wonita apiserver。验收：

当前状态（2026-07-21）：首次空目录 Release 已发布并通过下载摘要与 Minisign 独立验收；Wonita
已将代理路由迁移到 V2，桌面端与服务端统一使用下列固定端点；生产端点已返回 HTTP 200 和有效的
签名空目录信封。

```bash
curl -fsS 'https://api.wonita.link/v2/extensions/registry?limit=64'
```

然后在桌面设置页点击“刷新目录”。HTTP 200 只代表代理可用；必须看到客户端来源为 network、状态为
current 且没有签名错误，才代表端到端成功。空目录应显示正常的当前序列，而不是“目录不可用”。

## 5. 故障与回退

- **工作流失败**：不要删除现有 Release；修复后以更高 sequence 重新发布。
- **发布机时钟回退**：手动触发 workflow 并填写大于当前生产值的 `sequence` 输入；`generatedAt` 仍取
  当前真实时间，不要用未来时间绕过检查。
- **上游或 GitHub 故障**：wonita 最多回退 5 分钟服务端缓存，桌面继续保留其已验签缓存并显示刷新故障。
- **误发目录**：修正真相源并发布更高 sequence；不要把固定 tag 指回旧资产。
- **密钥疑似泄露**：立即停用 Registry 工作流和新增扩展发布。当前 Registry、官方扩展包与 App updater
  共用运营根，必须按统一根轮换/恢复事故处理；独立离线恢复根是下一阶段工作。
