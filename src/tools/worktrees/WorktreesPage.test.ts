import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import WorktreesPage from './WorktreesPage.vue'
import { worktreesApi } from './api'
import { toasts } from '../../ui/store/toasts'
import { addFolder, excludeRepo, removeAllMerged, removeWorktree } from './actions'
import { applyWorktrees, resetWorktrees, setRemoving, setWorktreesError, useWorktrees } from './store'
import { applySnapshot, resetServices, setFilter, useServices } from '../services/store/services'
import { resetNavigation, setServicesSection, useNavigation } from '../../shell/navigation'
import { proc, snapshot } from '../../test/fixtures'
import type { RepoGroup, WorktreeInfo, WorktreesSnapshot } from './types'

vi.mock('../../ui/store/toasts', () => ({ toasts: { success: vi.fn(), error: vi.fn() } }))
vi.mock('./api', () => ({ worktreesApi: { scan: vi.fn(), removeMerged: vi.fn(), removeWorktree: vi.fn(), openShell: vi.fn() } }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }))
vi.mock('./actions', async (original) => ({
  ...(await original<typeof import('./actions')>()),
  addFolder: vi.fn(),
  excludeRepo: vi.fn(),
  removeAllMerged: vi.fn(),
  removeWorktree: vi.fn(),
}))

const NOW = 1_800_000_000
const GB = 1024 ** 3
const DAY = 86400

function wt(overrides: Partial<WorktreeInfo> = {}): WorktreeInfo {
  return {
    path: '/w/repo-a',
    branch: 'feat/a',
    lastCommitAt: NOW - 3 * DAY,
    changes: 0,
    upstream: { kind: 'tracking', ahead: 0 },
    merged: false,
    trusted: true,
    sizeBytes: 2 * GB,
    ...overrides,
  }
}

function repo(name: string, worktrees: WorktreeInfo[], mainSize: number | null = 5 * GB): RepoGroup {
  return { path: `/r/${name}`, name, mainSize, worktrees }
}

function load(repos: RepoGroup[], scanning = false) {
  const snap: WorktreesSnapshot = { takenAt: 1, scanning, repos }
  applyWorktrees(snap)
}

let wrapper: VueWrapper | null = null
function mountPage() {
  wrapper = mount(WorktreesPage)
  return wrapper
}
const buttonByText = (w: VueWrapper, text: string) => w.findAll('button').find((b) => b.text().startsWith(text))

