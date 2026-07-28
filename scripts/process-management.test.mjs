import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { existsSync, readFileSync, writeFileSync } from "node:fs"
import { mkdtemp, rm } from "node:fs/promises"
import net from "node:net"
import { tmpdir } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"

const SELF = fileURLToPath(import.meta.url)
const fixtureMode = process.env.IDEALL_PROCESS_FIXTURE

async function writeStream(stream, text) {
  await new Promise((resolve, reject) => {
    stream.write(text, (error) => (error ? reject(error) : resolve()))
  })
}

async function runFixture(mode) {
  if (mode === "hang") {
    process.on("SIGTERM", () => {})
    setInterval(() => {}, 1_000)
    return
  }
  if (mode === "spam") {
    await writeStream(process.stdout, "stdout:" + "o".repeat(512 * 1024))
    await writeStream(process.stderr, "stderr:" + "e".repeat(512 * 1024))
    return
  }
  if (mode === "fail") {
    console.error("fixture exits abnormally")
    process.exitCode = 7
    return
  }
  if (mode === "grandchild-server") {
    const port = Number(process.env.IDEALL_FIXTURE_PORT)
    const pidFile = process.env.IDEALL_FIXTURE_PID_FILE
    const server = net.createServer()
    await new Promise((resolve, reject) => {
      server.once("error", reject)
      server.listen(port, "127.0.0.1", resolve)
    })
    writeFileSync(pidFile, String(process.pid))
    setInterval(() => {}, 1_000)
    return
  }
  if (mode === "grandchild-parent") {
    const child = spawn(process.execPath, [SELF], {
      env: { ...process.env, IDEALL_PROCESS_FIXTURE: "grandchild-server" },
      stdio: "ignore",
    })
    child.unref()
    process.on("SIGTERM", () => {})
    setInterval(() => {}, 1_000)
    return
  }
  throw new Error(`unknown fixture mode: ${mode}`)
}

if (fixtureMode) {
  await runFixture(fixtureMode)
} else {
  const { afterEach, test } = await import("node:test")
  const { runTestFile } = await import("./lib/test-runner.mjs")
  const { isPortAvailable, sleep } = await import("./lib/process.mjs")
  const tempRoots = []

  afterEach(async () => {
    await Promise.all(tempRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })))
  })

  function fixtureOptions(mode, overrides = {}) {
    return {
      timeoutMs: 2_000,
      termGraceMs: 100,
      killGraceMs: 500,
      logLimitBytes: 8 * 1024,
      execArgv: [],
      env: { ...process.env, IDEALL_PROCESS_FIXTURE: mode, ...overrides },
    }
  }

  async function freePort() {
    const server = net.createServer()
    await new Promise((resolve, reject) => {
      server.once("error", reject)
      server.listen(0, "127.0.0.1", resolve)
    })
    const address = server.address()
    await new Promise((resolve) => server.close(resolve))
    return address.port
  }

  test("runTestFile times out and escalates TERM to KILL", async () => {
    const result = await runTestFile(SELF, {
      ...fixtureOptions("hang"),
      // 高负载 CI 环境也要留出 fixture 安装 SIGTERM handler 的启动窗口。
      timeoutMs: 500,
    })
    assert.equal(result.timedOut, true)
    if (process.platform !== "win32") assert.equal(result.forced, true)
    assert.equal(result.processTreeStopped, true)
    assert.ok(result.durationMs < 3_000)
  })

  test("runTestFile bounds stdout and stderr while retaining diagnostic tails", async () => {
    const result = await runTestFile(SELF, fixtureOptions("spam"))
    assert.equal(result.code, 0)
    assert.equal(result.stdout.truncated, true)
    assert.equal(result.stderr.truncated, true)
    assert.ok(result.stdout.keptBytes <= 8 * 1024)
    assert.ok(result.stderr.keptBytes <= 8 * 1024)
    assert.match(result.stdout.text, /已截断/)
    assert.match(result.stderr.text, /已截断/)
  })

  test("runTestFile reports abnormal exits without treating them as timeouts", async () => {
    const result = await runTestFile(SELF, fixtureOptions("fail"))
    assert.equal(result.timedOut, false)
    assert.equal(result.code, 7)
    assert.match(result.stderr.text, /exits abnormally/)
  })

  test("runTestFile kills descendants in the same managed process group", async () => {
    const root = await mkdtemp(path.join(tmpdir(), "ideall-process-test-"))
    tempRoots.push(root)
    const pidFile = path.join(root, "grandchild.pid")
    const port = await freePort()
    const result = await runTestFile(SELF, {
      ...fixtureOptions("grandchild-parent", {
        IDEALL_FIXTURE_PORT: String(port),
        IDEALL_FIXTURE_PID_FILE: pidFile,
      }),
      // 这里要验证“已启动的孙进程”会被进程组清理；CI 的 CPU 负载下，
      // 两层 Node 冷启动偶尔超过 500ms。给前置监听留足时间，不改变 TERM→KILL 的短宽限期。
      timeoutMs: 2_000,
    })
    assert.equal(result.timedOut, true)
    assert.equal(result.processTreeStopped, true)
    assert.equal(existsSync(pidFile), true, "grandchild should have reached its listening state")
    const grandchildPid = Number(readFileSync(pidFile, "utf8"))
    assert.ok(Number.isSafeInteger(grandchildPid) && grandchildPid > 0)

    let available = false
    for (let attempt = 0; attempt < 40; attempt++) {
      available = await isPortAvailable(port)
      if (available) break
      await sleep(25)
    }
    assert.equal(available, true, "grandchild listening port should be released")
  })
}
