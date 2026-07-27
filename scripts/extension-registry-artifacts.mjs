import { execFileSync } from "node:child_process"
import { createHash, createPublicKey, verify as verifySignature } from "node:crypto"
import {
  copyFileSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import {
  assertSignatureMatchesPublicKey,
  minisignPublicKeyId,
  minisignSignatureKeyId,
} from "./release-preflight.mjs"

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..")
const CARGO = process.platform === "win32" ? "cargo.exe" : "cargo"
const REGISTRY_ID = "ideall.official"
const RELEASE_TAG = "extension-registry"
const PAGE_LIMIT = 64
const MAX_PAGES = 8
const MAX_ENTRIES = 256
const MAX_PAGE_BYTES = 256 * 1024
const MAX_PACKAGE_BYTES = 96 * 1024 * 1024
const MAX_CONNECTOR_BYTES = 64 * 1024 * 1024
const MAX_SAFE_INTEGER = Number.MAX_SAFE_INTEGER
const MAX_VALIDITY_MS = 30 * 24 * 60 * 60 * 1000
const ALLOWED_PERMISSIONS = new Set(["resources:read", "tools:invoke"])
const CONTENT_DIGEST = /^sha256:[A-Za-z0-9_-]{43}$/
const HEX_SHA256 = /^[a-f0-9]{64}$/
const ID = /^[a-z0-9](?:[a-z0-9.-]{0,126}[a-z0-9])?$/
const PACKAGE_NAME = /^[a-z0-9][a-z0-9._-]{0,159}\.ideall-extension$/
const CONNECTOR_FILE_NAME = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/

function fail(message) {
  throw new Error(`extension registry: ${message}`)
}

function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value)
}

function exactKeys(value, expected, label) {
  if (!isRecord(value)) fail(`${label}必须是对象`)
  const actual = Object.keys(value).sort()
  const wanted = [...expected].sort()
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    fail(`${label}字段不匹配: ${actual.join(",")}`)
  }
}

function safeInteger(value, label) {
  if (!Number.isSafeInteger(value) || value < 1 || value > MAX_SAFE_INTEGER) {
    fail(`${label}必须是正安全整数`)
  }
  return value
}

function boundedText(value, maxBytes, label) {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.trim() !== value ||
    Buffer.byteLength(value) > maxBytes ||
    /[\u0000-\u001f\u007f]/u.test(value)
  ) {
    fail(`${label}无效`)
  }
  return value
}

function sha256Hex(bytes) {
  return createHash("sha256").update(bytes).digest("hex")
}

function sha256Digest(bytes) {
  return `sha256:${createHash("sha256").update(bytes).digest("base64url")}`
}

function safePackagePath(packageDir, packageFile) {
  if (typeof packageFile !== "string" || !PACKAGE_NAME.test(packageFile)) {
    fail(`packageFile 无效: ${JSON.stringify(packageFile)}`)
  }
  const root = realpathSync(packageDir)
  const candidate = path.join(root, packageFile)
  const metadata = lstatSync(candidate)
  if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size < 1) {
    fail(`扩展包不是非空普通文件: ${packageFile}`)
  }
  if (metadata.size > MAX_PACKAGE_BYTES) fail(`扩展包超过 96 MiB: ${packageFile}`)
  const resolved = realpathSync(candidate)
  const relative = path.relative(root, resolved)
  if (!relative || relative === ".." || relative.startsWith(`..${path.sep}`)) {
    fail(`扩展包越出 packages 目录: ${packageFile}`)
  }
  return resolved
}

