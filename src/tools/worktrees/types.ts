// Mirrors src-tauri/src/tools/worktrees (serde camelCase).
export type UpstreamState = { kind: 'none' } | { kind: 'gone' } | { kind: 'tracking'; ahead: number }

export type WorktreeInfo = {
  path: string
  branch: string | null
  lastCommitAt: number | null
  /** null: git status failed, the state is unknown. */
  changes: number | null
  upstream: UpstreamState
  merged: boolean
  trusted: boolean
  sizeBytes: number | null
}

export type RepoGroup = {
  path: string
  name: string
  mainSize: number | null
  worktrees: WorktreeInfo[]
}

export type WorktreesSnapshot = {
  takenAt: number
  scanning: boolean
  repos: RepoGroup[]
}

export type RemoveMergedReport = {
  removed: string[]
  skipped: { path: string; reason: string }[]
}
