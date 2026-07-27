import {
  NODE,
  installShutdownHandlers,
  isPortAvailable,
  runCommand,
  runPnpm,
  spawnDetached,
  stopChildProcess,
  waitForHttpReady,
} from "./lib/process.mjs"

const PORTS = [5030, 5031, 5032, 5033]
const READY_TIMEOUT_MS = 45_000
const HELP = `用法:
  pnpm verify:smoke:static
  node scripts/verify-static-smoke.mjs [--no-build] [smoke:notes|smoke:files|smoke:files:preview|smoke:plugins|smoke:trash ...]

说明:
  对迁移期 Web 兼容层运行浏览器冒烟：可选执行 pnpm legacy:web:export，检查 out/，启动静态预览服，再运行指定 smoke 脚本。

环境变量:
  SMOKE_LEVEL=core|full  控制部分冒烟脚本的覆盖深度，默认 full。
`

let staticServer = null

function startStaticServer(port) {
  return spawnDetached(NODE, ["scripts/serve-out.mjs"], {
    env: { ...process.env, PORT: String(port) },
  })
}

async function waitForServer(baseUrl, child) {
  return waitForHttpReady({
    url: `${baseUrl}/home/notes`,
    child,
    timeoutMs: READY_TIMEOUT_MS,
    timeoutMessage: `static server did not become ready at ${baseUrl}`,
  })
}

async function stopStaticServer() {
  const child = staticServer
  staticServer = null
  await stopChildProcess(child)
}

async function startReadyStaticServer() {
  for (const port of PORTS) {
    const baseUrl = `http://localhost:${port}`
    if (!(await isPortAvailable(port))) {
      console.log(`[verify:static-smoke] port ${port} is already in use; trying next port`)
      continue
    }

    console.log(`\n[verify:static-smoke] starting static server at ${baseUrl}`)
    staticServer = startStaticServer(port)
    const ready = await waitForServer(baseUrl, staticServer)
    if (ready) return baseUrl

    await stopStaticServer()
    console.log(`[verify:static-smoke] port ${port} unavailable; trying next port`)
  }
  throw new Error(`could not start a static server on ports ${PORTS.join(", ")}`)
}

async function main() {
  const args = process.argv.slice(2)
  if (args.includes("--help") || args.includes("-h")) {
    console.log(HELP.trimEnd())
    return
  }

  const noBuild = args.includes("--no-build")
  const smokeLevel = (process.env.SMOKE_LEVEL || "full").toLowerCase() === "core" ? "core" : "full"
  const defaultSmokeScripts = ["smoke:notes", "smoke:plugins", "smoke:trash", "smoke:files"]
  const allowedSmokeScripts = [...defaultSmokeScripts, "smoke:files:preview"]
  const requestedScripts = args.filter((arg) => arg !== "--" && arg !== "--no-build")
  const smokeScripts = requestedScripts.length ? requestedScripts : defaultSmokeScripts
  const unknownScript = smokeScripts.find((script) => !allowedSmokeScripts.includes(script))
  if (unknownScript) {
    throw new Error(`unknown smoke script: ${unknownScript}`)
  }

  if (!noBuild) {
    console.log("[verify:static-smoke] building and validating the static export")
    await runPnpm(["legacy:web:export"])
  } else {
    await runCommand(NODE, ["scripts/check-static-export.mjs"])
  }

  const baseUrl = await startReadyStaticServer()
  const env = { ...process.env, BASE: baseUrl, SMOKE_LEVEL: smokeLevel }

  try {
    for (const script of smokeScripts) {
      console.log(
        `\n[verify:static-smoke] running ${script} against ${baseUrl} (level=${smokeLevel})`,
      )
      await runPnpm([script], { env })
    }
  } finally {
    await stopStaticServer()
  }
}

installShutdownHandlers(stopStaticServer)

main().catch(async (error) => {
  await stopStaticServer()
  console.error(`\n[verify:static-smoke] ${error.message}`)
  process.exit(1)
})
