import { beforeEach, describe, expect, it, vi } from 'vitest'
import { addFolder, excludeRepo, removeAllMerged, removeWorktree } from './actions'
import { worktreesApi } from './api'
import { confirm } from '../../ui/store/confirm'
import { toasts } from '../../ui/store/toasts'
import { updateSettings, useSettings } from '../../ui/store/settings'
import { open } from '@tauri-apps/plugin-dialog'
import { applyWorktrees, resetWorktrees, useWorktrees } from './store'
import { applySnapshot, resetServices } from '../services/store/services'
import { proc, snapshot } from '../../test/fixtures'
import type { WorktreeInfo } from './types'

vi.mock('./api', () => ({
  worktreesApi: { scan: vi.fn(), removeMerged: vi.fn(), removeWorktree: vi.fn(), openTerminal: vi.fn(), openShell: vi.fn() },
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }))
vi.mock('../../ui/store/confirm', () => ({ confirm: vi.fn() }))
vi.mock('../../ui/store/toasts', () => ({ toasts: { success: vi.fn(), error: vi.fn() } }))
vi.mock('../../ui/store/settings', () => ({ updateSettings: vi.fn(), useSettings: vi.fn() }))

const confirmMock = vi.mocked(confirm)
const MB = 1024 * 1024

function info(overrides: Partial<WorktreeInfo> = {}): WorktreeInfo {
  return {
    path: '/w/a',
    branch: 'feat/a',
    lastCommitAt: 1,
    changes: 0,
    upstream: { kind: 'none' },
    merged: true,
    trusted: true,
    sizeBytes: 10 * MB,
    ...overrides,
  }
}

function load(worktrees: WorktreeInfo[]) {
  applyWorktrees({ takenAt: 1, scanning: false, repos: [{ path: '/w/repo', name: 'repo', mainSize: null, worktrees }] })
}

const IGNORED_FILES_NOTICE = 'Ignored files in the worktree (.env, node_modules, build output) are deleted.'

