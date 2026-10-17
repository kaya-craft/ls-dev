import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import DetailPane from './DetailPane.vue'
import { applySnapshot, resetServices, select } from './store/services'
import { closeLogs } from './store/logs'
import { ctr, proc, snapshot } from '../../test/fixtures'
import { resetTunnels, useTunnels } from '../tunnels/store'
import { api } from './api'
import { isDrawer } from '../../shell/viewport'

vi.mock('./api', () => ({
  api: { logsOpen: vi.fn(), logsClose: vi.fn() },
}))

describe('DetailPane', () => {
  beforeEach(() => {
    resetServices()
    resetTunnels()
    closeLogs()
    vi.mocked(api.logsOpen).mockReset().mockResolvedValue(undefined)
    vi.mocked(api.logsClose).mockReset().mockResolvedValue(undefined)
    const snap = snapshot()
    applySnapshot(snap)
    select(snap.standalone[0].id)
  })

  it('closes the backend log stream when unmounted with Logs open', async () => {
    const wrapper = mount(DetailPane)
    await wrapper.findAll('nav button')[1].trigger('click')
    await flushPromises()
    expect(api.logsOpen).toHaveBeenCalledTimes(1)
    expect(api.logsClose).not.toHaveBeenCalled()
    wrapper.unmount()
    expect(api.logsClose).toHaveBeenCalledTimes(1)
  })

  it('renders as an overlay drawer below 1280 px and side by side above', async () => {
    isDrawer.value = true
    const wrapper = mount(DetailPane)
    expect(wrapper.find('[data-testid="detail-pane"]').classes()).toContain('absolute')
    isDrawer.value = false
    await wrapper.vm.$nextTick()
    expect(wrapper.find('[data-testid="detail-pane"]').classes()).not.toContain('absolute')
  })

  it('truncates process tree lines and keeps the full command in the title', () => {
    const s = snapshot()
    applySnapshot(s)
    select(s.processes[0].id)
    const wrapper = mount(DetailPane)
    const lines = wrapper.findAll('dd .truncate[title]')
    expect(lines.length).toBeGreaterThan(0)
    expect(lines[0].attributes('title')).toBe(s.processes[0].tree[0].command)
  })

  const shareButtons = (w: ReturnType<typeof mount>) => w.findAll('[data-testid="detail-actions"] button').filter((b) => b.text().startsWith('Share'))

  it('opens the share dialog from a listening process with its port, project and cwd', async () => {
    const s = snapshot()
    s.processes = [proc({ ports: [{ port: 4000, responsive: true }], project: 'api', cwd: '/w/api' })]
    applySnapshot(s)
    select(s.processes[0].id)
    const w = mount(DetailPane)
    const buttons = shareButtons(w)
    expect(buttons.map((b) => b.text())).toEqual(['Share publicly…'])
    await buttons[0].trigger('click')
    expect(useTunnels().state.share).toEqual({ open: true, port: 4000, project: 'api', cwd: '/w/api' })
  })

  it('offers one button per port, and none without a listening port', async () => {
    const s = snapshot()
    s.processes = [
      proc({ id: 'proc:1:/a', ports: [{ port: 3000, responsive: true }, { port: 3001, responsive: null }] }),
      proc({ id: 'proc:2:/b', ports: [] }),
    ]
    applySnapshot(s)
    select('proc:1:/a')
    const w = mount(DetailPane)
    expect(shareButtons(w).map((b) => b.text())).toEqual(['Share :3000', 'Share :3001'])
    await shareButtons(w)[1].trigger('click')
    expect(useTunnels().state.share.port).toBe(3001)
    select('proc:2:/b')
    await w.vm.$nextTick()
    expect(shareButtons(w)).toHaveLength(0)
  })

  it('shares a running container with a null cwd, not a stopped one', async () => {
    const s = snapshot()
    s.standalone = [ctr({ id: 'docker:r', name: 'pg', ports: [{ port: 5432, responsive: true }] }), ctr({ id: 'docker:s', name: 'old', state: 'exited', ports: [{ port: 6379, responsive: null }] })]
    applySnapshot(s)
    select('docker:r')
    const w = mount(DetailPane)
    await shareButtons(w)[0].trigger('click')
    expect(useTunnels().state.share).toEqual({ open: true, port: 5432, project: 'pg', cwd: null })
    select('docker:s')
    await w.vm.$nextTick()
    expect(shareButtons(w)).toHaveLength(0)
  })
})
