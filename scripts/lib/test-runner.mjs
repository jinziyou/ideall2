import { runCapturedProcess } from "./process.mjs"

const DEFAULT_TIMEOUT_MS = 120_000
const DEFAULT_LOG_LIMIT_KB = 256
const DEFAULT_TERM_GRACE_MS = 2_000
const DEFAULT_KILL_GRACE_MS = 1_000

export function runTestFile(
  file,
  {
    timeoutMs = DEFAULT_TIMEOUT_MS,
    logLimitBytes = DEFAULT_LOG_LIMIT_KB * 1024,
    termGraceMs = DEFAULT_TERM_GRACE_MS,
    killGraceMs = DEFAULT_KILL_GRACE_MS,
    env = process.env,
    execArgv = process.execArgv,
  } = {},
) {
  return runCapturedProcess(process.execPath, [file], {
    env,
    execArgv,
    timeoutMs,
    termTimeoutMs: termGraceMs,
    killTimeoutMs: killGraceMs,
    maxOutputBytes: logLimitBytes,
  })
}
