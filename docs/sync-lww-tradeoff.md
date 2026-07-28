# 跨端同步合并策略：LWW 取舍 + 删除传播（决策记录）

> **状态：已落地。** 对应审计项 **Low-6**（跨端 LWW 取舍 + 删除不传播）。
> 实现：`@protocol/sync`（纯合并/GC 逻辑 + 单测）、`@protocol/storage-sync`（本地快照 CAS）、各领域 store（软删除/恢复快照/GC 落地）、`src/plugins/sync/lib/sync-domain-runner.ts`（关注/笔记/书签共用编排）。

## 背景

跨端同步（端到端加密、账号绑定密文；见 [README](../README.md) / [development.md](development.md)）把关注、笔记与书签在多设备间对齐：
本地优先存于 IndexedDB，同步时**只上传密文**（`storageId` + AES 密钥由同步码派生）。
服务端是不透明密文存储，**不参与合并**——合并逻辑全在客户端。多端可并发改同一份关注集，
故需要一个确定性、可结合、幂等的合并函数。

## 决策一：合并用 LWW（last-write-wins）并集

**选 union-merge + 按 `updatedAt` 的 LWW**（`@protocol/sync#unionMerge`）：

- 按 `id` 取本地 ∪ 远端；同 `id` 取 `updatedAt` 较新者胜，并列本地优先（稳定）。
- 幂等 + 可结合：每轮把新拉到的远端并入累积 `merged` 即可，天然支持「409 重试→重新 GET→再合并」。

**为什么不用更强的 CRDT（如逐字段 OR-Set / 版本向量）？**

| | LWW 并集（选定） | 逐字段 CRDT / 版本向量 |
|---|---|---|
| 复杂度 | 低（一个纯函数 + 单测） | 高（每字段元数据、因果序、合并规则） |
| 收益对关注场景 | 足够：关注是小粒度记录，字段冲突罕见且无损要求低 | 过剩：为「同字段并发改」保真，但关注几乎不存在此模式 |
| 数据量 | 关注数量级为「十～百」，整集合并成本可忽略 | 元数据开销与同步块体积上升 |
| 可审计性 | 合并结果一眼可推（看 `updatedAt`） | 需理解因果元数据 |

**取舍代价（已知且接受）**：同一关注在两端「同一毫秒」内被改时，按「本地优先」任选其一——
对关注这种低冲突、字段无损要求低的数据，可接受。若未来有高保真合并需求（如富文本笔记同步），
另选 CRDT，不复用本策略。

## 决策二：删除用墓碑（tombstone）传播，而非物理删

**问题（旧实现）**：删除是**物理删** + 合并是**并集**——在 A 端取消关注某来源，B 端仍持有它，
下次同步 `unionMerge` 把它从 B 带回 A，**删除被「复活」**。即「删除不传播」。

**选 tombstone 软删除**（让删除也走同一条 LWW，从而跨端收敛）：

- `removeSubscription` 写 `deletedAt` 墓碑（并 bump `updatedAt`），而非 `idbDelete`。墓碑只是一条
  `deletedAt` 已设的 `Subscription`，**原样参与 `unionMerge`**：
  - 删除较新 → 墓碑胜 → 删除跨端收敛（不再被对端活跃副本复活）。
  - 删除后又重新关注（`addSubscription` 命中墓碑则**复活**：清 `deletedAt` + 新 `updatedAt`）较新 → 活跃项胜。
- **读路径过滤**：`listSubscriptions` / `isSubscribed` 只返回活跃项（`isLive`），UI/插件/嵌入桥无感。
- **同步读含墓碑**：新增 `listAllSubscriptions`（墓碑须进合并/上传才能传播），UI 读路径仍走 `listSubscriptions`。

### 墓碑 GC

墓碑若永不清除会无限累积、撑大加密同步块。但**清早了会复活**（某端尚未见过该删除时，
它的活跃副本会盖过已消失的墓碑）。故按**保留期**折中：

- `TOMBSTONE_TTL_MS = 90 天`：远超任何合理的多端离线窗口；超过即视为所有端已收敛，可安全物理清除。
- `pruneExpiredTombstones`（纯函数，`now` 注入）在同步落地/上传前剔除过期墓碑，使**本地与远端同步块都不再累积**。
- 落地侧：`bulkPutSubscriptions` 整批写入合并 + GC 后的集合，并物理删除墓碑——但删除判定**据落地时刻 fresh 重读的真实库状态**
  （`expiredTombstoneIdsToDelete`：库中当前已过期、且不在本批集合里的墓碑），**而非调用方的同步快照**。
  这样绝不会误删同步往返窗口内并发新增的活跃关注（本轮未上传，下轮自然带上），也不会删掉正被写回的复活项。
  > **注（避坑）**：早期实现曾用「删除库内不在本批集合的所有 id」做 reconcile，会把同步 await 窗口内并发 `addSubscription` 的新关注
  > 误判为陈旧而物理删除（静默数据丢失）。修正为「只删确属过期墓碑的 id」。`expiredTombstoneIdsToDelete` 有纯逻辑单测守此。

## 决策三：同步落地使用本地快照 CAS

网络 409 只能保护远端 blob，不能保护 GET/解密期间发生的本地编辑。同步状态因此区分两份本地状态：

