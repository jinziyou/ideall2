import assert from "node:assert/strict"
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import path from "node:path"
import { afterEach, test } from "node:test"
import {
  assertProjectVersions,
  loadProjectVersionState,
  prepareProjectVersionUpdate,
} from "./lib/project-version.mjs"

const roots = []

afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })))
})

async function projectFixture(versions) {
  const root = await mkdtemp(path.join(tmpdir(), "ideall-version-"))
  roots.push(root)
  await mkdir(path.join(root, "native", "scripts"), { recursive: true })
  await mkdir(path.join(root, "native", "apps", "ideall-mobile", "platforms", "android", "app"), {
    recursive: true,
  })
  await Promise.all([
    writeFile(
      path.join(root, "package.json"),
      `{"name":"ideall","version":"${versions.package}"}\n`,
    ),
    writeFile(
      path.join(root, "native", "Cargo.toml"),
      `[workspace]\nmembers = []\n\n[workspace.package]\nversion = "${versions.native}"\n`,
    ),
    writeFile(
      path.join(root, "native", "Cargo.lock"),
      `[[package]]\nname = "ideall-domain"\nversion = "${versions.package}"\n\n[[package]]\nname = "ideall-mobile"\nversion = "${versions.package}"\n`,
    ),
    writeFile(
      path.join(root, "native", "scripts", "package-desktop.sh"),
      `version="\${IDEALL_VERSION:-${versions.package}}"\n`,
    ),
    writeFile(
      path.join(root, "native", "apps", "ideall-mobile", "build-mobile.sh"),
      `version="\${IDEALL_VERSION:-${versions.package}}"\n`,
    ),
    writeFile(
      path.join(
        root,
        "native",
        "apps",
        "ideall-mobile",
        "platforms",
        "android",
        "app",
        "build.gradle.kts",
      ),
      `val ideallVersionName = System.getenv("IDEALL_VERSION") ?: "${versions.package}"\n`,
    ),
  ])
  return root
}

test("project versions are validated together before an in-memory update", async () => {
  const root = await projectFixture({
    package: "1.2.3",
    native: "1.2.3",
  })
  const entries = loadProjectVersionState(root)
  assert.equal(assertProjectVersions(entries), "1.2.3")
  const updates = prepareProjectVersionUpdate(entries, "2.0.0-rc.1")
  assert.equal(updates.length, 6)
  assert.ok(updates.every((entry) => entry.nextContents.includes("2.0.0-rc.1")))
})

test("project version drift fails closed with per-file diagnostics", async () => {
  const root = await projectFixture({
    package: "1.2.3",
    native: "1.2.4",
  })
  assert.throws(
    () => assertProjectVersions(loadProjectVersionState(root)),
    /package\.json=1\.2\.3.*native\/Cargo\.toml=1\.2\.4/,
  )
})