describe('WorktreesPage', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(NOW * 1000)
    vi.clearAllMocks()
    vi.mocked(worktreesApi.scan).mockResolvedValue()
    resetWorktrees()
    resetServices()
    resetNavigation()
  })
  afterEach(() => {
    wrapper?.unmount()
    wrapper = null
    vi.useRealTimers()
  })

  it('scans on mount and every 60 s, and stops on unmount', async () => {
    load([])
    const w = mountPage()
    expect(worktreesApi.scan).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(60_000)
    expect(worktreesApi.scan).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(60_000)
    expect(worktreesApi.scan).toHaveBeenCalledTimes(3)
    w.unmount()
    wrapper = null
    await vi.advanceTimersByTimeAsync(180_000)
    expect(worktreesApi.scan).toHaveBeenCalledTimes(3)
  })

  it('renders the header with its actions', () => {
    load([repo('a', [wt()])])
    const w = mountPage()
    expect(w.find('h1').text()).toBe('Worktrees')
    expect(w.text()).toContain('Git worktrees across your local repositories')
    expect(buttonByText(w, 'Rescan')).toBeTruthy()
    expect(buttonByText(w, 'Add folder')).toBeTruthy()
    expect(w.find('[aria-label="Settings"]').exists()).toBe(true)
  })

  it('forwards the Settings button as open-settings', async () => {
    load([])
    const w = mountPage()
    await w.find('[aria-label="Settings"]').trigger('click')
    expect(w.emitted('open-settings')).toHaveLength(1)
  })

  it('disables Rescan while scanning and calls scan otherwise', async () => {
    load([repo('a', [wt()])], true)
    const w = mountPage()
    expect(buttonByText(w, 'Rescan')!.attributes('disabled')).toBeDefined()
    load([repo('a', [wt()])], false)
    await w.vm.$nextTick()
    vi.mocked(worktreesApi.scan).mockClear()
    await buttonByText(w, 'Rescan')!.trigger('click')
    expect(worktreesApi.scan).toHaveBeenCalledTimes(1)
  })

  it('shows the eligible count on Remove all merged and disables it when there is none', async () => {
    load([repo('a', [wt({ path: '/w/1', merged: true }), wt({ path: '/w/2', merged: true }), wt({ path: '/w/3' })])])
    const w = mountPage()
    const btn = buttonByText(w, 'Remove all merged')!
    expect(btn.text()).toBe('Remove all merged (2)')
    expect(btn.attributes('disabled')).toBeUndefined()
    await btn.trigger('click')
    expect(removeAllMerged).toHaveBeenCalled()
    load([repo('a', [wt({ merged: false })])])
    await w.vm.$nextTick()
    expect(buttonByText(w, 'Remove all merged')!.attributes('disabled')).toBeDefined()
  })

  it('shows the summary line', () => {
    load([repo('a', [wt({ path: '/w/1', sizeBytes: 3 * GB }), wt({ path: '/w/2', sizeBytes: 1 * GB })])])
    applySnapshot({ ...snapshot(), processes: [proc({ cwd: '/w/1/app' })] })
    expect(mountPage().find('[data-testid="summary"]').text()).toBe('2 worktrees · 4.0 GB · 1 without running service')
  })

  it('renders one group per repository with name, path, main size and an exclude menu', async () => {
    load([repo('alpha', [wt({ path: '/w/a1' })], 5 * GB), repo('beta', [wt({ path: '/w/b1', sizeBytes: 1 * GB })], null)])
    const w = mountPage()
    const groups = w.findAll('[data-testid="repo-group"]')
    expect(groups).toHaveLength(2)
    expect(groups[0].text()).toContain('alpha')
    expect(groups[0].text()).toContain('/r/alpha')
    expect(groups[0].text()).toContain('5.0 GB')
    expect(groups[1].text()).toContain('…')
    await groups[0].find('[aria-label="Menu alpha"]').trigger('click')
    const item = w.findAll('button').find((b) => b.text() === 'Exclude repository')!
    await item.trigger('click')
    expect(excludeRepo).toHaveBeenCalledWith('/r/alpha')
  })

  describe('repository menu dismissal', () => {
    async function openMenu() {
      load([repo('alpha', [wt({ path: '/w/a1' })])])
      const w = mount(WorktreesPage, { attachTo: document.body })
      wrapper = w
      await w.find('[aria-label="Menu alpha"]').trigger('click')
      expect(w.text()).toContain('Exclude repository')
      return w
    }

    it('closes on Escape without reaching the global Esc handler', async () => {
      const w = await openMenu()
      const reached = vi.fn()
      window.addEventListener('keydown', reached)
      document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
      await w.vm.$nextTick()
      expect(w.text()).not.toContain('Exclude repository')
      expect(reached).not.toHaveBeenCalled()
      // Menu closed: Esc passes through again.
      document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
      expect(reached).toHaveBeenCalledTimes(1)
      window.removeEventListener('keydown', reached)
    })

    it('closes on an outside click and stays open on a click inside', async () => {
      const w = await openMenu()
      await w.find('[aria-label="Menu alpha"]').element.parentElement!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await w.vm.$nextTick()
      expect(w.text()).toContain('Exclude repository')
      document.body.click()
      await w.vm.$nextTick()
      expect(w.text()).not.toContain('Exclude repository')
    })
  })

  it('renders a row with branch, path, size, relative date and state badges', () => {
    load([
      repo('a', [
        wt({ changes: 3, upstream: { kind: 'tracking', ahead: 2 }, merged: true }),
        wt({ path: '/w/none', branch: 'feat/b', upstream: { kind: 'none' }, sizeBytes: null, lastCommitAt: NOW - DAY }),
        wt({ path: '/w/gone', branch: 'feat/c', upstream: { kind: 'gone' }, sizeBytes: 1 * GB, lastCommitAt: NOW - 400 * DAY }),
      ]),
    ])
    const rows = mountPage().findAll('[data-testid="worktree-row"]')
    expect(rows).toHaveLength(3)
    const first = rows[0].text()
    expect(first).toContain('feat/a')
    expect(first).toContain('/w/repo-a')
    expect(first).toContain('2.0 GB')
    expect(first).toContain('3 days ago')
    expect(first).toContain('3 changes')
    expect(first).toContain('2 unpushed')
    expect(first).toContain('merged')
    const none = rows.find((r) => r.text().includes('feat/b'))!.text()
    expect(none).toContain('no upstream')
    expect(none).toContain('…')
    expect(none).toContain('1 day ago')
    expect(rows.find((r) => r.text().includes('feat/c'))!.text()).toContain('upstream gone')
  })

  it('shows detached worktrees without branch-derived badges', () => {
    load([repo('a', [wt({ branch: null, upstream: { kind: 'none' } })])])
    const row = mountPage().find('[data-testid="worktree-row"]').text()
    expect(row).toContain('detached')
    expect(row).not.toContain('no upstream')
  })

  it('flags an untrusted worktree and an unknown state', () => {
    load([
      repo('a', [
        wt({ path: '/w/u', branch: 'feat/u', trusted: false, changes: null }),
        wt({ path: '/w/k', branch: 'feat/k', changes: null }),
      ]),
    ])
    const rows = mountPage().findAll('[data-testid="worktree-row"]')
    const untrusted = rows.find((r) => r.text().includes('feat/u'))!
    expect(untrusted.text()).toContain('untrusted config')
    expect(untrusted.text()).not.toContain('state unknown')
    expect(untrusted.find('[data-testid="untrusted-badge"]').attributes('title')).toContain('did not read')
    expect(rows.find((r) => r.text().includes('feat/k'))!.text()).toContain('state unknown')
  })

  it('shows running services as chips that open them in Services', async () => {
    load([repo('a', [wt({ path: '/w/repo-a' })])])
    applySnapshot({
      ...snapshot(),
      processes: [proc({ id: 'proc:7', cwd: '/w/repo-a/packages/api', project: 'repo-a/packages/api' }), proc({ id: 'proc:8', cwd: '/w/repo-a-2' })],
    })
    const w = mountPage()
    const chips = w.findAll('[data-testid="service-chip"]')
    expect(chips).toHaveLength(1)
    expect(chips[0].text()).toContain('api')
    await chips[0].trigger('click')
    expect(useNavigation().state.currentTool).toBe('services')
    expect(useServices().state.selectedId).toBe('proc:7')
  })

  it('resets the Services section and filter so the selected service is visible', async () => {
    load([repo('a', [wt({ path: '/w/repo-a' })])])
    applySnapshot({ ...snapshot(), processes: [proc({ id: 'proc:7', cwd: '/w/repo-a/api', project: 'repo-a/api' })] })
    setServicesSection('standalone')
    setFilter('postgres')
    await mountPage().find('[data-testid="service-chip"]').trigger('click')
    expect(useNavigation().state.servicesSection).toBe('all')
    expect(useServices().state.filter).toBe('')
    expect(useServices().state.selectedId).toBe('proc:7')
  })

  it('keeps interval scan failures silent (store error) while the Rescan button toasts', async () => {
    load([repo('a', [wt()])])
    vi.mocked(worktreesApi.scan).mockRejectedValue('scan boom')
    const w = mountPage()
    await vi.advanceTimersByTimeAsync(60_000)
    expect(toasts.error).not.toHaveBeenCalled()
    expect(useWorktrees().state.error).toBe('scan boom')
    await buttonByText(w, 'Rescan')!.trigger('click')
    await vi.advanceTimersByTimeAsync(0)
    expect(toasts.error).toHaveBeenCalledWith('scan boom')
  })

  it('shows an error state instead of Scanning… when the first scan failed with no snapshot', async () => {
    vi.mocked(worktreesApi.scan).mockRejectedValue('first boom')
    const w = mountPage()
    await vi.advanceTimersByTimeAsync(0)
    expect(w.text()).toContain('Scan failed')
    expect(w.text()).not.toContain('Scanning…')
    expect(w.text()).not.toContain('No repositories to scan yet')
  })

  it('removes a worktree with its running services', async () => {
    load([repo('a', [wt({ path: '/w/repo-a', branch: 'feat/a' })])])
    applySnapshot({ ...snapshot(), processes: [proc({ cwd: '/w/repo-a/app' })] })
    await mountPage().find('[data-testid="worktree-row"] [data-testid="remove"]').trigger('click')
    expect(removeWorktree).toHaveBeenCalledWith(
      { branch: 'feat/a', path: '/w/repo-a', mainPath: '/r/a' },
      [expect.objectContaining({ cwd: '/w/repo-a/app' })],
    )
  })

  it('opens a terminal in the worktree path', async () => {
    load([repo('a', [wt({ path: '/w/repo-a', branch: 'feat/a' })])])
    await mountPage().find('[data-testid="worktree-row"] [data-testid="open-shell"]').trigger('click')
    expect(worktreesApi.openShell).toHaveBeenCalledWith('/w/repo-a')
  })

  it('replaces the row actions with a spinner while the worktree is being removed', async () => {
    load([repo('a', [wt({ path: '/w/repo-a', branch: 'feat/a' }), wt({ path: '/w/repo-b', branch: 'feat/b' })])])
    const w = mountPage()
    setRemoving(['/w/repo-a'], true)
    await w.vm.$nextTick()
    const rows = w.findAll('[data-testid="worktree-row"]')
    const busy = rows.find((r) => r.attributes('data-path') === '/w/repo-a')!
    const idle = rows.find((r) => r.attributes('data-path') === '/w/repo-b')!
    expect(busy.find('[data-testid="removing"]').text()).toContain('Removing…')
    expect(busy.find('[data-testid="remove"]').exists()).toBe(false)
    expect(idle.find('[data-testid="remove"]').exists()).toBe(true)
    expect(buttonByText(w, 'Remove all merged')!.attributes('disabled')).toBeDefined()
    setRemoving(['/w/repo-a'], false)
    await w.vm.$nextTick()
    expect(w.find('[data-testid="removing"]').exists()).toBe(false)
  })

  it('switches the sort between Size and Last commit', async () => {
    load([repo('a', [wt({ path: '/w/big', branch: 'big', sizeBytes: 9 * GB, lastCommitAt: 1 }), wt({ path: '/w/new', branch: 'new', sizeBytes: 1 * GB, lastCommitAt: NOW })])])
    const w = mountPage()
    const order = () => w.findAll('[data-testid="worktree-row"]').map((r) => r.attributes('data-path'))
    expect(order()).toEqual(['/w/big', '/w/new'])
    await buttonByText(w, 'Last commit')!.trigger('click')
    expect(useWorktrees().state.sort).toBe('date')
    expect(order()).toEqual(['/w/new', '/w/big'])
    await buttonByText(w, 'Size')!.trigger('click')
    expect(order()).toEqual(['/w/big', '/w/new'])
  })

  it('shows Scanning… before the first result', () => {
    const w = mountPage()
    expect(w.text()).toContain('Scanning…')
    expect(w.text()).not.toContain('No repositories to scan yet')
  })

  it('shows the empty state with its ghost columns when the scan found nothing', async () => {
    load([])
    const w = mountPage()
    expect(w.text()).toContain('No repositories to scan yet')
    for (const c of ['Branch', 'Repository', 'Disk size', 'Last modified', 'Running']) expect(w.text()).toContain(c)
    await buttonByText(w, 'Choose a folder')!.trigger('click')
    expect(addFolder).toHaveBeenCalledTimes(1)
    await buttonByText(w, 'Add folder')!.trigger('click')
    expect(addFolder).toHaveBeenCalledTimes(2)
  })

  it('shows Scanning… instead of the empty state while a scan is running', () => {
    load([], true)
    expect(mountPage().text()).toContain('Scanning…')
  })

  it('shows the last scan error', () => {
    load([repo('a', [wt()])])
    setWorktreesError('git exploded')
    expect(mountPage().text()).toContain('git exploded')
  })
})
