import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"
import path from "node:path"
import test from "node:test"
import { fileURLToPath } from "node:url"

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_DIR, "..")
const ALLOWED_MAINTENANCE_DEPENDENCIES = ["actionlint", "prettier"]
const RUNTIME_DEPENDENCY_SECTIONS = ["dependencies", "optionalDependencies", "peerDependencies"]

async function readRepositoryFile(relativePath) {
  return readFile(path.join(REPOSITORY_ROOT, relativePath), "utf8")
}

test("根 npm 清单只允许仓库维护工具", async () => {
  const manifest = JSON.parse(await readRepositoryFile("package.json"))

  for (const section of RUNTIME_DEPENDENCY_SECTIONS) {
    assert.deepEqual(
      Object.keys(manifest[section] ?? {}),
      [],
      `${section} 不得承载产品运行时；旧前端只能存在于 legacy/tauri-v2`,
    )
  }

  assert.deepEqual(
    Object.keys(manifest.devDependencies ?? {}).sort(),
    ALLOWED_MAINTENANCE_DEPENDENCIES,
    "根 devDependencies 只能包含当前仓库维护工具",
  )
})

test("pnpm 锁文件不保留旧前端依赖树", async () => {
  const lockfile = await readRepositoryFile("pnpm-lock.yaml")
  const packagesStart = lockfile.indexOf("\npackages:\n")
  const snapshotsStart = lockfile.indexOf("\nsnapshots:\n")
  assert.ok(packagesStart >= 0, "pnpm-lock.yaml 缺少 packages 区段")
  assert.ok(snapshotsStart > packagesStart, "pnpm-lock.yaml 缺少 snapshots 区段")

  const packagesSection = lockfile.slice(packagesStart, snapshotsStart)
  const lockedPackages = [...packagesSection.matchAll(/^  ([^\s][^:]+):$/gm)].map(
    (match) => match[1],
  )
  assert.ok(lockedPackages.some((entry) => entry.startsWith("actionlint@")))
  assert.ok(lockedPackages.some((entry) => entry.startsWith("prettier@")))
  assert.ok(
    lockedPackages.every((entry) =>
      ALLOWED_MAINTENANCE_DEPENDENCIES.some((name) => entry.startsWith(`${name}@`)),
    ),
    `锁文件包含未授权 npm 包: ${lockedPackages.join(", ")}`,
  )
})
