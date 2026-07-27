import { createHash } from "node:crypto"
import {
  copyFileSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { assertSignatureMatchesPublicKey } from "./release-preflight.mjs"

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..")

function sha256(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex")
}

function strictText(value, label) {
  if (typeof value !== "string" || !value.trim() || /[\u0000-\u001f]/.test(value)) {
    throw new Error(`${label}必须是非空文本`)
  }
  return value.trim()
}

function assertVersion(version) {
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
    throw new Error("version 必须是 SemVer")
  }
}

function assetDefinitions(version) {
  const desktop = [
    [`ideall-native-${version}-linux-x86_64.tar.gz`, "linux-x86_64", "tar.gz"],
    [`ideall-native-${version}-linux-x86_64.deb`, "linux-x86_64", "deb"],
    [`ideall-native-${version}-linux-x86_64.rpm`, "linux-x86_64", "rpm"],
    [`ideall-native-${version}-macos-arm64.zip`, "darwin-aarch64", "zip"],
    [`ideall-native-${version}-macos-arm64.dmg`, "darwin-aarch64", "dmg"],
    [`ideall-native-${version}-windows-x86_64.zip`, "windows-x86_64", "zip"],
    [`ideall-native-${version}-windows-x86_64.msi`, "windows-x86_64", "msi"],
    [`ideall-native-${version}-windows-x86_64-setup.exe`, "windows-x86_64", "nsis"],
  ].map(([name, target, kind]) => ({ name, target, kind, update: true }))
  return [
    ...desktop,
    { name: `ideall-native-${version}-android-arm64.aab`, update: false },
    { name: `ideall-native-${version}-ios-arm64.ipa`, update: false },
  ]
}

function walkFiles(directory) {
  const output = []
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name)
    if (entry.isDirectory()) output.push(...walkFiles(file))
    else if (entry.isFile()) output.push(file)
  }
  return output
}

function releaseUrl(repository, tag, name) {
  return `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`
}

export function prepareNativeRelease({
  inputDir,
  outputDir,
  version,
  channel,
  repository,
  tag,
  notes = "",
  pubDate = new Date().toISOString(),
}) {
  assertVersion(strictText(version, "version"))
  if (!["preview", "stable"].includes(channel)) throw new Error("channel 必须是 preview 或 stable")
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) {
    throw new Error("repository 必须是 owner/name")
  }
  strictText(tag, "tag")
  if (!Number.isFinite(Date.parse(pubDate))) throw new Error("pubDate 必须是 ISO 日期")
  if (typeof notes !== "string" || notes.length > 8 * 1024) throw new Error("notes 过长")

  const inputRoot = realpathSync(inputDir)
  const definitions = assetDefinitions(version)
  const wanted = new Map(definitions.map((definition) => [definition.name, definition]))
  const found = new Map()
  for (const candidate of walkFiles(inputRoot)) {
    const name = path.basename(candidate)
    if (!wanted.has(name)) continue
    const resolved = realpathSync(candidate)
    const relative = path.relative(inputRoot, resolved)
    if (relative === "" || relative === ".." || relative.startsWith(`..${path.sep}`)) {
      throw new Error(`artifact 越出输入目录: ${name}`)
    }
    if (found.has(name)) throw new Error(`重复 artifact: ${name}`)
    const stats = statSync(resolved)
    if (!stats.isFile() || stats.size <= 0) throw new Error(`artifact 为空: ${name}`)
    found.set(name, resolved)
  }
  const missing = definitions.filter(({ name }) => !found.has(name)).map(({ name }) => name)
  if (missing.length) throw new Error(`缺少原生发行物: ${missing.join(", ")}`)

  rmSync(outputDir, { recursive: true, force: true })
  mkdirSync(outputDir, { recursive: true })
  const artifacts = []
  for (const definition of definitions) {
    const source = found.get(definition.name)
    const destination = path.join(outputDir, definition.name)
    copyFileSync(source, destination)
    if (definition.update) {
      artifacts.push({
        target: definition.target,
        kind: definition.kind,
        file: definition.name,
        url: releaseUrl(repository, tag, definition.name),
        size: statSync(destination).size,
        sha256: sha256(destination),
      })
    }
  }
  const updateManifest = {
    schemaVersion: 1,
    channel,
    version,
    pubDate,
    notes,
    artifacts,
  }
  const updateName = channel === "preview" ? "native-preview.json" : "native-stable.json"
  writeFileSync(path.join(outputDir, updateName), `${JSON.stringify(updateManifest, null, 2)}\n`)
  return { updateName, updateManifest }
}

