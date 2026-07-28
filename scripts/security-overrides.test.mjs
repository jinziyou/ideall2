import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"

const PACKAGE_JSON = new URL("../package.json", import.meta.url)
const PNPM_LOCK = new URL("../pnpm-lock.yaml", import.meta.url)
const BRACE_EXPANSION_PATCH = new URL("../patches/brace-expansion@5.0.8.patch", import.meta.url)

test("brace-expansion resolves only to the bounded release with CJS compatibility", () => {
  const packageJson = JSON.parse(readFileSync(PACKAGE_JSON, "utf8"))
  const lockfile = readFileSync(PNPM_LOCK, "utf8")
  const patch = readFileSync(BRACE_EXPANSION_PATCH, "utf8")

  assert.equal(packageJson.pnpm?.overrides?.["brace-expansion"], "5.0.8")
  assert.equal(
    packageJson.pnpm?.patchedDependencies?.["brace-expansion@5.0.8"],
    "patches/brace-expansion@5.0.8.patch",
  )
  assert.match(lockfile, /^  brace-expansion: 5\.0\.8$/m)
  assert.match(lockfile, /^  brace-expansion@5\.0\.8:$/m)
  assert.match(patch, /module\.exports = Object\.assign\(expand, exports\)/)

  const lockedVersions = [
    ...lockfile.matchAll(/^  brace-expansion@([0-9]+\.[0-9]+\.[0-9]+):$/gm),
  ].map((match) => match[1])
  assert.deepEqual(
    [...new Set(lockedVersions)],
    ["5.0.8"],
    `unexpected brace-expansion release remains locked: ${lockedVersions.join(", ")}`,
  )
})

test("runtime dependency security floors stay locked", () => {
  const packageJson = JSON.parse(readFileSync(PACKAGE_JSON, "utf8"))
  const lockfile = readFileSync(PNPM_LOCK, "utf8")
  const overrides = packageJson.pnpm?.overrides ?? {}

  assert.equal(packageJson.dependencies?.next, "16.2.11")
  assert.equal(packageJson.devDependencies?.["eslint-config-next"], "16.2.11")
  assert.equal(overrides["@hono/node-server@<2.0.12"], "2.0.12")
  assert.equal(overrides["fast-uri@>=3.0.0 <3.1.4"], "3.1.4")
  assert.equal(overrides["hono@<4.12.32"], "4.12.32")
  assert.equal(overrides["sharp@<0.35.0"], "0.35.0")

  for (const release of [
    "@hono/node-server@2.0.12",
    "fast-uri@3.1.4",
    "hono@4.12.32",
    "next@16.2.11",
    "sharp@0.35.0",
  ]) {
    assert.match(
      lockfile,
      new RegExp(`^  '?${escapeRegExp(release)}'?:$`, "m"),
      `${release} is not locked`,
    )
  }
})

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}