export function verifyMinisign(content, signatureText, publicPacketText) {
  const publicPacket = Buffer.from(publicPacketText, "base64")
  const lines = signatureText.trim().split(/\r?\n/u)
  if (
    publicPacket.length !== 42 ||
    publicPacket.subarray(0, 2).toString("ascii") !== "Ed" ||
    lines.length !== 4 ||
    !lines[0].startsWith("untrusted comment:") ||
    !lines[2].startsWith("trusted comment: ")
  ) {
    fail("Minisign 公钥或签名文本无效")
  }
  const signaturePacket = Buffer.from(lines[1], "base64")
  const globalSignature = Buffer.from(lines[3], "base64")
  if (
    signaturePacket.length !== 74 ||
    signaturePacket.subarray(0, 2).toString("ascii") !== "ED" ||
    globalSignature.length !== 64 ||
    !signaturePacket.subarray(2, 10).equals(publicPacket.subarray(2, 10))
  ) {
    fail("Minisign signature packet 无效或 key id 不匹配")
  }
  const key = createPublicKey({
    key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), publicPacket.subarray(10)]),
    format: "der",
    type: "spki",
  })
  const signature = signaturePacket.subarray(10)
  const prehash = createHash("blake2b512").update(content).digest()
  const trustedComment = Buffer.from(lines[2].slice("trusted comment: ".length), "utf8")
  if (
    !verifySignature(null, prehash, key, signature) ||
    !verifySignature(null, Buffer.concat([signature, trustedComment]), key, globalSignature)
  ) {
    fail("Minisign signature 验证失败")
  }
}

function parsePackage(file, source, publisherPublicPacket, signatureVerifier) {
  const bytes = readFileSync(file)
  let bundle
  try {
    bundle = JSON.parse(bytes.toString("utf8"))
  } catch {
    fail(`${source.id} 扩展包不是合法 JSON`)
  }
  exactKeys(bundle, ["schemaVersion", "manifest", "signature", "connectorBase64"], "扩展包")
  if (bundle.schemaVersion !== 1) fail(`${source.id} 扩展包 schemaVersion 无效`)
  boundedText(bundle.manifest, 64 * 1024, `${source.id} manifest`)
  boundedText(bundle.signature, 8 * 1024, `${source.id} signature`)
  if (
    typeof bundle.connectorBase64 !== "string" ||
    bundle.connectorBase64.length > Math.ceil((MAX_CONNECTOR_BYTES * 4) / 3) + 8 ||
    !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u.test(
      bundle.connectorBase64,
    )
  ) {
    fail(`${source.id} connectorBase64 无效`)
  }
  const connector = Buffer.from(bundle.connectorBase64, "base64")
  if (connector.length > MAX_CONNECTOR_BYTES) fail(`${source.id} connector 超过 64 MiB`)

  let manifest
  try {
    manifest = JSON.parse(bundle.manifest)
  } catch {
    fail(`${source.id} manifest 不是合法 JSON`)
  }
  exactKeys(
    manifest,
    ["schemaVersion", "id", "label", "version", "publisher", "permissions", "connector"],
    `${source.id} manifest`,
  )
  exactKeys(
    manifest.connector,
    ["protocol", "executable", "sha256", "args"],
    `${source.id} connector`,
  )
  if (
    manifest.schemaVersion !== 1 ||
    manifest.id !== source.id ||
    manifest.label !== source.label ||
    manifest.version !== source.version ||
    manifest.publisher !== source.publisher ||
    JSON.stringify(manifest.permissions) !== JSON.stringify(source.permissions) ||
    manifest.connector.protocol !== "mcp-stdio" ||
    !HEX_SHA256.test(manifest.connector.sha256) ||
    !Array.isArray(manifest.connector.args) ||
    manifest.connector.args.length > 32 ||
    manifest.connector.args.some(
      (argument) =>
        typeof argument !== "string" ||
        Buffer.byteLength(argument) > 1024 ||
        /[\u0000-\u001f\u007f]/u.test(argument),
    )
  ) {
    fail(`${source.id} 扩展包与目录元数据不一致`)
  }
  if (!CONNECTOR_FILE_NAME.test(manifest.connector.executable)) {
    fail(`${source.id} connector executable 无效`)
  }
  if (sha256Hex(connector) !== manifest.connector.sha256) {
    fail(`${source.id} connector SHA-256 不匹配`)
  }
  const digest = sha256Digest(Buffer.from(bundle.manifest, "utf8"))
  if (digest !== source.digest) fail(`${source.id} manifest digest 不匹配`)
  signatureVerifier(Buffer.from(bundle.manifest, "utf8"), bundle.signature, publisherPublicPacket)
  return { bytes, digest }
}