- `localAll` 是同步开始时的不可变统计基线；
- `localSnapshot` 是下一次本地提交的期望快照，每次成功落地后推进为 Storage 返回的实际规范化快照。

`bulkPutSubscriptions` / `bulkPutNotes` / `bulkPutBookmarkNodes` 在同一个 IndexedDB readwrite 事务内 fresh 读取当前领域，只有当前逻辑快照仍等于 `expectedLocal` 时才写入合并结果与 tombstone GC。收藏夹与书签共享一个快照和提交事务，避免父子关系部分落地。当前状态已经等于目标时允许幂等成功；真实不一致则抛 `StorageSyncConflictError`，整批不写、也不发送远端 PUT。远端 PUT 返回 409 时，下一轮携带上次实际提交快照继续合并。

关注、笔记、书签与收藏夹的 live↔tombstone 转换、GC 与本机 `trash_snapshots` 在同一跨 store 事务提交；新同步 id 使用 `add`，不会覆盖同主键的其它 Node kind。关注入站要求规范 `type/key/id` 与安全 tool URL；书签入站拒绝坏 Node 结构和 `javascript:` 等伪协议。活跃书签若指向缺失或已删除收藏夹，会以确定性版本时间移动到根级；Storage CAS 再次拒绝嵌套收藏夹或孤儿书签，避免关系不完整的快照落库。三个领域由同一同步码派生互不相同的 storageId 和 AES-GCM 密钥。

## 决策四：分区快照与原子 manifest

JSON/Base64/AES-GCM 会同时占用字符串和二进制缓冲，客户端因此在 JSON 编码、分片解密/重组、本地 CAS 和上传前实施记录数与明文字节硬上限：关注 4 MiB、笔记 32 MiB、书签 16 MiB。超限以稳定的 `block-limit` 分类失败，不会先覆盖本地或发布不完整快照。

V2 协议把整个域的 JSON UTF-8 字节流切成最多 1024 片，每片明文最多 196,592 字节，加上 16-byte GCM tag 后 canonical Base64 不超过 262,144 字符。分片 0 继续使用历史 HKDF info，分片 1..1023 使用稳定后缀派生独立密钥；每片另用随机 96-bit IV。切片可落在多字节字符内，读取时必须先重组字节再做一次 UTF-8/JSON 解码。

写入时先上传随机 128-bit generation 的不可变 parts，所有 index 连续且配额校验通过后，再以 `expected=manifest.version` CAS 提交 manifest。服务端只允许读取 manifest 当前指向的 generation，并在提交事务内清理上一代，因此上传中断和 CAS 冲突都不会暴露部分快照。客户端在提交响应丢失时会回读 manifest 确认；只有能确认未提交或收到明确失败时才最善努力删除 staged generation，回读本身失败时不会冒险删除可能已经激活的 generation。

读取侧逐片复算 `content_sha256`，再校验 manifest 的有序 `parts_sha256` 聚合与密文总长度；读取期间若旧 generation 的 part 返回 404，则在统一的四次尝试上限内重新拉取 manifest 并从新 generation 开始，绝不混用两代分片。没有 manifest 时依次探测账号绑定的 V2 单 blob 与历史 V1 blob，解密合并后立即发布为分区快照。迁移桥只读旧存储；新客户端不再向匿名 V1 继续写入。

## 已知局限（接受 / 留待）

1. **版本偏斜下的墓碑**：旧版客户端（不识别 `deletedAt`）会把墓碑当活跃项渲染并回传，削弱删除收敛。
   缓解：原生 App 使用签名更新清单（见
   [native/RELEASE.md](../native/RELEASE.md)），版本偏斜窗口有限；wire 校验继续容忍墓碑字段。
2. **90 天内频繁删/关注累积墓碑**：体积影响对「十～百」量级关注可忽略；不优化。
3. **同毫秒并发改**：按决策一「本地优先」任选其一，接受。
4. **同步往返窗口内的并发本地写**：若本轮需要落地远端变化，本地快照 CAS 会拒绝覆盖并发 add/edit/move/remove，用户需重试同步；若合并结果无需写本地，窗口内的新本地变化不会丢失，但可能要到下一轮才上传。这是“安全失败 / 最终补传”的取舍，不自动吞冲突继续合并。
5. **旧客户端版本偏斜**：仍写 V1 单 blob 的旧客户端看不到新 V2 manifest 的更新；新客户端会把它最后读到的旧 blob 单向迁移到 V2，不做长期双写。自动更新用于缩短该窗口。

## 验证

`src/protocol/sync.test.ts` 覆盖删除收敛与 GC；各 domain sync 测试覆盖真实加解密、远端推进、提交快照与入站校验；store 测试覆盖跨 kind 碰撞、本地 CAS、事务回滚和 tombstone/恢复/GC 快照生命周期。

## 关联

- 纯合并/GC 契约：`@protocol/sync`。同步编排：`src/plugins/sync/`。
- 同步加解密：`src/lib/sync-crypto.ts`。本地 CAS 与远端 409 重试见 `sync-domain-runner.ts`。
- 关注类型（含 `deletedAt`）：`@protocol/subscription`。
