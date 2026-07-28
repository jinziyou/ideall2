import { spawn } from "node:child_process"
import { createServer } from "node:net"

export const NODE = process.execPath
export const PNPM = process.platform === "win32" ? "pnpm.cmd" : "pnpm"
export const IS_WIN = process.platform === "win32"

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const MANAGED_PROCESS_GROUP = Symbol("ideall.managed-process-group")

export function runCommand(command, args, { env = process.env, cwd = process.cwd() } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env,
      stdio: "inherit",
    })
    child.on("error", reject)
    child.on("exit", (code, signal) => {
      if (code === 0) {
        resolve()
        return
      }
      reject(new Error(`${command} ${args.join(" ")} exited with ${signal ?? code}`))
    })
  })
}

export function runPnpm(args, options) {
  return runCommand(PNPM, args, options)
}

export function spawnDetached(command, args, { env = process.env, cwd = process.cwd() } = {}) {
  const child = spawn(command, args, {
    cwd,
    detached: !IS_WIN,
    env,
    stdio: "inherit",
  })
  child[MANAGED_PROCESS_GROUP] = !IS_WIN
  return child
}

/** 启动需要捕获输出的独立进程组；供测试 runner 等长流程复用。 */
export function spawnCaptured(
  command,
  args,
  { env = process.env, cwd = process.cwd(), execArgv = undefined } = {},
) {
  const finalArgs = execArgv ? [...execArgv, ...args] : args
  const child = spawn(command, finalArgs, {
    cwd,
    detached: !IS_WIN,
    env,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  })
  child[MANAGED_PROCESS_GROUP] = !IS_WIN
  return child
}

export function isPortAvailable(port) {
  return new Promise((resolve) => {
    const server = createServer()
    server.once("error", () => resolve(false))
    server.once("listening", () => server.close(() => resolve(true)))
    server.listen(port)
  })
}

export async function waitForHttpReady({
  url,
  child,
  timeoutMs,
  requestTimeoutMs = 2_000,
  intervalMs = 1_000,
  isReady = (response) => response.status < 500,
  timeoutMessage = `server did not become ready at ${url}`,
}) {
  const end = Date.now() + timeoutMs
  while (Date.now() < end) {
    if (child.exitCode != null || child.signalCode != null) return false
    try {
      const response = await fetch(url, { signal: AbortSignal.timeout(requestTimeoutMs) })
      if (isReady(response)) return true
    } catch {
      // Keep polling until the server finishes booting or the child exits.
    }
    await sleep(intervalMs)
  }
  throw new Error(timeoutMessage)
}

function processTreeAlive(child) {
  if (!child?.pid) return false
  if (IS_WIN || !child[MANAGED_PROCESS_GROUP]) {
    return child.exitCode == null && child.signalCode == null
  }
  try {
    process.kill(-child.pid, 0)
    return true
  } catch {
    return false
  }
}

async function signalProcessTree(child, signal) {
  if (!child?.pid) return
  if (!IS_WIN && child[MANAGED_PROCESS_GROUP]) {
    try {
      process.kill(-child.pid, signal)
    } catch {
      // 进程组已经退出即视为清理完成。
    }
    return
  }

  if (IS_WIN) {
    await new Promise((resolve) => {
      const args = ["/PID", String(child.pid), "/T"]
      if (signal === "SIGKILL") args.push("/F")
      const killer = spawn("taskkill", args, { stdio: "ignore", windowsHide: true })
      killer.once("error", resolve)
      killer.once("exit", resolve)
    })
    return
  }

  try {
    child.kill(signal)
  } catch {
    // 子进程已经退出。
  }
}

async function waitForProcessTreeExit(child, timeoutMs) {
  const deadline = Date.now() + timeoutMs
  while (processTreeAlive(child) && Date.now() < deadline) await sleep(25)
  return !processTreeAlive(child)
}

/**
 * 先向整个受管进程组发送 TERM，超时后再 KILL。`cleanupExitedGroup` 用于直接子进程已经
 * 退出、但可能遗留孙进程的 runner 场景；普通调用保持旧语义即可。
 */
export async function stopChildProcess(
  child,
  { timeoutMs = 5_000, killTimeoutMs = 1_000, cleanupExitedGroup = false } = {},
) {
  if (!child?.pid) return { forced: false, stopped: true }
  if (!cleanupExitedGroup && child.exitCode != null && child.signalCode == null) {
    return { forced: false, stopped: true }
  }
  if (!processTreeAlive(child)) return { forced: false, stopped: true }

  await signalProcessTree(child, "SIGTERM")
  if (await waitForProcessTreeExit(child, timeoutMs)) return { forced: false, stopped: true }

  await signalProcessTree(child, "SIGKILL")
  const stopped = await waitForProcessTreeExit(child, killTimeoutMs)
  return { forced: true, stopped }
}

