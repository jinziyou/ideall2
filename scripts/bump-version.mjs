// 发版版本号一键同步。所有目标文件会先完整读取、定位并校验，确认无结构漂移后才开始写入。
import { writeFileSync } from "node:fs"
import {
  assertProjectVersions,
  loadProjectVersionState,
  prepareProjectVersionUpdate,
  validateProjectVersion,
} from "./lib/project-version.mjs"

const HELP = `用法:
  pnpm bump <x.y.z>
  node scripts/bump-version.mjs <x.y.z>

说明:
  同步原生 Cargo、桌面打包与移动构建入口的项目版本。
  写入前会完整校验全部文件及其当前版本，任一文件异常都不会产生修改。
`

const args = process.argv.slice(2)
if (args.includes("--help") || args.includes("-h")) {
  console.log(HELP.trimEnd())
  process.exit(0)
}

if (args.length !== 1) {
  console.error(HELP.trimEnd())
  process.exit(1)
}

try {
  const nextVersion = validateProjectVersion(args[0], "目标版本号")

  // 先在内存中完成全部读取、字段定位、当前一致性检查和替换，之后才允许落盘。
  const currentEntries = loadProjectVersionState()
  const currentVersion = assertProjectVersions(currentEntries)
  const updates = prepareProjectVersionUpdate(currentEntries, nextVersion)

  for (const update of updates) {
    writeFileSync(update.absolutePath, update.nextContents, "utf8")
    console.log(`✓ ${update.path}: ${currentVersion} → ${nextVersion}`)
  }
  console.log(
    `\n版本已统一为 ${nextVersion} —— 检查 diff 后提交, 再打 tag: git tag app-v${nextVersion}`,
  )
} catch (error) {
  console.error(`✗ ${error instanceof Error ? error.message : error}`)
  process.exit(1)
}
