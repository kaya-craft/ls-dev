import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import App from './App.vue'
import { resetServices, select, useServices } from './tools/services/store/services'
import { useSettings } from './ui/store/settings'
import { api } from './tools/services/api'
import { coreApi } from './core/api'
import type { Settings } from './core/types'
import { worktreesApi } from './tools/worktrees/api'
import { resetWorktrees, useWorktrees } from './tools/worktrees/store'
import { tunnelsApi } from './tools/tunnels/api'
import { snapshot } from './test/fixtures'
import { resetNavigation, setTool } from './shell/navigation'

vi.mock('./tools/services/api', () => ({
  api: {
    getSnapshot: vi.fn(),
    onSnapshot: vi.fn(),
    onCollectError: vi.fn(),
    onLogChunk: vi.fn(),
    onLogClosed: vi.fn(),
    refresh: vi.fn(),
  },
}))
vi.mock('./tools/worktrees/api', () => ({
  worktreesApi: {
    get: vi.fn(),
    scan: vi.fn(),
    onChanged: vi.fn(),
    onError: vi.fn(),
    removeMerged: vi.fn(),
    removeWorktree: vi.fn(),
  },
}))
vi.mock('./tools/tunnels/api', () => ({
  tunnelsApi: { onChanged: vi.fn(), list: vi.fn(), status: vi.fn() },
}))
vi.mock('./core/api', () => ({
  coreApi: {
    getSettings: vi.fn(),
    setSettings: vi.fn(),
  },
}))