export function waitForChildExit(child) {
  if (child.exitCode != null || child.signalCode != null) {
    return Promise.resolve({ code: child.exitCode, signal: child.signalCode, error: null })
  }
  return new Promise((resolve) => {
    let settled = false
    const finish = (result) => {
      if (settled) return
      settled = true
      resolve(result)
    }
    child.once("error", (error) => finish({ code: null, signal: null, error }))
    // `close` 晚于 `exit`，此时 stdout/stderr 已关闭，失败日志不会丢最后一段。
    child.once("close", (code, signal) => finish({ code, signal, error: null }))
  })
}

function createBoundedOutput(maxBytes) {
  const chunks = []
  let keptBytes = 0
  let totalBytes = 0

  return {
    append(chunk) {
      const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk)
      totalBytes += buffer.length
      if (maxBytes <= 0) return
      chunks.push(buffer)
      keptBytes += buffer.length
      while (keptBytes > maxBytes && chunks.length > 0) {
        const excess = keptBytes - maxBytes
        const first = chunks[0]
        if (first.length <= excess) {
          chunks.shift()
          keptBytes -= first.length
        } else {
          chunks[0] = first.subarray(excess)
          keptBytes -= excess
        }
      }
    },
    snapshot() {
      const droppedBytes = totalBytes - keptBytes
      const tail = Buffer.concat(chunks, keptBytes).toString("utf8")
      return {
        text: droppedBytes > 0 ? `[... 已截断 ${droppedBytes} bytes ...]\n${tail}` : tail,
        totalBytes,
        keptBytes,
        droppedBytes,
        truncated: droppedBytes > 0,
      }
    },
  }
}

/** 运行一个受管进程组，捕获有限输出，并在超时后完成 TERM→KILL。 */
export async function runCapturedProcess(
  command,
  args,
  {
    cwd = process.cwd(),
    env = process.env,
    execArgv,
    timeoutMs = 120_000,
    termTimeoutMs = 2_000,
    killTimeoutMs = 1_000,
    maxOutputBytes = 256 * 1024,
  } = {},
) {
  const startedAt = Date.now()
  const stdout = createBoundedOutput(maxOutputBytes)
  const stderr = createBoundedOutput(maxOutputBytes)
  const child = spawnCaptured(command, args, { cwd, env, execArgv })
  child.stdout?.on("data", (chunk) => stdout.append(chunk))
  child.stderr?.on("data", (chunk) => stderr.append(chunk))

  const exitPromise = waitForChildExit(child)
  let timer
  const timeoutPromise = new Promise((resolve) => {
    timer = setTimeout(() => resolve({ timeout: true }), timeoutMs)
    timer.unref?.()
  })
  const first = await Promise.race([exitPromise, timeoutPromise])
  let timedOut = false
  let termination = { forced: false, stopped: true }
  let outcome

  if (first?.timeout) {
    timedOut = true
    termination = await stopChildProcess(child, {
      timeoutMs: termTimeoutMs,
      killTimeoutMs,
      cleanupExitedGroup: true,
    })
    let exitFallbackTimer
    try {
      outcome = await Promise.race([
        exitPromise,
        new Promise((resolve) => {
          exitFallbackTimer = setTimeout(
            () =>
              resolve({
                code: child.exitCode,
                signal: child.signalCode ?? (termination.forced ? "SIGKILL" : "SIGTERM"),
                error: null,
              }),
            killTimeoutMs + 100,
          )
          exitFallbackTimer.unref?.()
        }),
      ])
    } finally {
      clearTimeout(exitFallbackTimer)
    }
  } else {
    outcome = first
    // 测试进程可能退出后遗留孙进程；立即回收同一受管进程组。
    termination = await stopChildProcess(child, {
      timeoutMs: termTimeoutMs,
      killTimeoutMs,
      cleanupExitedGroup: true,
    })
  }
  clearTimeout(timer)

  return {
    ...outcome,
    timedOut,
    forced: termination.forced,
    processTreeStopped: termination.stopped,
    durationMs: Date.now() - startedAt,
    stdout: stdout.snapshot(),
    stderr: stderr.snapshot(),
  }
}

export function installShutdownHandlers(stop) {
  let shuttingDown = false
  const handle = (exitCode) => async () => {
    if (shuttingDown) return
    shuttingDown = true
    try {
      await stop()
    } finally {
      process.exit(exitCode)
    }
  }
  const onSigint = handle(130)
  const onSigterm = handle(143)
  process.on("SIGINT", onSigint)
  process.on("SIGTERM", onSigterm)
  return () => {
    process.off("SIGINT", onSigint)
    process.off("SIGTERM", onSigterm)
  }
}
