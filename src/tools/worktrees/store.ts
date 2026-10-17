import { computed, reactive } from 'vue'
import { useServices } from '../services/store/services'
import type { ProcessService } from '../services/types'
import type { RepoGroup, WorktreeInfo, WorktreesSnapshot } from './types'

export type WorktreeSort = 'size' | 'date'

const state = reactive({
  snapshot: null as WorktreesSnapshot | null,
  error: null as string | null,
  sort: 'size' as WorktreeSort,
  /** Paths whose removal is in progress (single or bulk), until the rescan that follows. */
  removing: [] as string[],
})

export function applyWorktrees(snapshot: WorktreesSnapshot) {
  state.snapshot = snapshot
  state.error = null
}

/** Applies the startup snapshot unless the event already delivered one as recent (a tie keeps the event). */
export function applyInitialWorktrees(snapshot: WorktreesSnapshot) {
  if (state.snapshot && state.snapshot.takenAt >= snapshot.takenAt) return
  applyWorktrees(snapshot)
}

export function setWorktreesError(message: string) {
  state.error = message
}

export function setSort(sort: WorktreeSort) {
  state.sort = sort
}

export function setRemoving(paths: string[], on: boolean) {
  state.removing = on
    ? [...new Set([...state.removing, ...paths])]
    : state.removing.filter((p) => !paths.includes(p))
}

export function resetWorktrees() {
  state.snapshot = null
  state.error = null
  state.sort = 'size'
  state.removing = []
}

/** Services whose cwd is the worktree or below it, on a path boundary (/x/wt does not own /x/wt-2). */
export function servicesInPath(path: string, processes: ProcessService[]): ProcessService[] {
  const root = path.replace(/\/+$/, '')
  const prefix = `${root}/`
  return processes.filter((p) => p.cwd !== null && (p.cwd === root || p.cwd.startsWith(prefix)))
}

/** Same rule as Rust `eligible` (src-tauri/src/tools/worktrees/scan.rs `skip_reason`). */
export function isEligible(info: WorktreeInfo, hasService: boolean): boolean {
  return (
    info.trusted &&
    info.changes === 0 &&
    !(info.upstream.kind === 'tracking' && info.upstream.ahead > 0) &&
    info.merged &&
    !hasService
  )
}

/** Descending, unknown (null) last. */
function descNullLast(a: number | null, b: number | null): number {
  if (a === null && b === null) return 0
  if (a === null) return 1
  if (b === null) return -1
  return b - a
}

const key = (w: WorktreeInfo, sort: WorktreeSort) => (sort === 'size' ? w.sizeBytes : w.lastCommitAt)

const processes = computed<ProcessService[]>(() => useServices().state.snapshot?.processes ?? [])

const byPath = (a: { path: string }, b: { path: string }) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0)

const groups = computed<RepoGroup[]>(() => {
  const sort = state.sort
  const sorted = (state.snapshot?.repos ?? []).map((r) => ({
    ...r,
    worktrees: [...r.worktrees].sort((a, b) => descNullLast(key(a, sort), key(b, sort)) || byPath(a, b)),
  }))
  const lead = (g: RepoGroup) => (g.worktrees.length > 0 ? key(g.worktrees[0], sort) : null)
  return sorted.sort((a, b) => descNullLast(lead(a), lead(b)) || byPath(a, b))
})

const all = computed<WorktreeInfo[]>(() => groups.value.flatMap((g) => g.worktrees))

const summary = computed(() => ({
  count: all.value.length,
  totalBytes: all.value.reduce((n, w) => n + (w.sizeBytes ?? 0), 0),
  withoutService: all.value.filter((w) => servicesInPath(w.path, processes.value).length === 0).length,
}))

const eligibleMerged = computed<WorktreeInfo[]>(() =>
  all.value.filter((w) => isEligible(w, servicesInPath(w.path, processes.value).length > 0)),
)

export function useWorktrees() {
  return {
    state,
    groups,
    summary,
    eligibleMerged,
    servicesIn: (path: string) => servicesInPath(path, processes.value),
  }
}
