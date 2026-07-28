#!/usr/bin/env node
/** 一次性配置本仓库 fetch refspec, 永久避免 app-edge 等移动标签冲突。 */
import { fileURLToPath } from "node:url"
import path from "node:path"
import { ensureMovingTagFetchRefspecs } from "./lib/git-fetch-config.mjs"

const HELP = `用法:
  pnpm git:setup
  node scripts/git-setup.mjs

说明:
  为 origin 添加移动发布标签的强制更新 fetch refspec。该配置只修改当前仓库。
`
const args = process.argv.slice(2)
if (args.includes("--help") || args.includes("-h")) {
  console.log(HELP.trimEnd())
  process.exit(0)
}
if (args.length > 0) {
  console.error(`[git:setup] 未知参数: ${args.join(" ")}`)
  process.exit(1)
}

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
const added = ensureMovingTagFetchRefspecs(root)
if (added > 0) {
  console.log(`[git:setup] 已添加 ${added} 条 remote.origin.fetch refspec (移动标签强制更新)。`)
} else {
  console.log("[git:setup] fetch refspec 已就绪, 无需变更。")
}
console.log("此后直接 git pull / Cursor 同步即可, 不应再出现 app-edge 标签冲突弹窗。")