describe('removeWorktree', () => {
  const target = { branch: 'feat/login', path: '/w/shop-feat', mainPath: '/w/shop' }
  const running = [proc({ project: 'shop-feat/app' })]

  beforeEach(() => vi.clearAllMocks())

  it('asks a second confirmation when Git needs --force, then rescans', async () => {
    confirmMock.mockResolvedValue({ ok: true, checked: false })
    vi.mocked(worktreesApi.removeWorktree)
      .mockRejectedValueOnce("needs-force:fatal: '/w/shop-feat' contains modified or untracked files")
      .mockResolvedValueOnce()
    await removeWorktree(target, running)
    expect(confirmMock).toHaveBeenCalledTimes(2)
    expect(confirmMock.mock.calls[0][0].details).toContain('  shop-feat/app')
    expect(confirmMock.mock.calls[0][0].body).toContain(IGNORED_FILES_NOTICE)
    expect(confirmMock.mock.calls[1][0].body).toContain('modified or untracked')
    expect(worktreesApi.removeWorktree).toHaveBeenNthCalledWith(1, target, false)
    expect(worktreesApi.removeWorktree).toHaveBeenLastCalledWith(target, true)
    expect(toasts.success).toHaveBeenCalled()
    expect(worktreesApi.scan).toHaveBeenCalled()
  })

  it('marks the worktree as removing until the rescan ends, then clears it', async () => {
    confirmMock.mockResolvedValue({ ok: true, checked: false })
    let during: string[] = []
    vi.mocked(worktreesApi.removeWorktree).mockResolvedValue()
    vi.mocked(worktreesApi.scan).mockImplementation(async () => {
      during = [...useWorktrees().state.removing]
    })
    await removeWorktree(target, [])
    expect(during).toEqual(['/w/shop-feat'])
    expect(useWorktrees().state.removing).toEqual([])
  })

  it('clears the removing mark when the removal fails', async () => {
    confirmMock.mockResolvedValue({ ok: true, checked: false })
    vi.mocked(worktreesApi.removeWorktree).mockRejectedValue('fatal: boom')
    await removeWorktree(target, [])
    expect(useWorktrees().state.removing).toEqual([])
  })

  it('stops after the first refusal', async () => {
    confirmMock.mockResolvedValue({ ok: false, checked: false })
    await removeWorktree(target, [])
    expect(worktreesApi.removeWorktree).not.toHaveBeenCalled()
  })

  it('shows any other backend refusal as a toast, with no force path', async () => {
    confirmMock.mockResolvedValue({ ok: true, checked: false })
    vi.mocked(worktreesApi.removeWorktree).mockRejectedValue('fatal: not a working tree')
    await removeWorktree(target, [])
    expect(confirmMock).toHaveBeenCalledTimes(1)
    expect(toasts.error).toHaveBeenCalledWith('fatal: not a working tree')
    expect(worktreesApi.removeWorktree).toHaveBeenCalledTimes(1)
  })

  it('offers to open a terminal for an untrusted repository, never forcing', async () => {
    confirmMock.mockResolvedValue({ ok: true, checked: false })
    vi.mocked(worktreesApi.removeWorktree).mockRejectedValue('untrusted:This repository can run programs.')
    await removeWorktree(target, [])
    expect(confirmMock).toHaveBeenCalledTimes(2)
    const offer = confirmMock.mock.calls[1][0]
    expect(offer.confirmLabel).toBe('Open terminal')
    expect(offer.danger).toBeFalsy()
    expect(offer.body).toContain('This repository can run programs.')
    expect(offer.details).toEqual(["git worktree remove '/w/shop-feat'"])
    expect(worktreesApi.openTerminal).toHaveBeenCalledWith(target)
    expect(worktreesApi.removeWorktree).toHaveBeenCalledTimes(1)
  })

  it('opens no terminal when the offer is declined', async () => {
    confirmMock.mockResolvedValueOnce({ ok: true, checked: false }).mockResolvedValueOnce({ ok: false, checked: false })
    vi.mocked(worktreesApi.removeWorktree).mockRejectedValue('untrusted:x')
    await removeWorktree(target, [])
    expect(worktreesApi.openTerminal).not.toHaveBeenCalled()
  })
})

