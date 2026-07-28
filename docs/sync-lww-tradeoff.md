# 原生同步的 LWW 与冲突取舍

ideall 的同步协议由 `ideall-sync` 实现，SQLite 适配位于 `ideall-application` /
`ideall-storage`。服务端只保存加密分区和 manifest，不接触明文。

## 目标

- 多端离线编辑后能够最终收敛；
- 删除不会因旧设备重新上线而轻易复活；
- 同步往返期间的本地写入不会被静默覆盖；
- 不完整上传、坏密文和越界数据不能进入本地数据库；
- 与 V2 同步 wire 保持兼容。

## LWW 规则

普通记录按稳定 ID 合并：

1. 只存在一侧的记录直接保留；
2. `updatedAt` 较大的记录获胜；
3. 时间相同由本地记录获胜，避免一次同步立即覆盖用户当前设备的修改；
4. 结果按 ID 排序，保证序列化和摘要稳定。

笔记进一步按 `blockMeta` 合并。每个块使用版本、作者与排序键选择获胜侧；块墓碑不重新
插入正文，幸存块按排序键稳定排列。若任一侧缺少块级 metadata，则退回整条笔记 LWW，
保证旧数据仍可同步。

LWW 不等于实时协作。相同对象的并发修改可能丢失一侧内容；当前优先保证离线、确定性和
可解释冲突，不引入通用 CRDT 的复杂度。

## 删除与墓碑

删除使用带 `deletedAt` 的记录参与合并。墓碑保留 90 天：

- 保留期内，较新的删除能够压过旧设备上的活跃副本；
- 超过保留期后，`prune_expired_tombstones` 才允许物理清理；
- 笔记块墓碑使用相同保留期；
- 本机 `trash_snapshots` 是恢复能力，不上传为同步正文。

90 天是离线设备收敛窗口与长期存储增长之间的显式取舍。超过窗口才重新上线的旧设备仍可能
带来历史数据，需要由产品层提示用户核对。

## 本地快照 CAS

网络 manifest 的 409 只能保护远端版本，不能保护下载期间发生的本地编辑。因此同步 runner
同时维护：

- 同步开始时读取的本地快照；
- 每次合并后的期望快照；
- SQLite 实际提交返回的规范化快照。

`SyncLocalStore::compare_and_swap` 只有在数据库当前逻辑快照仍等于 expected 时才提交。
若用户在同步期间编辑，CAS 返回 conflict，runner 重新读取、合并并重试，最多四次；耗尽后
明确失败，不静默覆盖。

## 分区、加密与 manifest

每个同步 scope 从 128-bit 十六进制同步码经 HKDF 派生独立 `storageId` 和 AES-256-GCM
密钥。同步码不是用户口令，必须使用应用生成的高熵值。

V2 快照规则：

- 明文按最多 196,592 bytes 分片；
- 最多 1,024 片，每片使用随机 96-bit IV；
- 每片绑定 SHA-256，manifest 再绑定有序 parts 摘要和总长度；
- 上传先写随机 generation 的不可变 parts，最后 CAS 提交 manifest；
- 读取必须验证 generation、连续索引、摘要、长度、记录数和明文预算；
- 任何验证失败都不会写入 SQLite。

当前预算：

| Scope | 最大记录数 | 最大明文 |
| --- | ---: | ---: |
| subscriptions | 50,000 | 4 MiB |
| notes | 100,000 | 32 MiB |
| bookmarks | 100,000 | 16 MiB |

manifest 提交响应丢失时，客户端回读确认是否已经生效；无法确认时不删除可能已激活的
generation。远端 409、generation 切换和临时 404 都在统一的四次尝试预算内处理。

## 校验边界

远端记录进入合并前必须通过：

- schema 和 Node kind 校验；
- 时间戳与未来偏移预算；
- URL scheme、父子引用和收藏夹关系校验；
- 记录数、密文、明文和分片预算；
- AES-GCM 认证与 SHA-256 摘要。

本地提交把 Node、全文索引、回收快照和相关 tombstone 放在 SQLite 事务内；失败时整批回滚。

## 已接受的局限

1. LWW 无法保留同一字段的全部并发意图；笔记只对带 metadata 的块提供更细粒度合并。
2. 超过 90 天未上线的旧客户端可能错过墓碑窗口。
3. 同步码泄露会暴露读写能力；当前没有前向保密和密钥轮换。
4. 后端可扣留或回滚旧密文；协议尚未提供设备签名的全局单调日志。
5. 本地持续高频写入可能使四次 CAS 重试耗尽，需要用户稍后重试。

## 验证入口

- 合并、墓碑、加密和分区：`native/crates/ideall-sync/src/lib.rs`
- 笔记块合并：`native/crates/ideall-sync/src/notes.rs`
- CAS 与远端重试：`native/crates/ideall-sync/src/runner.rs`
- SQLite 事务适配：`native/crates/ideall-application/src/workspace.rs`
- 集成测试：`cargo test --locked -p ideall-sync -p ideall-application -p ideall-storage`