function officialFingerprint(root) {
  const publicKey = readFileSync(path.join(root, "native", "updater-public-key.txt"), "utf8").trim()
  const publicDocument = Buffer.from(publicKey, "base64").toString("utf8").trim()
  const packet = publicDocument
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .find((line) => line && !line.includes("comment:"))
  if (!packet) fail("原生 updater 公钥无效")
  return {
    fingerprint: sha256Digest(Buffer.from(packet, "utf8")),
    packet,
    publicKey,
  }
}

function validateSourceEntry(
  source,
  generatedAt,
  packageDir,
  releaseRepository,
  official,
  signatureVerifier,
) {
  exactKeys(
    source,
    [
      "id",
      "label",
      "summary",
      "version",
      "publisher",
      "publisherFingerprint",
      "permissions",
      "digest",
      "packageFile",
      "publishedAt",
    ],
    "目录条目",
  )
  if (!ID.test(source.id) || !ID.test(source.publisher)) fail("目录条目 id/publisher 无效")
  if (source.publisher !== REGISTRY_ID) {
    fail(`${source.id} 当前生产目录只接受官方 publisher`)
  }
  boundedText(source.label, 160, `${source.id} label`)
  boundedText(source.summary, 512, `${source.id} summary`)
  safeInteger(source.version, `${source.id} version`)
  safeInteger(source.publishedAt, `${source.id} publishedAt`)
  if (source.publishedAt > generatedAt) fail(`${source.id} publishedAt 晚于 generatedAt`)
  if (!CONTENT_DIGEST.test(source.publisherFingerprint) || !CONTENT_DIGEST.test(source.digest)) {
    fail(`${source.id} 内容摘要无效`)
  }
  if (source.publisher === REGISTRY_ID && source.publisherFingerprint !== official.fingerprint) {
    fail(`${source.id} 官方 publisher 指纹与 App updater 根不匹配`)
  }
  if (
    !Array.isArray(source.permissions) ||
    source.permissions.length < 1 ||
    source.permissions.length > ALLOWED_PERMISSIONS.size ||
    source.permissions.some((permission) => !ALLOWED_PERMISSIONS.has(permission)) ||
    source.permissions.some(
      (permission, index) => index > 0 && source.permissions[index - 1] >= permission,
    )
  ) {
    fail(`${source.id} permissions 必须是有序允许列表`)
  }
  const packagePath = safePackagePath(packageDir, source.packageFile)
  const parsed = parsePackage(packagePath, source, official.packet, signatureVerifier)
  return {
    entry: {
      id: source.id,
      label: source.label,
      summary: source.summary,
      version: source.version,
      publisher: source.publisher,
      publisherFingerprint: source.publisherFingerprint,
      permissions: source.permissions,
      digest: source.digest,
      packageUrl: `https://github.com/${releaseRepository}/releases/download/${RELEASE_TAG}/${encodeURIComponent(source.packageFile)}`,
      packageSha256: sha256Hex(parsed.bytes),
      publishedAt: source.publishedAt,
    },
    packagePath,
    packageFile: source.packageFile,
  }
}

function pageCursor(index) {
  return `page_${String(index).padStart(4, "0")}`
}

export function registryAssetName(cursor) {
  if (cursor === null) return "registry.json"
  if (!/^page_000[1-7]$/u.test(cursor)) fail(`cursor 无效: ${cursor}`)
  return `registry-${cursor}.json`
}