describe('removeAllMerged', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    resetWorktrees()
    resetServices()
  })

  it('lists branch, path and size of each eligible worktree and the total freed', async () => {
    load([info({ path: '/w/a', branch: 'feat/a', sizeBytes: 10 * MB }), info({ path: '/w/b', branch: 'feat/b', sizeBytes: 30 * MB }), info({ path: '/w/c', merged: false })])
    confirmMock.mockResolvedValue({ ok: false, checked: false })
    await removeAllMerged()
    const req = confirmMock.mock.calls[0][0]
    expect(req.body).toContain('40 MB')
    expect(req.body).toContain(IGNORED_FILES_NOTICE)
    expect(req.body).not.toContain('unknown sizes')
    expect(req.details).toEqual(['feat/b  /w/b  30 MB', 'feat/a  /w/a  10 MB'])
    expect(worktreesApi.removeMerged).not.toHaveBeenCalled()
  })

  it('flags unknown sizes in the freed total', async () => {
    load([info({ path: '/w/a', sizeBytes: 10 * MB }), info({ path: '/w/b', sizeBytes: null })])
    confirmMock.mockResolvedValue({ ok: false, checked: false })
    await removeAllMerged()
    expect(confirmMock.mock.calls[0][0].body).toContain('10 MB (+ unknown sizes)')
  })

  it('uses the singular for a single worktree', async () => {
    load([info({ path: '/w/a' })])
    confirmMock.mockResolvedValue({ ok: true, checked: false })
    vi.mocked(worktreesApi.removeMerged).mockResolvedValue({ removed: ['/w/a'], skipped: [] })
    await removeAllMerged()
    expect(confirmMock.mock.calls[0][0].title).toBe('Remove 1 merged worktree?')
    expect(toasts.success).toHaveBeenCalledWith('Removed 1 worktree')
    expect(toasts.error).not.toHaveBeenCalled()
  })

  it('does nothing when no worktree is eligible', async () => {
    load([info({ merged: false })])
    await removeAllMerged()
    expect(confirmMock).not.toHaveBeenCalled()
    expect(toasts.success).toHaveBeenCalledWith('No merged worktree to remove')
  })

  it('does not offer a worktree with a running service', async () => {
    load([info({ path: '/w/a' })])
    applySnapshot({ ...snapshot(), processes: [proc({ cwd: '/w/a/app' })] })
    await removeAllMerged()
    expect(confirmMock).not.toHaveBeenCalled()
  })

  it('removes exactly the eligible paths, reports skipped worktrees in the success toast and rescans', async () => {
    load([info({ path: '/w/a' }), info({ path: '/w/b' }), info({ path: '/w/c', merged: false })])
    confirmMock.mockResolvedValue({ ok: true, checked: false })
    vi.mocked(worktreesApi.removeMerged).mockResolvedValue({
      removed: ['/w/a'],
      skipped: [{ path: '/w/b', reason: 'has local changes' }],
    })
    await removeAllMerged()
    expect(worktreesApi.removeMerged).toHaveBeenCalledWith(['/w/a', '/w/b'])
    expect(toasts.success).toHaveBeenCalledWith('Removed 1 worktree\nSkipped 1:\n/w/b: has local changes')
    expect(toasts.error).not.toHaveBeenCalled()
    expect(worktreesApi.scan).toHaveBeenCalled()
  })

  it('shows an error toast only when nothing was removed, one skipped worktree per line, capped at 5', async () => {
    load([info({ path: '/w/a' })])
    confirmMock.mockResolvedValue({ ok: true, checked: false })
    const skipped = Array.from({ length: 7 }, (_, i) => ({ path: `/w/${i}`, reason: 'busy' }))
    vi.mocked(worktreesApi.removeMerged).mockResolvedValue({ removed: [], skipped })
    await removeAllMerged()
    expect(toasts.success).not.toHaveBeenCalled()
    expect(toasts.error).toHaveBeenCalledWith(
      'No worktree removed\nSkipped 7:\n/w/0: busy\n/w/1: busy\n/w/2: busy\n/w/3: busy\n/w/4: busy\n+2 more',
    )
  })

  it('surfaces a backend failure', async () => {
    load([info()])
    confirmMock.mockResolvedValue({ ok: true, checked: false })
    vi.mocked(worktreesApi.removeMerged).mockRejectedValue('boom')
    await removeAllMerged()
    expect(toasts.error).toHaveBeenCalledWith('boom')
  })
})

describe('addFolder and excludeRepo', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(useSettings).mockReturnValue({ settings: { worktreeRoots: ['/a'], worktreeExcludes: ['/e'] } } as ReturnType<typeof useSettings>)
  })

  it('does nothing when the folder picker is cancelled', async () => {
    vi.mocked(open).mockResolvedValue(null)
    await addFolder()
    expect(open).toHaveBeenCalledWith({ directory: true })
    expect(updateSettings).not.toHaveBeenCalled()
    expect(worktreesApi.scan).not.toHaveBeenCalled()
  })

  it('appends the chosen folder to the roots and rescans', async () => {
    vi.mocked(open).mockResolvedValue('/b')
    await addFolder()
    expect(updateSettings).toHaveBeenCalledWith({ worktreeRoots: ['/a', '/b'] })
    expect(worktreesApi.scan).toHaveBeenCalled()
  })

  it('does not add a folder twice', async () => {
    vi.mocked(open).mockResolvedValue('/a')
    await addFolder()
    expect(updateSettings).not.toHaveBeenCalled()
  })

  it('appends the repository to the excludes and rescans', async () => {
    await excludeRepo('/r/x')
    expect(updateSettings).toHaveBeenCalledWith({ worktreeExcludes: ['/e', '/r/x'] })
    expect(worktreesApi.scan).toHaveBeenCalled()
  })
})
