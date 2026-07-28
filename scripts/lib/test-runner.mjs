import { runCapturedProcess } from "./process.mjs"

const DEFAULT_TIMEOUT_MS = 120_000
const DEFAULT_LOG_LIMIT_KB = 256
const DEFAULT_SLOW_COUNT = 8
const DEFAULT_TERM_GRACE_MS = 2_000
const DEFAULT_KILL_GRACE_MS = 1_000

function integerSetting(raw, name, { min = 0, max = Number.MAX_SAFE_INTEGER } = {}) {
  const value = Number(raw)
  if (!Number.isSafeInteger(value) || value < min || value > max) {
    throw new Error(`${name} 必须是 ${min}..${max} 的整数`)
  }
  return value
}

function envSetting(env, key, fallback, range) {
  return env[key] === undefined ? fallback : integerSetting(env[key], key, range)
}

export function parseRunnerArgs(argv, env = process.env) {
  const help = argv.includes("--help") || argv.includes("-h")
  const settingsEnv = help ? {} : env
  const options = {
    timeoutMs: envSetting(settingsEnv, "IDEALL_TEST_TIMEOUT_MS", DEFAULT_TIMEOUT_MS, {
      min: 100,
      max: 60 * 60 * 1000,
    }),
    logLimitBytes:
      envSetting(settingsEnv, "IDEALL_TEST_LOG_LIMIT_KB", DEFAULT_LOG_LIMIT_KB, {
        min: 1,
        max: 64 * 1024,
      }) * 1024,
    slowCount: envSetting(settingsEnv, "IDEALL_TEST_SLOW_COUNT", DEFAULT_SLOW_COUNT, {
      min: 0,
      max: 100,
    }),
    termGraceMs: envSetting(settingsEnv, "IDEALL_TEST_TERM_GRACE_MS", DEFAULT_TERM_GRACE_MS, {
      min: 10,
      max: 60_000,
    }),
    killGraceMs: envSetting(settingsEnv, "IDEALL_TEST_KILL_GRACE_MS", DEFAULT_KILL_GRACE_MS, {
      min: 10,
      max: 60_000,
    }),
    filters: [],
    help,
  }
  if (help) return options

  const valueOptions = new Map([
    ["--timeout-ms", ["timeoutMs", 100, 60 * 60 * 1000, 1]],
    ["--log-limit-kb", ["logLimitBytes", 1, 64 * 1024, 1024]],
    ["--slow-count", ["slowCount", 0, 100, 1]],
  ])
  for (let index = 0; index < argv.length; index++) {
    const argument = argv[index]
    if (argument === "--") continue
    const equals = argument.startsWith("--") ? argument.indexOf("=") : -1
    const optionName = equals > 0 ? argument.slice(0, equals) : argument
    const spec = valueOptions.get(optionName)
    if (spec) {
      const raw = equals > 0 ? argument.slice(equals + 1) : argv[++index]
      if (raw === undefined) throw new Error(`${optionName} 缺少参数`)
      const [property, min, max, multiplier] = spec
      options[property] = integerSetting(raw, optionName, { min, max }) * multiplier
      continue
    }
    if (argument.startsWith("-")) throw new Error(`未知选项: ${argument}`)
    options.filters.push(argument)
  }
  return options
}

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