export function buildRegistryPages({ entries, sequence, generatedAt, expiresAt }) {
  safeInteger(sequence, "sequence")
  safeInteger(generatedAt, "generatedAt")
  safeInteger(expiresAt, "expiresAt")
  if (expiresAt <= generatedAt || expiresAt - generatedAt > MAX_VALIDITY_MS) {
    fail("有效期必须大于 0 且不超过 30 天")
  }
  if (!Array.isArray(entries) || entries.length > MAX_ENTRIES) fail("目录条目超过 256")
  if (entries.some((entry, index) => index > 0 && entries[index - 1].id >= entry.id)) {
    fail("目录条目必须按 id 严格递增")
  }
  const pageCount = Math.max(1, Math.ceil(entries.length / PAGE_LIMIT))
  if (pageCount > MAX_PAGES) fail("目录页超过 8")
  return Array.from({ length: pageCount }, (_, index) => {
    const cursor = index === 0 ? null : pageCursor(index)
    const nextCursor = index + 1 < pageCount ? pageCursor(index + 1) : null
    const payload = JSON.stringify({
      schemaVersion: 1,
      registry: REGISTRY_ID,
      sequence,
      generatedAt,
      expiresAt,
      cursor,
      nextCursor,
      entries: entries.slice(index * PAGE_LIMIT, (index + 1) * PAGE_LIMIT),
    })
    if (Buffer.byteLength(payload) > MAX_PAGE_BYTES) fail(`目录页 ${index + 1} 超过 256 KiB`)
    return { cursor, payload, assetName: registryAssetName(cursor) }
  })
}

function defaultSigner(payloadFile, { root }) {
  execFileSync(
    CARGO,
    [
      "run",
      "--locked",
      "--manifest-path",
      path.join(root, "native", "Cargo.toml"),
      "-p",
      "ideall-cli",
      "--",
      "update-sign",
      "--manifest",
      payloadFile,
      "--signature",
      `${payloadFile}.sig`,
    ],
    {
      cwd: root,
      env: process.env,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      timeout: 120_000,
    },
  )
  return readFileSync(`${payloadFile}.sig`, "utf8").trim()
}

function decodeNativeSignature(encoded, publicKey, publicPacket, content, signatureVerifier) {
  assertSignatureMatchesPublicKey(encoded, publicKey)
  const text = Buffer.from(encoded, "base64").toString("utf8").trim()
  const lines = text.split(/\r?\n/u)
  if (
    lines.length !== 4 ||
    !lines[0].startsWith("untrusted comment:") ||
    !lines[2].startsWith("trusted comment:")
  ) {
    fail("原生 signer 输出不是完整 Minisign signature 文本")
  }
  signatureVerifier(Buffer.from(content, "utf8"), text, publicPacket)
  return text
}

