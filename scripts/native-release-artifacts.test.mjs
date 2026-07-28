import assert from "node:assert/strict"
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import os from "node:os"
import path from "node:path"
import test from "node:test"
import {
  finalizeNativeRelease,
  nativeReleaseEnvironment,
  prepareNativeRelease,
} from "./native-release-artifacts.mjs"

function minisignFixtures() {
  const keyId = Buffer.from("0102030405060708", "hex")
  const publicPacket = Buffer.alloc(42)
  publicPacket.write("Ed")
  keyId.copy(publicPacket, 2)
  const publicText = `untrusted comment: minisign public key\n${publicPacket.toString("base64")}\n`
  const signaturePacket = Buffer.alloc(74)
  signaturePacket.write("ED")
  keyId.copy(signaturePacket, 2)
  const globalPacket = Buffer.alloc(74)
  const signatureText = `untrusted comment: signature\n${signaturePacket.toString("base64")}\ntrusted comment: timestamp:0\n${globalPacket.toString("base64")}\n`
  return {
    publicKey: Buffer.from(publicText).toString("base64"),
    signature: Buffer.from(signatureText).toString("base64"),
  }
}

function names(version) {
  return [
    `ideall-native-${version}-linux-x86_64.tar.gz`,
    `ideall-native-${version}-linux-x86_64.deb`,
    `ideall-native-${version}-linux-x86_64.rpm`,
    `ideall-native-${version}-macos-arm64.zip`,
    `ideall-native-${version}-macos-arm64.dmg`,
    `ideall-native-${version}-windows-x86_64.zip`,
    `ideall-native-${version}-windows-x86_64.msi`,
    `ideall-native-${version}-windows-x86_64-setup.exe`,
    `ideall-native-${version}-android-arm64.aab`,
    `ideall-native-${version}-ios-arm64.ipa`,
  ]
}

function fixture(t) {
  const root = mkdtempSync(path.join(os.tmpdir(), "ideall-native-release-"))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const inputDir = path.join(root, "input")
  const outputDir = path.join(root, "output")
  const version = "1.2.3"
  for (const [index, name] of names(version).entries()) {
    const directory = path.join(inputDir, `artifact-${index}`)
    mkdirSync(directory, { recursive: true })
    writeFileSync(path.join(directory, name), `${name}\n`)
  }
  return { root, inputDir, outputDir, version }
}

test("聚合五平台产物并生成严格的 preview 更新清单", (t) => {
  const value = fixture(t)
  const result = prepareNativeRelease({
    ...value,
    channel: "preview",
    repository: "jinziyou/ideall",
    tag: "native-v1.2.3",
    notes: "preview",
    pubDate: "2026-07-22T00:00:00.000Z",
  })
  assert.equal(result.updateName, "native-preview.json")
  assert.equal(result.updateManifest.artifacts.length, 8)
  assert.equal(
    result.updateManifest.artifacts.some((artifact) => artifact.kind === "aab"),
    false,
  )
  assert.match(result.updateManifest.artifacts[0].url, /native-v1\.2\.3/)
})

test("缺少任何平台发行物时拒绝聚合", (t) => {
  const value = fixture(t)
  rmSync(path.join(value.inputDir, "artifact-0"), { recursive: true })
  assert.throws(
    () =>
      prepareNativeRelease({
        ...value,
        channel: "stable",
        repository: "jinziyou/ideall",
        tag: "native-v1.2.3",
      }),
    /缺少原生发行物/,
  )
})

test("finalize 绑定 minisign key id 并覆盖所有校验和", (t) => {
  const value = fixture(t)
  const keys = minisignFixtures()
  prepareNativeRelease({
    ...value,
    channel: "stable",
    repository: "jinziyou/ideall",
    tag: "native-v1.2.3",
    pubDate: "2026-07-22T00:00:00.000Z",
  })
  writeFileSync(path.join(value.outputDir, "native-stable.json.sig"), keys.signature)
  const configDir = path.join(value.root, "native")
  mkdirSync(configDir, { recursive: true })
  writeFileSync(path.join(configDir, "updater-public-key.txt"), keys.publicKey)
  const manifest = finalizeNativeRelease({
    outputDir: value.outputDir,
    version: value.version,
    channel: "stable",
    repository: "jinziyou/ideall",
    tag: "native-v1.2.3",
    root: value.root,
  })
  assert.equal(manifest.minisignKeyId, "0102030405060708")
  assert.match(
    readFileSync(path.join(value.outputDir, "SHA256SUMS"), "utf8"),
    /native-stable\.json\.sig/,
  )
})

test("release 环境优先使用显式版本并兼容全局 IDEALL_VERSION", () => {
  assert.equal(
    nativeReleaseEnvironment({
      IDEALL_VERSION: "1.2.3",
      RELEASE_CHANNEL: "preview",
      GITHUB_REPOSITORY: "jinziyou/ideall",
      RELEASE_TAG: "native-preview",
    }).version,
    "1.2.3",
  )
  assert.equal(
    nativeReleaseEnvironment({
      IDEALL_VERSION: "1.2.3",
      RELEASE_VERSION: "2.0.0",
    }).version,
    "2.0.0",
  )
})