export function finalizeNativeRelease({
  outputDir,
  version,
  channel,
  repository,
  tag,
  root = ROOT,
}) {
  assertVersion(strictText(version, "version"))
  const updateName = channel === "preview" ? "native-preview.json" : "native-stable.json"
  const signatureName = `${updateName}.sig`
  const updatePath = path.join(outputDir, updateName)
  const signaturePath = path.join(outputDir, signatureName)
  const updateManifest = JSON.parse(readFileSync(updatePath, "utf8"))
  if (updateManifest.version !== version || updateManifest.channel !== channel) {
    throw new Error("更新清单版本或频道不匹配")
  }
  const publicKey = readFileSync(path.join(root, "native", "updater-public-key.txt"), "utf8").trim()
  const keyId = assertSignatureMatchesPublicKey(readFileSync(signaturePath, "utf8"), publicKey)

  for (const name of ["SHA256SUMS", "native-release-manifest.json"]) {
    rmSync(path.join(outputDir, name), { force: true })
  }
  const names = readdirSync(outputDir).sort()
  const checksums = names.map((name) => `${sha256(path.join(outputDir, name))}  ${name}`).join("\n")
  writeFileSync(path.join(outputDir, "SHA256SUMS"), `${checksums}\n`)
  const files = readdirSync(outputDir)
    .sort()
    .map((name) => {
      const file = path.join(outputDir, name)
      return { name, size: statSync(file).size, sha256: sha256(file) }
    })
  const manifest = {
    schemaVersion: 1,
    channel,
    version,
    tag,
    repository,
    updateManifest: updateName,
    minisignKeyId: keyId,
    files,
  }
  writeFileSync(
    path.join(outputDir, "native-release-manifest.json"),
    `${JSON.stringify(manifest, null, 2)}\n`,
  )
  return manifest
}

export function nativeReleaseEnvironment(environment = process.env) {
  return {
    outputDir: path.resolve(environment.RELEASE_OUTPUT_DIR || "native-release-ready"),
    version: environment.RELEASE_VERSION || environment.IDEALL_VERSION,
    channel: environment.RELEASE_CHANNEL,
    repository: environment.GITHUB_REPOSITORY,
    tag: environment.RELEASE_TAG,
  }
}

function main() {
  const command = process.argv[2]
  const common = nativeReleaseEnvironment()
  if (command === "prepare") {
    const result = prepareNativeRelease({
      ...common,
      inputDir: path.resolve(process.env.RELEASE_INPUT_DIR || "native-release-input"),
      notes: process.env.RELEASE_NOTES || "",
    })
    console.log(`✓ 已准备 ${result.updateManifest.artifacts.length} 个桌面更新目标`)
    return
  }
  if (command === "finalize") {
    const result = finalizeNativeRelease(common)
    console.log(`✓ 原生发行聚合完成: ${result.files.length} 个文件, key=${result.minisignKeyId}`)
    return
  }
  console.log("用法: node scripts/native-release-artifacts.mjs prepare|finalize")
  if (command && command !== "--help" && command !== "-h") process.exitCode = 1
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (isMain) {
  try {
    main()
  } catch (error) {
    console.error(`✗ ${error instanceof Error ? error.message : error}`)
    process.exit(1)
  }
}