export function prepareExtensionRegistry({
  root = ROOT,
  sourceFile = path.join(root, "registry", "extensions.json"),
  packageDir = path.join(root, "registry", "packages"),
  outputDir,
  releaseRepository = "jinziyou/ideall",
  sequence,
  generatedAt,
  expiresAt,
  signer = defaultSigner,
  signatureVerifier = verifyMinisign,
}) {
  const source = JSON.parse(readFileSync(sourceFile, "utf8"))
  exactKeys(source, ["schemaVersion", "registry", "entries"], "目录源文件")
  if (
    source.schemaVersion !== 1 ||
    source.registry !== REGISTRY_ID ||
    !Array.isArray(source.entries)
  ) {
    fail("目录源文件 schema/registry/entries 无效")
  }
  if (source.entries.length > MAX_ENTRIES) fail("目录源文件条目超过 256")
  const official = officialFingerprint(root)
  const normalized = source.entries.map((entry) =>
    validateSourceEntry(
      entry,
      generatedAt,
      packageDir,
      releaseRepository,
      official,
      signatureVerifier,
    ),
  )
  normalized.sort((left, right) => left.entry.id.localeCompare(right.entry.id))
  if (
    normalized.some((item, index) => index > 0 && normalized[index - 1].entry.id === item.entry.id)
  ) {
    fail("目录源文件包含重复 id")
  }
  const packageNames = new Set()
  for (const item of normalized) {
    if (packageNames.has(item.packageFile)) fail(`扩展包文件名重复: ${item.packageFile}`)
    packageNames.add(item.packageFile)
  }
  const pages = buildRegistryPages({
    entries: normalized.map((item) => item.entry),
    sequence,
    generatedAt,
    expiresAt,
  })

  rmSync(outputDir, { recursive: true, force: true })
  mkdirSync(outputDir, { recursive: true })
  for (const item of normalized)
    copyFileSync(item.packagePath, path.join(outputDir, item.packageFile))

  const pageFiles = []
  for (const page of pages) {
    const payloadFile = path.join(outputDir, `${page.assetName}.payload`)
    writeFileSync(payloadFile, page.payload, { encoding: "utf8", mode: 0o600 })
    let signature
    try {
      signature = decodeNativeSignature(
        signer(payloadFile, { root, page }),
        official.publicKey,
        official.packet,
        page.payload,
        signatureVerifier,
      )
    } finally {
      rmSync(payloadFile, { force: true })
      rmSync(`${payloadFile}.sig`, { force: true })
    }
    const envelope = { schemaVersion: 1, payload: page.payload, signature }
    const envelopeBytes = Buffer.from(`${JSON.stringify(envelope, null, 2)}\n`)
    if (envelopeBytes.length > MAX_PAGE_BYTES) fail(`${page.assetName} 信封超过 256 KiB`)
    writeFileSync(path.join(outputDir, page.assetName), envelopeBytes)
    pageFiles.push(page.assetName)
  }

  const assetNames = [...pageFiles, ...normalized.map((item) => item.packageFile)].sort()
  const files = assetNames.map((name) => {
    const file = path.join(outputDir, name)
    return { name, size: statSync(file).size, sha256: sha256Hex(readFileSync(file)) }
  })
  const manifest = {
    schemaVersion: 1,
    registry: REGISTRY_ID,
    sequence,
    generatedAt,
    expiresAt,
    releaseRepository,
    releaseTag: RELEASE_TAG,
    rootAsset: "registry.json",
    files,
  }
  writeFileSync(
    path.join(outputDir, "extension-registry-manifest.json"),
    `${JSON.stringify(manifest, null, 2)}\n`,
  )
  return manifest
}

export function readPreparedExtensionRegistry(directory) {
  const manifest = JSON.parse(
    readFileSync(path.join(directory, "extension-registry-manifest.json"), "utf8"),
  )
  exactKeys(
    manifest,
    [
      "schemaVersion",
      "registry",
      "sequence",
      "generatedAt",
      "expiresAt",
      "releaseRepository",
      "releaseTag",
      "rootAsset",
      "files",
    ],
    "prepared manifest",
  )
  if (
    manifest.schemaVersion !== 1 ||
    manifest.registry !== REGISTRY_ID ||
    manifest.releaseTag !== RELEASE_TAG ||
    manifest.rootAsset !== "registry.json" ||
    !Array.isArray(manifest.files) ||
    manifest.files.length < 1
  ) {
    fail("prepared manifest 无效")
  }
  safeInteger(manifest.sequence, "prepared sequence")
  safeInteger(manifest.generatedAt, "prepared generatedAt")
  safeInteger(manifest.expiresAt, "prepared expiresAt")
  const names = new Set()
  const files = manifest.files.map((entry) => {
    exactKeys(entry, ["name", "size", "sha256"], "prepared file")
    if (
      typeof entry.name !== "string" ||
      path.basename(entry.name) !== entry.name ||
      names.has(entry.name) ||
      !Number.isSafeInteger(entry.size) ||
      entry.size < 1 ||
      !HEX_SHA256.test(entry.sha256)
    ) {
      fail("prepared file 条目无效")
    }
    names.add(entry.name)
    const file = path.join(directory, entry.name)
    const metadata = lstatSync(file)
    if (!metadata.isFile() || metadata.isSymbolicLink())
      fail(`prepared asset 非普通文件: ${entry.name}`)
    if (metadata.size !== entry.size || sha256Hex(readFileSync(file)) !== entry.sha256) {
      fail(`prepared asset 校验失败: ${entry.name}`)
    }
    return { ...entry, path: file }
  })
  if (!names.has(manifest.rootAsset)) fail("prepared manifest 缺少 registry.json")
  const unexpected = readdirSync(directory).filter(
    (name) => name !== "extension-registry-manifest.json" && !names.has(name),
  )
  if (unexpected.length > 0) fail(`prepared 目录包含未声明文件: ${unexpected.join(",")}`)
  return { manifest, files }
}