describe('App startup', () => {
  beforeEach(() => {
    resetNavigation()
    resetServices()
    resetWorktrees()
    useSettings().settings = null
    vi.mocked(worktreesApi.onChanged).mockResolvedValue(() => {})
    vi.mocked(worktreesApi.onError).mockResolvedValue(() => {})
    vi.mocked(tunnelsApi.onChanged).mockResolvedValue(() => {})
    vi.mocked(tunnelsApi.list).mockResolvedValue([])
    vi.mocked(tunnelsApi.status).mockResolvedValue({ binary: null, loggedIn: false, domain: null, stable: [] })
    vi.mocked(worktreesApi.get).mockResolvedValue({ takenAt: 1, scanning: false, repos: [] })
    vi.mocked(worktreesApi.scan).mockResolvedValue()
    vi.mocked(api.onSnapshot).mockResolvedValue(() => {})
    vi.mocked(api.onCollectError).mockResolvedValue(() => {})
    vi.mocked(api.onLogChunk).mockResolvedValue(() => {})
    vi.mocked(api.onLogClosed).mockResolvedValue(() => {})
  })

  it('still loads settings and shows the error when the initial snapshot fails', async () => {
    vi.mocked(api.getSnapshot).mockRejectedValue('snapshot boom')
    vi.mocked(coreApi.getSettings).mockResolvedValue({
      pollIntervalMs: 3000, showAll: false, terminal: 'terminal-app',
      editorCommand: 'code', notifyOnStop: true, launchAtLogin: false, theme: 'system', worktreeRoots: [], worktreeExcludes: [],
      tunnelDomain: '', tunnelPrefix: '', tunnelHours: 4,
    })
    const wrapper = mount(App)
    await flushPromises()
    expect(useServices().state.error).toBe('snapshot boom')
    expect(useSettings().settings?.pollIntervalMs).toBe(3000)
    wrapper.unmount()
  })

  it('registers the log listeners and closes the detail pane on Escape', async () => {
    vi.mocked(api.getSnapshot).mockResolvedValue(snapshot())
    vi.mocked(coreApi.getSettings).mockResolvedValue({
      pollIntervalMs: 3000, showAll: false, terminal: 'terminal-app',
      editorCommand: 'code', notifyOnStop: true, launchAtLogin: false, theme: 'system', worktreeRoots: [], worktreeExcludes: [],
      tunnelDomain: '', tunnelPrefix: '', tunnelHours: 4,
    })
    const wrapper = mount(App)
    await flushPromises()
    expect(api.onLogChunk).toHaveBeenCalled()
    expect(api.onLogClosed).toHaveBeenCalled()
    select('docker:aaa')
    await flushPromises()
    expect(wrapper.find('aside').exists()).toBe(true)
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await flushPromises()
    expect(wrapper.find('aside').exists()).toBe(false)
    wrapper.unmount()
  })

  it('closes the settings dialog on Escape before the detail pane', async () => {
    vi.mocked(api.getSnapshot).mockResolvedValue(snapshot())
    vi.mocked(coreApi.getSettings).mockResolvedValue({
      pollIntervalMs: 3000, showAll: false, terminal: 'terminal-app',
      editorCommand: 'code', notifyOnStop: true, launchAtLogin: false, theme: 'system', worktreeRoots: [], worktreeExcludes: [],
      tunnelDomain: '', tunnelPrefix: '', tunnelHours: 4,
    })
    const wrapper = mount(App)
    await flushPromises()
    select('docker:aaa')
    await flushPromises()
    expect(wrapper.find('aside').exists()).toBe(true)
    await wrapper.find('[aria-label="Settings"]').trigger('click')
    expect(wrapper.find('form[role="dialog"]').exists()).toBe(true)
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await flushPromises()
    expect(wrapper.find('form[role="dialog"]').exists()).toBe(false)
    expect(wrapper.find('aside').exists()).toBe(true)
    wrapper.unmount()
  })

  it('keeps the detail pane when Escape comes from a field inside the settings dialog', async () => {
    vi.mocked(api.getSnapshot).mockResolvedValue(snapshot())
    vi.mocked(coreApi.getSettings).mockResolvedValue({
      pollIntervalMs: 3000, showAll: false, terminal: 'terminal-app',
      editorCommand: 'code', notifyOnStop: true, launchAtLogin: false, theme: 'system', worktreeRoots: [], worktreeExcludes: [],
      tunnelDomain: '', tunnelPrefix: '', tunnelHours: 4,
    })
    const wrapper = mount(App, { attachTo: document.body })
    await flushPromises()
    select('docker:aaa')
    await flushPromises()
    await wrapper.find('[aria-label="Settings"]').trigger('click')
    const input = wrapper.find('input[name="interval"]')
    expect(input.exists()).toBe(true)
    input.element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await flushPromises()
    expect(wrapper.find('form[role="dialog"]').exists()).toBe(false)
    expect(wrapper.find('aside').exists()).toBe(true)
    wrapper.unmount()
  })

  it('shows the Worktrees page instead of the services table after switching tool', async () => {
    vi.mocked(api.getSnapshot).mockResolvedValue(snapshot())
    vi.mocked(coreApi.getSettings).mockResolvedValue({
      pollIntervalMs: 3000, showAll: false, terminal: 'terminal-app',
      editorCommand: 'code', notifyOnStop: true, launchAtLogin: false, theme: 'system', worktreeRoots: [], worktreeExcludes: [],
      tunnelDomain: '', tunnelPrefix: '', tunnelHours: 4,
    })
    const wrapper = mount(App)
    await flushPromises()
    expect(wrapper.find('h1').text()).toBe('All listening')
    setTool('worktrees')
    await flushPromises()
    expect(wrapper.find('h1').text()).toBe('Worktrees')
    expect(wrapper.text()).not.toContain('Show all')
    wrapper.unmount()
  })
  const SETTINGS: Settings = {
    pollIntervalMs: 3000, showAll: false, terminal: 'terminal-app' as const,
    editorCommand: 'code', notifyOnStop: true, launchAtLogin: false, theme: 'system', worktreeRoots: [], worktreeExcludes: [],
      tunnelDomain: '', tunnelPrefix: '', tunnelHours: 4,
  }

  it('subscribes to worktrees events, loads the initial snapshot and unsubscribes on unmount', async () => {
    const offChanged = vi.fn()
    const offError = vi.fn()
    let changed: (s: never) => void = () => {}
    let failed: (m: string) => void = () => {}
    vi.mocked(worktreesApi.onChanged).mockImplementation(async (cb) => { changed = cb as never; return offChanged })
    vi.mocked(worktreesApi.onError).mockImplementation(async (cb) => { failed = cb; return offError })
    vi.mocked(worktreesApi.get).mockResolvedValue({ takenAt: 1, scanning: false, repos: [{ path: '/r/a', name: 'a', mainSize: null, worktrees: [] }] })
    vi.mocked(api.getSnapshot).mockResolvedValue(snapshot())
    vi.mocked(coreApi.getSettings).mockResolvedValue(SETTINGS)
    const wrapper = mount(App)
    await flushPromises()
    expect(useWorktrees().state.snapshot?.repos).toHaveLength(1)
    changed({ takenAt: 2, scanning: true, repos: [] } as never)
    expect(useWorktrees().state.snapshot?.takenAt).toBe(2)
    failed('scan boom')
    expect(useWorktrees().state.error).toBe('scan boom')
    wrapper.unmount()
    expect(offChanged).toHaveBeenCalled()
    expect(offError).toHaveBeenCalled()
  })

  it('keeps the other loads and reports the error when the worktrees subscription or initial load fails', async () => {
    vi.mocked(worktreesApi.onChanged).mockRejectedValue('listen boom')
    vi.mocked(worktreesApi.get).mockRejectedValue('get boom')
    vi.mocked(api.getSnapshot).mockResolvedValue(snapshot())
    vi.mocked(coreApi.getSettings).mockResolvedValue(SETTINGS)
    const wrapper = mount(App)
    await flushPromises()
    expect(useWorktrees().state.error).toBe('get boom')
    expect(useServices().state.snapshot).not.toBeNull()
    expect(useSettings().settings?.pollIntervalMs).toBe(3000)
    wrapper.unmount()
  })

  it('subscribes onError even when onChanged fails, and keeps a newer event snapshot over the initial get', async () => {
    let failed: (m: string) => void = () => {}
    vi.mocked(worktreesApi.onChanged).mockRejectedValue('listen boom')
    vi.mocked(worktreesApi.onError).mockImplementation(async (cb) => { failed = cb; return () => {} })
    vi.mocked(api.getSnapshot).mockResolvedValue(snapshot())
    vi.mocked(coreApi.getSettings).mockResolvedValue(SETTINGS)
    const wrapper = mount(App)
    await flushPromises()
    expect(worktreesApi.onError).toHaveBeenCalled()
    failed('late')
    expect(useWorktrees().state.error).toBe('late')
    wrapper.unmount()
  })

  it('ignores an initial worktrees snapshot older than one already received', async () => {
    let changed: (s: never) => void = () => {}
    vi.mocked(worktreesApi.onChanged).mockImplementation(async (cb) => { changed = cb as never; return () => {} })
    let resolveGet: (s: never) => void = () => {}
    vi.mocked(worktreesApi.get).mockReturnValue(new Promise((r) => { resolveGet = r as never }))
    vi.mocked(api.getSnapshot).mockResolvedValue(snapshot())
    vi.mocked(coreApi.getSettings).mockResolvedValue(SETTINGS)
    const wrapper = mount(App)
    await flushPromises()
    changed({ takenAt: 9, scanning: false, repos: [{ path: '/r/new', name: 'new', mainSize: null, worktrees: [] }] } as never)
    resolveGet({ takenAt: 3, scanning: false, repos: [] } as never)
    await flushPromises()
    expect(useWorktrees().state.snapshot?.takenAt).toBe(9)
    wrapper.unmount()
  })
})
