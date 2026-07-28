import { readFileSync } from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

export const PROJECT_ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..")

const VERSION_RE =
  /^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/

const VERSION_FILES = [
  {
    path: "package.json",
    pattern: /("version"\s*:\s*")([^"]+)(")/,
  },
  {
    path: "native/Cargo.toml",
    pattern: /(\[workspace\.package\][\s\S]*?^version\s*=\s*")([^"]+)(")/m,
  },
  {
    path: "native/Cargo.lock",
    pattern: /(name = "ideall(?:-[^"]+)?"\r?\nversion = ")([^"]+)(")/,
    multiple: true,
  },
  {
    path: "native/scripts/package-desktop.sh",
    pattern: /(version="\$\{IDEALL_VERSION:-)([^}]+)(\}")/,
  },
  {
    path: "native/apps/ideall-mobile/build-mobile.sh",
    pattern: /(version="\$\{IDEALL_VERSION:-)([^}]+)(\}")/,
  },
  {
    path: "native/apps/ideall-mobile/platforms/android/app/build.gradle.kts",
    pattern: /(val ideallVersionName = System\.getenv\("IDEALL_VERSION"\) \?: ")([^"]+)(")/,
  },
]

export function validateProjectVersion(version, label = "版本号") {
  if (typeof version !== "string" || !VERSION_RE.test(version)) {
    throw new Error(
      `${label}不是受支持的 SemVer（期望 x.y.z 或 x.y.z-prerelease）: ${version ?? ""}`,
    )
  }
  return version
}

function findVersion(file, contents, pattern, multiple = false) {
  const flags = pattern.flags.includes("g") ? pattern.flags : `${pattern.flags}g`
  const matches = [...contents.matchAll(new RegExp(pattern.source, flags))]
  if ((!multiple && matches.length !== 1) || (multiple && matches.length === 0)) {
    const expected = multiple ? "至少一个" : "且只允许一个"
    throw new Error(`${file}: 期望${expected}项目版本字段，实际找到 ${matches.length} 个`)
  }

  const versions = new Set(
    matches.map((match) => validateProjectVersion(match[2], `${file} 中的版本号`)),
  )
  if (versions.size !== 1) {
    throw new Error(`${file}: 同一文件内的项目版本不一致: ${[...versions].join(" / ")}`)
  }
  return matches[0][2]
}

/**
 * 一次性读取并验证所有版本文件。调用方应在产生任何写入前完成本步骤，避免文件结构漂移时部分更新。
 */
export function loadProjectVersionState(root = PROJECT_ROOT) {
  return VERSION_FILES.map((definition) => {
    const absolutePath = path.join(root, definition.path)
    const contents = readFileSync(absolutePath, "utf8")
    const version = findVersion(definition.path, contents, definition.pattern, definition.multiple)
    return { ...definition, absolutePath, contents, version }
  })
}

/** 校验全部原生发布入口版本完全一致；传 expected 时同时校验发版目标版本。 */
export function assertProjectVersions(entries, expected) {
  if (!entries.length) throw new Error("没有配置项目版本文件")

  const versions = new Set(entries.map((entry) => entry.version))
  if (versions.size !== 1) {
    const details = entries.map((entry) => `${entry.path}=${entry.version}`).join(" / ")
    throw new Error(`项目版本不一致: ${details}`)
  }

  const actual = entries[0].version
  if (expected !== undefined) {
    validateProjectVersion(expected, "期望版本号")
    if (actual !== expected) {
      throw new Error(`项目版本 ${actual} 与期望版本 ${expected} 不一致`)
    }
  }
  return actual
}

/** 在内存中生成全部更新内容；这里成功后调用方才可以开始落盘。 */
export function prepareProjectVersionUpdate(entries, nextVersion) {
  validateProjectVersion(nextVersion, "目标版本号")
  return entries.map((entry) => {
    const pattern = entry.multiple
      ? new RegExp(
          entry.pattern.source,
          entry.pattern.flags.includes("g") ? entry.pattern.flags : `${entry.pattern.flags}g`,
        )
      : entry.pattern
    const contents = entry.contents.replace(
      pattern,
      (_match, prefix, _current, suffix) => `${prefix}${nextVersion}${suffix}`,
    )
    return { ...entry, nextVersion, nextContents: contents }
  })
}
