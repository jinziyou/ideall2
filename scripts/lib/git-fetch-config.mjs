/**
 * CI 会移动 app-edge 等发布标签; 在 remote.origin.fetch 里用 + 前缀允许非 fast-forward 更新,
 * 这样 git fetch / git pull (含 Cursor/VS Code) 不再因「本地标签冲突」弹窗。
 */
import { execFileSync } from "node:child_process"

/** 会被 CI 覆盖重打的远程标签 → 本地 fetch refspec (带 + 强制更新)。 */
export const MOVING_TAG_REFSPECS = ["+refs/tags/app-edge:refs/tags/app-edge"]

export function ensureMovingTagFetchRefspecs(cwd) {
  let existing = []
  try {
    existing = execFileSync("git", ["config", "--local", "--get-all", "remote.origin.fetch"], {
      cwd,
      encoding: "utf8",
    })
      .trim()
      .split("\n")
      .filter(Boolean)
  } catch {
    // 无 remote.origin.fetch 时由 git 使用默认 refspec
  }

  let added = 0
  for (const spec of MOVING_TAG_REFSPECS) {
    if (existing.includes(spec)) continue
    execFileSync("git", ["config", "--local", "--add", "remote.origin.fetch", spec], { cwd })
    existing.push(spec)
    added++
  }
  return added
}
