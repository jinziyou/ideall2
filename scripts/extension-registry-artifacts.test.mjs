import assert from "node:assert/strict"
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import os from "node:os"
import path from "node:path"
import test from "node:test"
import {
  buildRegistryPages,
  prepareExtensionRegistry,
  readPreparedExtensionRegistry,
  registryAssetName,
  verifyMinisign,
} from "./extension-registry-artifacts.mjs"
import {
  assertSequenceAdvances,
  registrySequenceFromEnvelope,
} from "./extension-registry-publish.mjs"

function keyFixture() {
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

function fixture(t) {
  const root = mkdtempSync(path.join(os.tmpdir(), "ideall-extension-registry-"))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const keys = keyFixture()
  mkdirSync(path.join(root, "native"), { recursive: true })
  mkdirSync(path.join(root, "registry", "packages"), { recursive: true })
  writeFileSync(path.join(root, "native", "updater-public-key.txt"), keys.publicKey)
  writeFileSync(
    path.join(root, "registry", "extensions.json"),
    JSON.stringify({ schemaVersion: 1, registry: "ideall.official", entries: [] }),
  )
  return { root, keys, outputDir: path.join(root, "ready") }
}

test("empty production catalog becomes a signed, bounded root page", (t) => {
  const { root, keys, outputDir } = fixture(t)
  const manifest = prepareExtensionRegistry({
    root,
    outputDir,
    sequence: 101,
    generatedAt: 1_784_260_000_000,
    expiresAt: 1_785_469_600_000,
    signer: () => keys.signature,
    signatureVerifier: () => {},
  })
  assert.equal(manifest.files.length, 1)
  assert.equal(manifest.files[0].name, "registry.json")
  const envelope = JSON.parse(readFileSync(path.join(outputDir, "registry.json"), "utf8"))
  assert.match(envelope.signature, /^untrusted comment:/)
  const payload = JSON.parse(envelope.payload)
  assert.deepEqual(payload.entries, [])
  assert.equal(payload.cursor, null)
  assert.equal(payload.nextCursor, null)
  assert.equal(registrySequenceFromEnvelope(Buffer.from(JSON.stringify(envelope))), 101)
  assert.equal(readPreparedExtensionRegistry(outputDir).manifest.sequence, 101)
})

test("pagination uses only the server allowlisted opaque cursors", () => {
  const entries = Array.from({ length: 65 }, (_, index) => ({
    id: `extension.${String(index).padStart(3, "0")}`,
  }))
  const pages = buildRegistryPages({
    entries,
    sequence: 2,
    generatedAt: 100,
    expiresAt: 200,
  })
  assert.equal(pages.length, 2)
  assert.equal(JSON.parse(pages[0].payload).nextCursor, "page_0001")
  assert.equal(JSON.parse(pages[1].payload).cursor, "page_0001")
  assert.equal(registryAssetName("page_0001"), "registry-page_0001.json")
  assert.throws(() => registryAssetName("../secret"), /cursor 无效/)
})

test("prepared assets fail closed on tampering and publication sequence rollback", (t) => {
  const { root, keys, outputDir } = fixture(t)
  prepareExtensionRegistry({
    root,
    outputDir,
    sequence: 10,
    generatedAt: 100,
    expiresAt: 200,
    signer: () => keys.signature,
    signatureVerifier: () => {},
  })
  const asset = path.join(outputDir, "registry.json")
  writeFileSync(asset, `${readFileSync(asset, "utf8")} `)
  assert.throws(() => readPreparedExtensionRegistry(outputDir), /校验失败/)
  assert.doesNotThrow(() => assertSequenceAdvances(9, 10))
  assert.throws(() => assertSequenceAdvances(10, 10), /非递增/)
  assert.throws(() => assertSequenceAdvances(11, 10), /非递增/)
})

test("Minisign verification accepts a documented prehashed vector and rejects mutation", () => {
  const publicKey = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3"
  const signature = `untrusted comment: signature from minisign secret key
RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=
trusted comment: timestamp:1633700835\tfile:test\tprehashed
wLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==`
  assert.doesNotThrow(() => verifyMinisign(Buffer.from("test"), signature, publicKey))
  assert.throws(() => verifyMinisign(Buffer.from("changed"), signature, publicKey), /验证失败/)
})
