// 维护脚本测试 runner：自动发现 scripts/*.test.mjs，并让每个文件在独立进程中执行，
// 避免共享 argv/env/global，同时绕开部分 Node 22 发行包的 `node --test` glob 兼容问题。
import { readdir } from "node:fs/promises"
import { availableParallelism } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { runTestFile } from "./lib/test-runner.mjs"

const SCRIPT_PATH = fileURLToPath(import.meta.url)
const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const HELP = `用法:
  pnpm test:scripts
  pnpm test:scripts -- <路径或文件名子串...>
  node scripts/run-script-tests.mjs [路径或文件名子串...]

说明:
  自动发现并运行 scripts/*.test.mjs。传入一个或多个子串时，只运行路径包含任一
  子串的测试文件；每个文件在隔离子进程中执行。
`

// 进程树时序测试需要先让 fixture 安装 signal handler，不能和其它高负载子进程争抢启动窗口。
const SERIAL_TEST_NAMES = new Set(["process-management.test.mjs"])

export function parseScriptTestArgs(argv) {
  const options = { filters: [], help: false }
  for (const argument of argv) {
    if (argument === "--") continue
    if (argument === "--help" || argument === "-h") {
      options.help = true
      continue
    }
    if (argument.startsWith("-")) throw new Error(`未知选项: ${argument}`)
    options.filters.push(argument)
  }
  return options
}

export async function discoverScriptTests(directory = SCRIPT_DIR) {
  const entries = await readdir(directory, { withFileTypes: true })
  return entries
    .filter((entry) => entry.isFile() && entry.name.endsWith(".test.mjs"))
    .map((entry) => path.join(directory, entry.name))
    .sort()
}

function passed(result) {
  return (
    !result.timedOut &&
    !result.error &&
    result.code === 0 &&
    !result.signal &&
    result.processTreeStopped
  )
}

async function runBatch(files, concurrency, results) {
  let nextIndex = 0
  const workerCount = Math.min(files.length, concurrency)
  async function worker() {
    while (nextIndex < files.length) {
      const file = files[nextIndex++]
      const relative = path.relative(process.cwd(), file)
      console.log(`START ${relative}`)
      const result = await runTestFile(file)
      results.push({ relative, ...result })
      if (passed(result)) {
        console.log(`PASS  ${relative} (${result.durationMs}ms)`)
        if (result.stderr.text) process.stderr.write(result.stderr.text)
        continue
      }

      process.exitCode = 1
      console.error(`FAIL  ${relative} (${result.durationMs}ms)`)
      if (result.stdout.text) process.stdout.write(result.stdout.text)
      if (result.stderr.text) process.stderr.write(result.stderr.text)
    }
  }
  await Promise.all(Array.from({ length: workerCount }, () => worker()))
}

export async function main(argv = process.argv.slice(2)) {
  let options
  try {
    options = parseScriptTestArgs(argv)
  } catch (error) {
    console.error(`script test runner: ${error.message}`)
    process.exitCode = 1
    return
  }
  if (options.help) {
    console.log(HELP.trimEnd())
    return
  }

  const testFiles = (await discoverScriptTests()).filter(
    (file) =>
      options.filters.length === 0 || options.filters.some((filter) => file.includes(filter)),
  )
  if (testFiles.length === 0) {
    console.error("未找到匹配的测试文件 (scripts/*.test.mjs)")
    process.exitCode = 1
    return
  }

  const results = []
  const parallelFiles = testFiles.filter((file) => !SERIAL_TEST_NAMES.has(path.basename(file)))
  const serialFiles = testFiles.filter((file) => SERIAL_TEST_NAMES.has(path.basename(file)))
  // 时序测试先在空闲启动窗口执行，避免前一批构建/HTTP fixture 的回收抖动影响 150ms 边界。
  await runBatch(serialFiles, 1, results)
  await runBatch(parallelFiles, Math.min(availableParallelism(), 4), results)
  const successCount = results.filter(passed).length
  console.log(`\nSUMMARY ${successCount} passed, ${results.length - successCount} failed`)
}

if (process.argv[1] && path.resolve(process.argv[1]) === SCRIPT_PATH) await main()
