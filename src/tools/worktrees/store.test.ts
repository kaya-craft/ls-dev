import { beforeEach, describe, expect, it } from 'vitest'
import { applyInitialWorktrees, applyWorktrees, resetWorktrees, setSort, setWorktreesError, servicesInPath, useWorktrees } from './store'
import { applySnapshot, resetServices } from '../services/store/services'
import { proc, snapshot } from '../../test/fixtures'
import type { RepoGroup, WorktreeInfo, WorktreesSnapshot } from './types'

function wt(overrides: Partial<WorktreeInfo> = {}): WorktreeInfo {
  return {
    path: '/x/wt',
    branch: 'feat/a',
    lastCommitAt: 1000,
    changes: 0,
    upstream: { kind: 'none' },
    merged: true,
    trusted: true,
    sizeBytes: 100,
    ...overrides,
  }
}

function repo(name: string, worktrees: WorktreeInfo[]): RepoGroup {
  return { path: `/r/${name}`, name, mainSize: null, worktrees }
}

function snap(repos: RepoGroup[]): WorktreesSnapshot {
  return { takenAt: 1, scanning: false, repos }
}

function withServices(cwds: string[]) {
  applySnapshot({ ...snapshot(), processes: cwds.map((cwd, i) => proc({ id: `proc:${i}:${cwd}`, cwd })) })
}

describe('worktrees store', () => {
  beforeEach(() => {
    resetWorktrees()
    resetServices()
  })

  it('sorts by size desc with unknown sizes last, and groups by their largest worktree', () => {
    applyWorktrees(
      snap([
        repo('small', [wt({ path: '/s/1', sizeBytes: 10 })]),
        repo('big', [
          wt({ path: '/b/unknown', sizeBytes: null }),
          wt({ path: '/b/1', sizeBytes: 50 }),
          wt({ path: '/b/2', sizeBytes: 900 }),
        ]),
      ]),
    )
    const { groups } = useWorktrees()
    expect(groups.value.map((g) => g.name)).toEqual(['big', 'small'])
    expect(groups.value[0].worktrees.map((w) => w.path)).toEqual(['/b/2', '/b/1', '/b/unknown'])
  })

  it('sorts by last commit desc when asked, unknown dates last', () => {
    applyWorktrees(
      snap([
        repo('a', [
          wt({ path: '/a/old', lastCommitAt: 10 }),
          wt({ path: '/a/none', lastCommitAt: null }),
          wt({ path: '/a/new', lastCommitAt: 99 }),
        ]),
        repo('b', [wt({ path: '/b/mid', lastCommitAt: 50 })]),
      ]),
    )
    setSort('date')
    const { groups } = useWorktrees()
    expect(groups.value.map((g) => g.name)).toEqual(['a', 'b'])
    expect(groups.value[0].worktrees.map((w) => w.path)).toEqual(['/a/new', '/a/old', '/a/none'])
  })

  it('summarises count, total size and worktrees without a service', () => {
    applyWorktrees(
      snap([repo('r', [wt({ path: '/x/wt', sizeBytes: 100 }), wt({ path: '/x/wt-2', sizeBytes: null }), wt({ path: '/x/other', sizeBytes: 5 })])]),
    )
    withServices(['/x/wt/packages/app'])
    expect(useWorktrees().summary.value).toEqual({ count: 3, totalBytes: 105, withoutService: 2 })
  })

  it('matches services on a path boundary: /x/wt does not own /x/wt-2', () => {
    const processes = [proc({ id: 'a', cwd: '/x/wt' }), proc({ id: 'b', cwd: '/x/wt-2' }), proc({ id: 'c', cwd: '/x/wt/sub' }), proc({ id: 'd', cwd: null })]
    expect(servicesInPath('/x/wt', processes).map((p) => p.id)).toEqual(['a', 'c'])
    expect(servicesInPath('/x/wt/', processes).map((p) => p.id)).toEqual(['a', 'c'])
    applyWorktrees(snap([repo('r', [wt({ path: '/x/wt' })])]))
    withServices(['/x/wt-2'])
    expect(useWorktrees().servicesIn('/x/wt')).toEqual([])
  })

  it('is eligible only when trusted, merged, clean, not ahead and without service', () => {
    applyWorktrees(
      snap([
        repo('r', [
          wt({ path: '/x/ok' }),
          wt({ path: '/x/running' }),
          wt({ path: '/x/dirty', changes: 2 }),
          wt({ path: '/x/unknown', changes: null }),
          wt({ path: '/x/ahead', upstream: { kind: 'tracking', ahead: 1 } }),
          wt({ path: '/x/gone', upstream: { kind: 'gone' } }),
          wt({ path: '/x/unmerged', merged: false }),
          wt({ path: '/x/untrusted', trusted: false }),
        ]),
      ]),
    )
    withServices(['/x/running'])
    expect(useWorktrees().eligibleMerged.value.map((w) => w.path).sort()).toEqual(['/x/gone', '/x/ok'])
  })

  it('keeps a stable order on ties by path, and tolerates a repository without worktree', () => {
    applyWorktrees(
      snap([
        { ...repo('empty', []), path: '/r/empty' },
        { ...repo('zeta', [wt({ path: '/z/2', sizeBytes: 5 }), wt({ path: '/z/1', sizeBytes: 5 })]), path: '/r/zeta' },
        { ...repo('alpha', [wt({ path: '/a/1', sizeBytes: 5 })]), path: '/r/alpha' },
      ]),
    )
    const { groups } = useWorktrees()
    expect(groups.value.map((g) => g.name)).toEqual(['alpha', 'zeta', 'empty'])
    expect(groups.value[1].worktrees.map((w) => w.path)).toEqual(['/z/1', '/z/2'])
  })

  it('ignores an initial snapshot older than the current one', () => {
    applyWorktrees({ ...snap([repo('new', [])]), takenAt: 10 })
    applyInitialWorktrees({ ...snap([repo('old', [])]), takenAt: 5 })
    expect(useWorktrees().state.snapshot?.repos[0].name).toBe('new')
    applyInitialWorktrees({ ...snap([repo('newer', [])]), takenAt: 11 })
    expect(useWorktrees().state.snapshot?.repos[0].name).toBe('newer')
  })

  it('keeps the event snapshot when the initial one has the same takenAt', () => {
    applyWorktrees({ ...snap([repo('event', [])]), takenAt: 10 })
    applyInitialWorktrees({ ...snap([repo('initial', [])]), takenAt: 10 })
    expect(useWorktrees().state.snapshot?.repos[0].name).toBe('event')
  })

  it('keeps the error until the next snapshot', () => {
    const { state } = useWorktrees()
    setWorktreesError('boom')
    expect(state.error).toBe('boom')
    applyWorktrees(snap([]))
    expect(state.error).toBeNull()
  })
})
