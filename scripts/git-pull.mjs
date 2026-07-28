#!/usr/bin/env node
/**
 * 安全 pull: 确保已配置移动标签 fetch refspec, 再 fast-forward pull。
 */
import { execFileSync } from "node:child_process"
import { fileURLToPath } from "node:url"
import path from "node:path"
import { ensureMovingTagFetchRefspecs } from "./lib/git-fetch-config.mjs"

const HELP = `用法:
  pnpm git:pull
  node scripts/git-pull.mjs

说明:
  先确保移动发布标签的 fetch refspec 已配置，再执行 git pull --ff-only。
`
const args = process.argv.slice(2)
if (args.includes("--help") || args.includes("-h")) {
  console.log(HELP.trimEnd())
  process.exit(0)
}
if (args.length > 0) {
  console.error(`[git:pull] 未知参数: ${args.join(" ")}`)
  process.exit(1)
}

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")

function run(cmd, args) {
  execFileSync(cmd, args, { cwd: root, stdio: "inherit" })
}

ensureMovingTagFetchRefspecs(root)
run("git", ["pull", "--ff-only"])