export function assertPreparedSignatureKey(directory, publicKey) {
  const prepared = readPreparedExtensionRegistry(directory)
  const publicDocument = Buffer.from(publicKey, "base64").toString("utf8").trim()
  const publicPacket = publicDocument.split(/\r?\n/u)[1]
  if (!publicPacket) fail("updater 公钥无效")
  for (const file of prepared.files.filter((entry) => entry.name.endsWith(".json"))) {
    const envelope = JSON.parse(readFileSync(file.path, "utf8"))
    exactKeys(envelope, ["schemaVersion", "payload", "signature"], file.name)
    if (
      envelope.schemaVersion !== 1 ||
      minisignSignatureKeyId(Buffer.from(envelope.signature).toString("base64")) !==
        minisignPublicKeyId(publicKey)
    ) {
      fail(`${file.name} 签名 key id 不匹配`)
    }
    verifyMinisign(Buffer.from(envelope.payload, "utf8"), envelope.signature, publicPacket)
  }
  return prepared
}

function main() {
  const command = process.argv[2]
  if (command === "prepare") {
    const generatedAt = Number(process.env.REGISTRY_GENERATED_AT || Date.now())
    const sequence = Number(process.env.REGISTRY_SEQUENCE || generatedAt)
    const validityDays = Number(process.env.REGISTRY_VALIDITY_DAYS || 14)
    if (!Number.isSafeInteger(validityDays) || validityDays < 1 || validityDays > 30) {
      fail("REGISTRY_VALIDITY_DAYS 必须是 1..30 的整数")
    }
    const outputDir = path.resolve(process.env.REGISTRY_OUTPUT_DIR || "registry-ready")
    const manifest = prepareExtensionRegistry({
      outputDir,
      releaseRepository: process.env.GITHUB_REPOSITORY || "jinziyou/ideall",
      sequence,
      generatedAt,
      expiresAt: generatedAt + validityDays * 24 * 60 * 60 * 1000,
    })
    console.log(
      `✓ Extension Registry 已准备: sequence=${manifest.sequence}, entries=${JSON.parse(readFileSync(path.join(ROOT, "registry", "extensions.json"), "utf8")).entries.length}, assets=${manifest.files.length}`,
    )
    return
  }
  if (command === "verify") {
    const directory = path.resolve(process.env.REGISTRY_OUTPUT_DIR || "registry-ready")
    const publicKey = readFileSync(
      path.join(ROOT, "native", "updater-public-key.txt"),
      "utf8",
    ).trim()
    const prepared = assertPreparedSignatureKey(directory, publicKey)
    console.log(`✓ Extension Registry 产物校验通过: sequence=${prepared.manifest.sequence}`)
    return
  }
  console.log(`用法:
  node scripts/extension-registry-artifacts.mjs prepare
  node scripts/extension-registry-artifacts.mjs verify`)
  if (command && command !== "--help" && command !== "-h") process.exitCode = 2
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (isMain) {
  try {
    main()
  } catch (error) {
    console.error(`✗ ${error instanceof Error ? error.message : error}`)
    process.exitCode = 1
  }
}
